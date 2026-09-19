//! M1 scaffold entry: NVS up, the ProofStore live, a diagnostic console.



use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

// Env-only credentials (never committed): the esp32-mint pattern.
const WIFI_SSID: Option<&str> = option_env!("MICRONUTS_WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("MICRONUTS_WIFI_PASS");
const MINT: &str = "http://192.168.13.221:3338";

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

    // 32 KiB fail-stop bound: wallet blobs that cannot fit NVS atomically
    // fail loudly here — never the nucula#1 silent RAM-only class.
    let partition = EspDefaultNvsPartition::take()?;
    let store_partition = partition.clone();
    let mut store = NvsProofStore::new(store_partition, WALLET_KEY, 32 * 1024).map_err(|e| anyhow::anyhow!("store: {e:?}"))?;

    println!("micronuts-esp32-wallet M1 scaffold");
    println!("stored blob: {} B", store.stored_len().map_err(|e| anyhow::anyhow!("{e:?}"))?);

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
    wifi.connect(ssid, pass)?;
    println!("wifi connected");

    // Full typed MintClient through the wallet-core wire protocol
    use cashu_core_lite::transport::MintClient as _;
    use micronuts_esp32_wallet::http_transport;
use micronuts_wallet_core::engine::WalletEngine;
    let mut mint = http_transport::esp_idf_mint_client(MINT);
    let keys = mint.get_keys()?;
    let total: usize = keys.keysets.iter().map(|ks| ks.keys.len()).sum();
    println!("NUT-01 get_keys: {} keysets, {} keys total", keys.keysets.len(), total);
    let info = mint.get_info()?;
    println!("NUT-06 get_info: {}", info.name.as_str());

    // Seed: generate on first boot, persist in NVS (key "seed").
    // 32 bytes per the WalletEngine contract.
    let seed_key = "wallet_seed";
    let mut seed_buf = [0u8; 32];
    match store.load() {
        Ok(Some(blob)) if blob.len() == 32 => {
            seed_buf.copy_from_slice(&blob);
            println!("seed: loaded from NVS");
        }
        _ => {
            // Generate from hardware RNG
            unsafe { esp_idf_svc::hal::sys::esp_fill_random(seed_buf.as_mut_ptr() as *mut core::ffi::c_void, 32) };
            store.save(&seed_buf).map_err(|e| anyhow::anyhow!("seed save: {e:?}"))?;
            println!("seed: generated + persisted");
        }
    }

    // The full WalletEngine: money operations through the same
    // code path as the host and wasm wallets.
    let mut engine = {
        use cashu_core_lite::transport::MintClient as _;
        let transport = http_transport::esp_idf_mint_client(MINT);
        let store2 = NvsProofStore::new(
            esp_idf_svc::nvs::EspDefaultNvsPartition::take()?,
            "engine_state",
            32 * 1024,
        ).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let engine_store_seed: [u8; 32] = seed_buf;
        WalletEngine::new(MINT, transport, store2, engine_store_seed, Vec::new())
            .map_err(|e| anyhow::anyhow!("engine: {e:?}"))?
    };

    let mut line = String::new();
    println!("nucula-mode wallet ready — type 'help'");
    loop {
        line.clear();
        std::io::stdin().read_line(&mut line)?;
        let trimmed = line.trim();
        match trimmed {
            "help" => println!("help | connect | balance | heap | seed"),
            "connect" => {
                match engine.connect() {
                    Ok(()) => println!("connected: mint={} keyset={}", engine.mint_url(), engine.keyset_id()),
                    Err(e) => println!("connect error: {e:?}"),
                }
            }
            "balance" => println!("balance: {} sats", engine.balance()),
            "heap" => println!("free: {} B", unsafe { esp_idf_svc::hal::sys::esp_get_free_heap_size() }),
            "seed" => println!("seed: {}", engine.seed_hex()),
            "" => {}
            other => {
                if let Some(token) = other.strip_prefix("receive ") {
                    match engine.receive_token(token) {
                        Ok(amount) => println!("received {amount} sats, balance: {} sats", engine.balance()),
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
                } else {
                    println!("unknown: {other}");
                }
            }
        }
    }
}
