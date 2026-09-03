use esp_idf_hal::prelude::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use esp_idf_sys::link_patches;
use std::net::TcpListener;
use std::io::{Read, Write};

// #58: fail closed on unset credentials — no placeholder fallbacks in
// release-shaped binaries.
const WIFI_SSID: &str = match option_env!("MICRONUTS_WIFI_SSID") {
    Some(ssid) => ssid,
    None => panic!("MICRONUTS_WIFI_SSID not set — refusing to build with placeholder credentials"),
};
const WIFI_PASS: &str = match option_env!("MICRONUTS_WIFI_PASS") {
    Some(pass) => pass,
    None => panic!("MICRONUTS_WIFI_PASS not set — refusing to build with placeholder credentials"),
};
const TCP_PORT: u16 = 3333;

fn main() -> anyhow::Result<()> {
    link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    log::info!("Micronuts ESP32 WiFi Bridge starting...");

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    wifi.set_configuration(&esp_idf_svc::wifi::Configuration::Client(
        esp_idf_svc::wifi::ClientConfiguration {
            ssid: WIFI_SSID.try_into().unwrap(),
            password: WIFI_PASS.try_into().unwrap(),
            ..Default::default()
        },
    ))?;

    wifi.start()?;
    log::info!("WiFi started");
    wifi.connect()?;
    log::info!("WiFi connected");
    wifi.wait_netif_up()?;
    let ip = wifi.wifi().sta_netif().get_ip_info()?;
    log::info!("IP: {:?}", ip.ip);

    let listener = TcpListener::bind(("0.0.0.0", TCP_PORT))?;
    log::info!("TCP listener on port {}", TCP_PORT);

    loop {
        match listener.accept() {
            Ok((mut stream, addr)) => {
                log::info!("Client connected: {}", addr);
                let mut buf = [0u8; 512];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => { log::info!("Client disconnected"); break; }
                        Ok(n) => {
                            log::info!("Recv {} bytes: {:02x?}", n, &buf[..n]);
                            if let Err(e) = stream.write_all(&buf[..n]) {
                                log::warn!("Write error: {}", e);
                                break;
                            }
                        }
                        Err(e) => { log::warn!("Read error: {}", e); break; }
                    }
                }
            }
            Err(e) => log::warn!("Accept error: {}", e),
        }
    }
}
