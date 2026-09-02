//! ESP32-resident micronuts Cashu mint (WiFi + HTTP).
//!
//! Status 2026-09-02: scaffold, host-prototype parity in progress.
//! - GET /v1/info, /v1/keys, /v1/keysets are fully wired to [`DemoMint`].
//! - POST routes (quotes/mint/swap/melt/checkstate) read the body with the
//!   house loop-read pattern and answer 501 until the JSON<->RPC mapping is
//!   unified with the audit-adapter (next milestone; the mapping functions
//!   are the same ones the host server uses).
//! Build/flash from THIS directory only (see README).

#[cfg(target_os = "espidf")]
use anyhow::Result;

#[cfg(target_os = "espidf")]
use esp_idf_hal::modem;
#[cfg(target_os = "espidf")]
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
#[cfg(target_os = "espidf")]
use esp_idf_svc::http::Method;
#[cfg(target_os = "espidf")]
use esp_idf_svc::hal::io::EspIOError;
#[cfg(target_os = "espidf")]
use log::{error, info};

#[cfg(target_os = "espidf")]
use micronuts_mint::DemoMint;

#[cfg(target_os = "espidf")]
use crate::json;

// WiFi credentials: build-time env with fallback (NVS-stored provisioning is
// the follow-up; hardcoding in source is accepted for the scaffold only).
#[cfg(target_os = "espidf")]
const WIFI_SSID: &str = match option_env!("MICRONUTS_WIFI_SSID") {
    Some(ssid) => ssid,
    None => "YOUR_SSID",
};
#[cfg(target_os = "espidf")]
const WIFI_PASS: &str = match option_env!("MICRONUTS_WIFI_PASS") {
    Some(pass) => pass,
    None => "YOUR_PASSWORD",
};

#[cfg(target_os = "espidf")]
const HTTP_PORT: u16 = 3338;

#[cfg(target_os = "espidf")]
fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;
    let mut wifi = micronuts_esp32_mint::wifi::WifiManager::new(peripherals.modem)?;

    // House lesson: give WiFi association minutes, not seconds, before
    // declaring failure; retry in a bounded loop instead of panicking.
    let mut connected = false;
    for attempt in 1..=5 {
        match wifi.connect(WIFI_SSID, WIFI_PASS) {
            Ok(()) => {
                connected = true;
                break;
            }
            Err(e) => error!("WiFi connect attempt {attempt} failed: {e}"),
        }
    }
    if !connected {
        anyhow::bail!("WiFi association failed after 5 attempts");
    }

    let mint = std::sync::Arc::new(std::sync::Mutex::new(DemoMint::new()));
    info!("DemoMint up (keyset {})", mint.lock().unwrap().keyset_id());

    // 32 KiB handler stacks: token parsing + secp256k1 overflowed 12 KiB in
    // the house flagship for the same workload class.
    let mut server = EspHttpServer::new(&HttpConfig {
        http_port: HTTP_PORT,
        max_uri_handlers: 16,
        stack_size: 32768,
        ..Default::default()
    })?;

    register_get(&mut server, mint.clone(), "/v1/info", |m| json::info_body(m))?;
    register_get(&mut server, mint.clone(), "/v1/keys", |m| json::keys_body(m))?;
    register_get(&mut server, mint.clone(), "/v1/keysets", |m| json::keysets_body(m))?;

    // POST endpoints: honest 501 stubs until the adapter JSON mapping is
    // shared. Body is drained with the loop-read pattern so the exact
    // handler shape is already in place.
    for route in [
        "/v1/mint/quote/bolt11",
        "/v1/mint/bolt11",
        "/v1/swap",
        "/v1/melt/quote/bolt11",
        "/v1/melt/bolt11",
        "/v1/checkstate",
    ] {
        server.fn_handler(route, Method::Post, |mut request| {
            drain_body(&mut request);
            let mut resp = request.into_response(501, "Not Implemented", &[])?;
            resp.write(json::error_body(501, "route not yet wired on device").as_bytes())?;
            Ok::<(), EspIOError>(())
        })?;
    }

    info!("micronuts-esp32-mint ready on :{HTTP_PORT}");

    // Health loop: watch link + heap; reconnect on drop (flagship pattern).
    loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        if !wifi.is_connected() {
            info!("WiFi down — reconnecting");
            let _ = wifi.connect(WIFI_SSID, WIFI_PASS);
        }
    }
}

#[cfg(target_os = "espidf")]
fn register_get<F>(
    server: &mut EspHttpServer,
    mint: std::sync::Arc<std::sync::Mutex<DemoMint>>,
    route: &str,
    build: F,
) -> Result<()>
where
    F: Fn(&DemoMint) -> String + Send + 'static,
{
    server.fn_handler(route, Method::Get, move |request| {
        let body = match mint.lock() {
            Ok(mint) => build(&mint),
            Err(_) => json::error_body(503, "mint lock poisoned"),
        };
        let mut resp = request.into_ok_response()?;
        resp.write(body.as_bytes())?;
        Ok::<(), EspIOError>(())
    })?;
    Ok(())
}

#[cfg(target_os = "espidf")]
fn drain_body(request: &mut esp_idf_svc::http::Request<&mut [u8]>) {
    let mut chunk = [0u8; 512];
    let mut seen = 0usize;
    loop {
        match request.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                seen += n;
                if seen > 16 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
