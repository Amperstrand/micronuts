//! M1 scaffold entry: NVS up, the ProofStore live, a diagnostic console.

use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

// Env-only credentials (never committed): the esp32-mint pattern.
const WIFI_SSID: Option<&str> = option_env!("MICRONUTS_WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("MICRONUTS_WIFI_PASS");
    const MINT: &str = "http://192.168.13.221:8383";

use cashu_core_lite::store::ProofStore as _;
use micronuts_esp32_wallet::{NvsProofStore, WALLET_KEY};

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();
    // Default main-task stack is 3.5 KB — WiFi+TLS+k256 need 64 KB.
    // sdkconfig.defaults isn't picked up by the embuild chain yet, so
    // spawn the real work on a thread with explicit stack size.
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(run)?
        .join()
        .map_err(|e| anyhow::anyhow!("main thread panicked"))??;
    Ok(())
}

fn run() -> anyhow::Result<()> {
    // The console thread doubles as the crypto worker; at default priority
    // a minutes-long k256 swap pins the CPU and starves the network stack
    // (bench 2026-09-27: the wifi association DROPPED mid-receive — WDT
    // flagged IDLE starvation, hostapd saw the client vanish). Priority 1
    // keeps wifi/lwIP (>=19) always preempting: beacons flow through
    // compute-heavy stretches.
    unsafe {
        esp_idf_svc::hal::sys::vTaskPrioritySet(core::ptr::null_mut(), 1);
    }
    // 32 KiB fail-stop bound: wallet blobs that cannot fit NVS atomically
    // fail loudly here — never the nucula#1 silent RAM-only class.
    let partition = EspDefaultNvsPartition::take()?;
    let store_partition = partition.clone();
    let mut store = NvsProofStore::new(store_partition.clone(), WALLET_KEY, 32 * 1024)
        .map_err(|e| anyhow::anyhow!("store: {e:?}"))?;

    println!("micronuts-esp32-wallet M1 scaffold");
    println!(
        "stored blob: {} B",
        store.stored_len().map_err(|e| anyhow::anyhow!("{e:?}"))?
    );

    // Round-trip smoke through the full ProofStore contract.
    let payload: Vec<u8> = (0..64u8).collect();
    store.save(&payload).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let back = store.load().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    assert_eq!(back.as_deref(), Some(payload.as_slice()));
    println!("store round-trip: OK (64 B)");

    let mut wifi = {
        let peripherals = Peripherals::take()?;
        micronuts_esp32_wallet::wifi::WifiManager::new(peripherals.modem, partition)?
    };
    let (ssid, pass) = match (WIFI_SSID, WIFI_PASS) {
        (Some(s), Some(p)) => (s, p),
        _ => anyhow::bail!("MICRONUTS_WIFI_SSID/PASS not set at build time"),
    };
    // One join can wedge in the event layer while the router sees a
    // completed DHCP (bench 2026-09-27) — retry the full cycle.
    if let Err(e) = wifi.scan_dump() {
        println!("wifi: scan failed: {e}");
    }
    let mut joined = false;
    for attempt in 1..=3u8 {
        println!("wifi: join attempt {attempt}");
        match wifi.connect(ssid, pass) {
            Ok(()) => {
                joined = true;
                break;
            }
            Err(e) => println!("wifi: attempt {attempt} failed: {e}"),
        }
    }
    if !joined {
        anyhow::bail!("wifi: all join attempts failed");
    }
    println!("wifi connected");

    // Full typed MintClient through the wallet-core wire protocol
    use cashu_core_lite::transport::MintClient as _;
    use micronuts_esp32_wallet::http_transport;
    use micronuts_wallet_core::engine::WalletEngine;
    let mut mint = http_transport::esp_idf_mint_client(MINT);
    let keys = mint.get_keys()?;
    let total: usize = keys.keysets.iter().map(|ks| ks.keys.len()).sum();
    println!(
        "NUT-01 get_keys: {} keysets, {} keys total",
        keys.keysets.len(),
        total
    );
    let info = mint.get_info()?;
    println!("NUT-06 get_info: {}", info.name.as_str());

    // Seed: generate on first boot, persist in NVS under its own key
    // ("seed") — the M1 smoke test keeps using the "wallet_blob" slot, so
    // the wallet identity survives every boot.
    // 32 bytes per the WalletEngine contract.
    let mut seed_store = NvsProofStore::new(store_partition.clone(), "seed", 64)
        .map_err(|e| anyhow::anyhow!("seed store: {e:?}"))?;
    let mut seed_buf = [0u8; 32];
    match seed_store.load() {
        Ok(Some(blob)) if blob.len() == 32 => {
            seed_buf.copy_from_slice(&blob);
            println!("seed: loaded from NVS");
        }
        _ => {
            // Generate from hardware RNG
            unsafe {
                esp_idf_svc::hal::sys::esp_fill_random(
                    seed_buf.as_mut_ptr() as *mut core::ffi::c_void,
                    32,
                )
            };
            seed_store
                .save(&seed_buf)
                .map_err(|e| anyhow::anyhow!("seed save: {e:?}"))?;
            println!("seed: generated + persisted");
        }
    }

    // The full WalletEngine: money operations through the same
    // code path as the host and wasm wallets.
    let mut engine = {
        use cashu_core_lite::transport::MintClient as _;
        let transport = http_transport::esp_idf_mint_client(MINT);
        // EspDefaultNvsPartition::take() yields the singleton once — the
        // smoke store's clone is reused here instead of a second take
        // (which aborts app_main with ESP_ERR_INVALID_STATE).
        let store2 = NvsProofStore::new(store_partition.clone(), "engine_state", 32 * 1024)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let engine_store_seed: [u8; 32] = seed_buf;
        WalletEngine::new(MINT, transport, store2, engine_store_seed, Vec::new())
            .map_err(|e| anyhow::anyhow!("engine: {e:?}"))?
    };

    // hostapd deauths a silent station (~30-60s inactivity) — and the
    // k256 swap stretches run minutes with zero TX (bench 2026-09-27:
    // every long-compute phase ended in "Host is unreachable"). One UDP
    // packet every 8s feeds the AP's timer and keeps the gateway ARP
    // fresh, through any compute stretch.
    std::thread::Builder::new()
        .stack_size(4096)
        .spawn(|| {
            let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(8));
                if let Some(s) = &sock {
                    let _ = s.send_to(b"ka", "1.1.1.1:9");
                }
            }
        })?;

    let mut line = String::new();
    // Cache the keyset while the network is boot-fresh: an idle station
    // behind the portal can blackhole later SYNs (bench 2026-09-27: the
    // console `connect` wedged >45s post-boot while the identical calls
    // succeeded seconds earlier at boot).
    match engine.connect() {
        Ok(()) => println!("connected: mint={} keyset={}", engine.mint_url(), engine.keyset_id()),
        Err(e) => println!("connect error: {e:?}"),
    }
    let mut tollgate = micronuts_esp32_wallet::tollgate::TollgateClient::new();
    // A ~400-byte token pasted at 115200 overflows the 128-byte UART FIFO
    // whenever the poll loop sleeps: install the driver so stdin blocks on
    // a 2 KiB ring buffer instead.
    unsafe {
        use esp_idf_svc::hal::sys::{esp_vfs_dev_uart_use_driver, uart_driver_install};
        uart_driver_install(0, 2048, 0, 0, core::ptr::null_mut(), 0);
        esp_vfs_dev_uart_use_driver(0);
    }
    println!("nucula-mode wallet ready — type 'help'");
    loop {
        line.clear();
        // esp-idf stdin is non-blocking (VFS returns EAGAIN while idle):
        // poll-retry instead of letting the error kill app_main.
        loop {
            match std::io::stdin().read_line(&mut line) {
                Ok(_) if line.ends_with('\n') => break,
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(e) => return Err(e.into()),
            }
        }
        let trimmed = line.trim();
        match trimmed {
            "help" => {
                println!("help | connect | balance | heap | seed | p2pk | tollgate join/pay/status")
            }
            "connect" => match engine.connect() {
                Ok(()) => println!(
                    "connected: mint={} keyset={}",
                    engine.mint_url(),
                    engine.keyset_id()
                ),
                Err(e) => println!("connect error: {e:?}"),
            },
            "balance" => println!("balance: {} sats", engine.balance()),
            "heap" => println!("free: {} B", unsafe {
                esp_idf_svc::hal::sys::esp_get_free_heap_size()
            }),
            "seed" => println!("seed: {}", engine.seed_hex()),
            "p2pk" => println!("p2pk: {}", engine.p2pk_pubkey_hex()),
            "" => {}
            other => {
                if let Some(rest) = other.strip_prefix("receive ") {
                    let (token, preimage) = match rest.split_once(' ') {
                        Some((token, preimage)) => (token, Some(preimage)),
                        None => (rest, None),
                    };
                    let options = micronuts_wallet_core::engine::ReceiveOptions { preimage };
                    match engine.receive_token_with(token, options) {
                        Ok(amount) => {
                            println!("received {amount} sats, balance: {} sats", engine.balance())
                        }
                        Err(e) => println!("receive error: {e:?}"),
                    }
                } else if let Some(amount) = other.strip_prefix("send ") {
                    match amount.parse::<u64>() {
                        Ok(amt) => match engine.send_token(amt, None) {
                            Ok(token) => println!("sent {amt}: {token}"),
                            Err(e) => println!("send error: {e:?}"),
                        },
                        Err(_) => println!("send: invalid amount"),
                    }
                } else if let Some(rest) = other.strip_prefix("tollgate ") {
                    let mut args = rest.split_whitespace();
                    match args.next() {
                        Some("join") => {
                            match args.next() {
                                Some(ssid) => {
                                    if let Some(extra) = args.next() {
                                        println!("tollgate join: open networks only — unexpected '{extra}'");
                                    } else {
                                        match tollgate.join(&mut wifi, ssid) {
                                            Ok(()) => println!("tollgate: joined"),
                                            Err(e) => println!("tollgate error: {e}"),
                                        }
                                    }
                                }
                                None => println!("usage: tollgate join <ssid>"),
                            }
                        }
                        Some("pay") => {
                            let amount = match args.next() {
                                None => None,
                                Some(sats) => match sats.parse::<u64>() {
                                    Ok(amount) => Some(amount),
                                    Err(_) => {
                                        println!("tollgate pay: invalid amount");
                                        continue;
                                    }
                                },
                            };
                            match tollgate.pay(&mut engine, &wifi, amount) {
                                Ok(micronuts_esp32_wallet::tollgate::PayOutcome::AlreadyAuthed) => {
                                    println!("tollgate: already authenticated, nothing spent")
                                }
                                Ok(micronuts_esp32_wallet::tollgate::PayOutcome::Paid {
                                    amount,
                                    http_status,
                                }) => {
                                    println!(
                                        "tollgate: paid {amount} sats (HTTP {http_status}), internet verified — balance: {} sats",
                                        engine.balance()
                                    )
                                }
                                Err(e) => println!("tollgate error: {e}"),
                            }
                        }
                        Some("status") => {
                            println!("tollgate: {}", tollgate.status());
                            println!("tollgate: wallet balance {} sats", engine.balance());
                        }
                        _ => println!(
                            "usage: tollgate join <ssid> | tollgate pay [sats] | tollgate status"
                        ),
                    }
                } else {
                    println!("unknown: {other}");
                }
            }
        }
    }
}
