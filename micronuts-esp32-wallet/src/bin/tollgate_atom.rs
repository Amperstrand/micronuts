//! tollgate_atom — the M5 Atom (ESP32-PICO + 5×5 SK6812) TollGate
//! client (micronuts: the ONLY embedded TollGate client; the NutBar
//! C firmware is decommissioned).
//!
//! State machine: SCAN → FOUND → JOIN → VALIDATE → PAY → MONITOR →
//! rescan. Every phase transition prints a PRTA line for the rig:
//! `TG_FOUND` / `TG_AD_VALID` / `TG_PAID` / `TG_INTERNET` /
//! `TG_DISCONNECTED`.
//!
//! Token funding is USB-serial only (no camera): paste
//! `token <cashuA...>` (or `receive <...>`) at 115200 baud — the UART
//! runs on a 2 KiB driver ring buffer because a ~400-byte token paste
//! overflows the 128-byte FIFO otherwise (main.rs lesson).
//!
//! LEDs: red blink = scanning; yellow pulse = validating; green solid
//! + data meter = session active; blue breathing = paying/renewing;
//! purple = balance can't fund the next renewal.

use std::io::{BufRead as _, Read as _};
use std::sync::mpsc::{Receiver, RecvTimeoutError};

use micronuts_esp32_wallet::http_transport;
use micronuts_esp32_wallet::led_matrix::{AtomState, LedMatrix};
use micronuts_esp32_wallet::tollgate::TollgateClient;
use micronuts_esp32_wallet::wifi::WifiManager;
use micronuts_wallet_core::engine::WalletEngine;

use cashu_core_lite::store::ProofStore;
use cashu_core_lite::transport::MintClient;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use micronuts_esp32_wallet::NvsProofStore;

const SSID_PREFIX: &str = "TollGate-";
const MINT: &str = match option_env!("MICRONUTS_MINT_URL") {
    Some(url) => url,
    None => "http://192.168.13.253:8383",
};
const SCAN_PERIOD_MS: u64 = 30_000;
const USAGE_POLL_MS: u64 = 5_000;
const RENEW_THRESHOLD_PCT: u64 = 20;
/// Balance below this (in renewals) lights purple.
const LOW_BALANCE_RENEWALS: u64 = 2;
const LED_TICK_MS: u64 = 25;

fn prta(event: &str) {
    println!("TG_{event}");
}

