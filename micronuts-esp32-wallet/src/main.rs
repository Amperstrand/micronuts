//! M1 scaffold entry: NVS up, the ProofStore live, a diagnostic console.

use embedded_svc::http::client::Client;

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

    let mut http = esp_idf_svc::http::client::EspHttpConnection::new(
        &esp_idf_svc::http::client::Configuration {
            buffer_size: Some(2048),
            ..Default::default()
        },
    )?;
    let mut client = Client::wrap(http);
    let headers: [(&str, &str); 0] = [];
    let mut request = client.request(embedded_svc::http::Method::Get, "http://192.168.13.221:3338/v1/keysets", &headers)?;
    let mut request = request.submit()?;
    let status = request.status();
    let mut body = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = request.read(&mut chunk)?;
        if n == 0 { break; }
        body.extend_from_slice(&chunk[..n]);
    }
    let preview = String::from_utf8_lossy(&body[..body.len().min(80)]);
    println!("GET {MINT}/v1/keysets -> {status}, {} bytes: {preview}", body.len());

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
