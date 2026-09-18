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
    let mut mint = http_transport::esp_idf_mint_client(MINT);
    let keys = mint.get_keys()?;
    let total: usize = keys.keysets.iter().map(|ks| ks.keys.len()).sum();
    println!("NUT-01 get_keys: {} keysets, {} keys total", keys.keysets.len(), total);
    let info = mint.get_info()?;
    println!("NUT-06 get_info: {}", info.name.as_str());

    let mut line = String::new();
    println!("type 'help' for commands");
    loop {
        line.clear();
        std::io::stdin().read_line(&mut line)?;
        match line.trim() {
            "help" => println!("help | status | heap"),
            "status" => println!(
                "stored: {} B | bound: 32768 B | core: cashu-core-lite",
                store.stored_len().map_err(|e| anyhow::anyhow!("{e:?}"))?
            ),
            "heap" => println!("free: {} B", unsafe { esp_idf_svc::hal::sys::esp_get_free_heap_size() }),
            "" => {}
            other => println!("unknown: {other}"),
        }
    }
}