fn spawn_stdin_reader() -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(6144)
        .spawn(move || {
            let mut buf = [0u8; 64];
            let mut line = String::new();
            let mut stdin = std::io::stdin().lock();
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Ok(n) => {
                        line.push_str(&String::from_utf8_lossy(&buf[..n]));
                        while let Some(pos) = line.find('\n') {
                            let taken: String = line.drain(..=pos).collect();
                            let _ = tx.send(taken.trim().to_string());
                        }
                        if line.len() > 4096 {
                            line.clear();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        })
        .expect("stdin reader");
    rx
}

fn engine_balance<T: MintClient + Clone, S: ProofStore>(engine: &WalletEngine<T, S>) -> u64 {
    engine.balance()
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();

    // UART ring buffer first: token pastes arrive before anything else.
    unsafe {
        use esp_idf_svc::hal::sys::{esp_vfs_dev_uart_use_driver, uart_driver_install};
        uart_driver_install(0, 2048, 0, 0, core::ptr::null_mut(), 0);
        esp_vfs_dev_uart_use_driver(0);
    }

    let peripherals = Peripherals::take()?;
    let store_partition = EspDefaultNvsPartition::take()?;

    let mut leds = LedMatrix::new(peripherals.pins.gpio27)?;
    leds.set_state(AtomState::Scanning);
    leds.render().ok();

    let modem = peripherals.modem;
    let mut wifi = WifiManager::new(modem, store_partition.clone())?;

    // Seed: generate on first boot, persist under "seed" (main.rs
    // pattern — wallet identity survives every boot).
    let mut seed_store = NvsProofStore::new(store_partition.clone(), "seed", 64)
        .map_err(|e| anyhow::anyhow!("seed store: {e:?}"))?;
    let mut seed_buf = [0u8; 32];
    match seed_store.load() {
        Ok(Some(blob)) if blob.len() == 32 => seed_buf.copy_from_slice(&blob),
        _ => unsafe {
            esp_idf_svc::hal::sys::esp_fill_random(
                seed_buf.as_mut_ptr() as *mut core::ffi::c_void,
                32,
            )
        },
    }
    seed_store
        .save(&seed_buf)
        .map_err(|e| anyhow::anyhow!("seed save: {e:?}"))?;

    let transport = http_transport::esp_idf_mint_client(MINT);
    let engine_store = NvsProofStore::new(store_partition.clone(), "engine_state", 32 * 1024)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut engine =
        WalletEngine::new(MINT, transport, engine_store, seed_buf, Vec::new())
            .map_err(|e| anyhow::anyhow!("engine: {e:?}"))?;
    println!(
        "tollgate_atom: mint={MINT} balance={} sats — fund with `token <cashu-token>`",
        engine_balance(&engine)
    );

    // hostapd deauths a silent station; keep the association warm.
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

    let serial = spawn_stdin_reader();
    let mut tollgate = TollgateClient::new();

    let mut phase = Phase::Scan;
    let mut since_scan_ms: u64 = SCAN_PERIOD_MS; // scan immediately
    let mut since_usage_ms: u64 = 0;
    let mut price_sats: u64 = 0;
    let mut frame_counter: u64 = 0;

    loop {
        FreeRtos::delay_ms(LED_TICK_MS as u32);
        frame_counter += 1;
        leds.tick(LED_TICK_MS);

        // Serial input: token funding + status queries.
        match serial.recv_timeout(std::time::Duration::from_millis(1)) {
            Ok(line) if line.starts_with("token ") || line.starts_with("receive ") => {
                let token = line.split_once(' ').map(|(_, t)| t.trim()).unwrap_or("");
                match engine.receive_token(token) {
                    Ok(amount) => println!(
                        "tollgate_atom: received {amount} sats — balance {} sats",
                        engine_balance(&engine)
                    ),
                    Err(e) => println!("tollgate_atom: receive error: {e:?}"),
                }
            }
            Ok(line) if line == "balance" => {
                println!("tollgate_atom: balance {} sats", engine_balance(&engine));
            }
            Ok(line) if line == "status" => {
                println!("tollgate_atom: {}", tollgate.status());
                println!("tollgate_atom: phase {phase:?}");
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!("serial reader died"),
        }

        match phase {
            Phase::Scan => {
                leds.set_state(AtomState::Scanning);
                since_scan_ms += LED_TICK_MS;
                if since_scan_ms >= SCAN_PERIOD_MS {
                    since_scan_ms = 0;
                    if let Some((ssid, rssi)) = wifi.scan_find_open(SSID_PREFIX)? {
                        println!("tollgate_atom: {ssid} ({rssi} dBm)");
                        prta("FOUND");
                        phase = Phase::Join { ssid: ssid.to_string() };
                    }
                }
            }
            Phase::Join { ssid } => {
                leds.set_state(AtomState::Validating);
                match wifi.connect(&ssid, "") {
                    Ok(()) => phase = Phase::Validate,
                    Err(e) => {
                        println!("tollgate_atom: join {ssid} failed: {e}");
                        phase = Phase::Scan;
                    }
                }
            }
            Phase::Validate => {
                leds.set_state(AtomState::Validating);
                match tollgate.discover(&wifi) {
                    Ok(_) => match tollgate.ad_valid() {
                        Ok(ad) => {
                            price_sats = ad.price_sats.unwrap_or(0);
                            if let Some(ad_mint) = &ad.mint_url {
                                if ad_mint != MINT {
                                    println!(
                                        "tollgate_atom: WARN ad mint {ad_mint} != wallet mint {MINT}"
                                    );
                                }
                            }
                            println!(
                                "tollgate_atom: ad valid — gw {} {price_sats} sats/step",
                                ad.gateway
                            );
                            prta("AD_VALID");
                            match tollgate.usage() {
                                Ok(Some((remaining, total))) if remaining > 0 => {
                                    leds.set_meter(remaining, total);
                                    prta("INTERNET");
                                    phase = Phase::Monitor;
                                }
                                _ => phase = Phase::Pay,
                            }
                        }
                        Err(e) => {
                            println!("tollgate_atom: ad invalid: {e}");
                            prta("DISCONNECTED");
                            phase = Phase::Scan;
                        }
                    },
                    Err(e) => {
                        println!("tollgate_atom: discovery failed: {e}");
                        prta("DISCONNECTED");
                        phase = Phase::Scan;
                    }
                }
            }
            Phase::Pay => {
                leds.set_state(AtomState::Paying);
                let balance = engine_balance(&engine);
                if balance < price_sats * LOW_BALANCE_RENEWALS {
                    leds.set_state(AtomState::LowBalance);
                    println!(
                        "tollgate_atom: low balance {balance} sats (< {}/{LOW_BALANCE_RENEWALS} renewals) — fund with `token <cashu-token>`",
                        price_sats * LOW_BALANCE_RENEWALS
                    );
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
                match tollgate.pay(&mut engine, &wifi, Some(price_sats)) {
                    Ok(_) => {
                        prta("PAID");
                        if let Ok(Some((remaining, total))) = tollgate.usage() {
                            leds.set_meter(remaining, total);
                        }
                        prta("INTERNET");
                        phase = Phase::Monitor;
                    }
                    Err(e) => {
                        println!("tollgate_atom: pay failed: {e}");
                        prta("DISCONNECTED");
                        phase = Phase::Scan;
                    }
                }
            }
            Phase::Monitor => {
                leds.set_state(AtomState::Active);
                if !wifi.is_connected() {
                    prta("DISCONNECTED");
                    leds.clear_meter();
                    phase = Phase::Scan;
                    continue;
                }
                since_usage_ms += LED_TICK_MS;
                if since_usage_ms >= USAGE_POLL_MS {
                    since_usage_ms = 0;
                    match tollgate.usage() {
                        Ok(Some((remaining, total))) => {
                            leds.set_meter(remaining, total);
                            let threshold = total * RENEW_THRESHOLD_PCT / 100;
                            if remaining <= threshold {
                                println!("tollgate_atom: {remaining}/{total} ms left — renewing");
                                phase = Phase::Pay;
                            }
                        }
                        Ok(None) | Err(_) => {
                            // backend hiccup: one missed poll is not a
                            // disconnect; two consecutive handled by
                            // is_connected next round.
                        }
                    }
                }
            }
        }

        if frame_counter % 2 == 0 {
            leds.render().ok();
        }
    }
}

#[derive(Debug)]
enum Phase {
    Scan,
    Join { ssid: String },
    Validate,
    Pay,
    Monitor,
}
