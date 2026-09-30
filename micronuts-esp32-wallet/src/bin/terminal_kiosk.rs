//! terminal_kiosk — the self-contained TollGate terminal on the CYD
//! (micronuts#79): wifi + display + live invoices on ONE device, no rig
//! bridge. Boots → joins the funding network (baked creds) → discovers
//! the TollGate backend on the DHCP gateway → creates real invoices →
//! renders the QR on the ST7796 → polls until granted → PAID screen →
//! repeats. Every phase is serial-logged for rig verification.
//!
//! Rig rule: a kiosk never exits — wifi/discovery/invoice failures draw
//! an error screen and retry; only display-init failure is fatal (a
//! terminal with a dead panel is not a terminal).

use std::io::BufRead as _;

use embedded_graphics::{
    mono_font::{MonoTextStyleBuilder, ascii::{FONT_10X20, FONT_6X10}},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Rectangle, PrimitiveStyleBuilder},
    text::{Baseline, Text, TextStyleBuilder},
};
use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::http::Status;
use esp_idf_svc::{
    hal::{
        delay::FreeRtos,
        gpio::{Gpio12, PinDriver},
        peripherals::Peripherals,
        spi::{
            Dma, SpiDeviceDriver, SpiDriver,
            config::{Config, DriverConfig},
        },
        units::FromValueType,
    },
    http::client::EspHttpConnection,
    nvs::EspDefaultNvsPartition,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7796,
    options::{ColorInversion, ColorOrder, Orientation},
};

use micronuts_esp32_wallet::wifi::WifiManager;

const WIFI_SSID: Option<&str> = option_env!("MICRONUTS_WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("MICRONUTS_WIFI_PASS");
const BACKEND_PORT: u16 = 2121;
const INVOICE_AMOUNT: u64 = 21;
/// The venue backend's configured mint — string-matched by /ln-invoice.
const MINT_URL: &str = "http://10.99.99.2:8383";
const POLL_SECS: u64 = 180;

const W: u32 = 320;
const H: u32 = 480;

static mut SPI_BUFFER: [u8; 4096] = [0u8; 4096];

const BLACK: Rgb565 = Rgb565::BLACK;
const WHITE: Rgb565 = Rgb565::WHITE;
const GREEN: Rgb565 = Rgb565::CSS_DARK_GREEN;
const AMBER: Rgb565 = Rgb565::new(60, 44, 2);
const RED: Rgb565 = Rgb565::RED;
const BLUE: Rgb565 = Rgb565::CSS_DARK_BLUE;

type OutPin = PinDriver<'static, esp_idf_svc::hal::gpio::Output>;
type Display = mipidsi::Display<
    SpiInterface<'static, SpiDeviceDriver<'static, SpiDriver<'static>>, OutPin>,
    ST7796,
    mipidsi::NoResetPin,
>;

struct Leds {
    r: OutPin,
    g: OutPin,
    b: OutPin,
}

impl Leds {
    fn set(&mut self, r: bool, g: bool, b: bool) {
        let _ = if r { self.r.set_low() } else { self.r.set_high() };
        let _ = if g { self.g.set_low() } else { self.g.set_high() };
        let _ = if b { self.b.set_low() } else { self.b.set_high() };
    }
}

fn wrap<T, E: core::fmt::Debug>(r: Result<T, E>) -> anyhow::Result<()> {
    r.map(|_| ()).map_err(|e| anyhow::anyhow!("{e:?}"))
}

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(run)?
        .join()
        .map_err(|_| anyhow::anyhow!("kiosk thread panicked"))?
}

struct Kiosk {
    display: Display,
    led: Leds,
    backend: String,
}

fn run() -> anyhow::Result<()> {
    let p = Peripherals::take()?;

    let mut bl = PinDriver::output(p.pins.gpio27)?;
    bl.set_high()?;

    let mut led = Leds {
        r: PinDriver::output(p.pins.gpio4)?,
        g: PinDriver::output(p.pins.gpio16)?,
        b: PinDriver::output(p.pins.gpio17)?,
    };

    let spi_driver = SpiDriver::new(
        p.spi2,
        p.pins.gpio14,
        p.pins.gpio13,
        None::<Gpio12>,
        &DriverConfig::new().dma(Dma::Channel1(64 * 1024)),
    )?;
    let spi_device = SpiDeviceDriver::new(
        spi_driver,
        Some(p.pins.gpio15),
        &Config::new().baudrate(40.MHz().into()),
    )?;
    let dc = PinDriver::output(p.pins.gpio2)?;
    let buffer: &'static mut [u8; 4096] = unsafe { &mut *core::ptr::addr_of_mut!(SPI_BUFFER) };
    let di = SpiInterface::new(spi_device, dc, buffer);

    println!("kiosk: mipidsi init (ST7796 320x480)");
    let mut display: Display = Builder::new(ST7796, di)
        .display_size(W as u16, H as u16)
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::default().flip_horizontal())
        .init(&mut FreeRtos)
        .map_err(|e| anyhow::anyhow!("init: {e:?}"))?;

    let (ssid, pass) = match (WIFI_SSID, WIFI_PASS) {
        (Some(s), Some(w)) => (s, w),
        _ => anyhow::bail!("kiosk: MICRONUTS_WIFI_SSID/PASS not set at build time"),
    };

    let mut wifi = {
        let partition = EspDefaultNvsPartition::take()?;
        WifiManager::new(p.modem, partition)?
    };

    // Phase 1: network. Retry forever — a kiosk waiting for its network
    // is normal life, not an error.
    loop {
        draw_status(&mut display, "CONNECTING", ssid, BLUE)?;
        led.set(false, false, true);
        println!("kiosk: joining '{ssid}'");
        match wifi.connect(ssid, pass) {
            Ok(()) => break,
            Err(e) => {
                println!("kiosk: wifi: {e} — retry in 10s");
                draw_status(&mut display, "WIFI RETRY", &format!("{e}"), RED)?;
                led.set(true, false, false);
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
    }
    let gateway = wifi.gateway_ip()?;
    let backend = format!("http://{gateway}:{BACKEND_PORT}");
    println!("kiosk: wifi up, gateway {gateway}, backend {backend}");

    let mut kiosk = Kiosk { display, led, backend };

    // Phase 2: discovery. Wait for a TollGate on the gateway.
    loop {
        draw_status(&mut kiosk.display, "DISCOVERING", &kiosk.backend, BLUE)?;
        println!("kiosk: discovering {}", kiosk.backend);
        match http_get(&kiosk.backend, "/") {
            Ok((status, _body)) if status == 200 => break,
            other => {
                println!("kiosk: discovery {other:?} — retry in 10s");
                draw_status(&mut kiosk.display, "NO TOLLGATE", "retrying…", RED)?;
                kiosk.led.set(true, false, false);
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
    }
    println!("kiosk: TollGate found — terminal live");

    // Phase 3: the kiosk loop — invoice → QR → poll → PAID → repeat.
    loop {
        match invoice_cycle(&mut kiosk) {
            Ok(()) => {}
            Err(e) => {
                println!("kiosk: cycle error: {e:#}");
                draw_status(&mut kiosk.display, "BACKEND ERROR", &format!("{e:#}"), RED)?;
                kiosk.led.set(true, false, false);
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(8));
    }
}

/// One full invoice lifecycle. Err means the cycle should restart.
fn invoice_cycle(k: &mut Kiosk) -> anyhow::Result<()> {
    draw_status(&mut k.display, "CREATING INVOICE", &format!("{INVOICE_AMOUNT} sats"), BLUE)?;
    k.led.set(false, false, true);
    let req = serde_json::json!({"amount": INVOICE_AMOUNT, "mint_url": MINT_URL});
    let (status, resp) = http_post(&k.backend, "/ln-invoice", &req)?;
    if status != 200 {
        anyhow::bail!("invoice HTTP {status}: {}", String::from_utf8_lossy(&resp));
    }
    let v: serde_json::Value = serde_json::from_slice(&resp)?;
    let quote = v["quote"].as_str().ok_or_else(|| anyhow::anyhow!("no quote in response"))?.to_string();
    let bolt11 = v["invoice"].as_str().ok_or_else(|| anyhow::anyhow!("no invoice in response"))?.to_string();
    println!("kiosk: invoice {quote} ({} chars)", bolt11.len());

    // QR screen
    let code = qrcode::QrCode::with_error_correction_level(bolt11.as_bytes(), qrcode::EcLevel::L)
        .map_err(|e| anyhow::anyhow!("qr: {e:?}"))?;
    clear(&mut k.display)?;
    header(&mut k.display, "TOLLGATE")?;
    body(&mut k.display, 48, &format!("{INVOICE_AMOUNT} sats — scan to pay"))?;
    draw_qr(&mut k.display, 80, &code)?;
    chip(&mut k.display, 400, "AWAITING PAYMENT", BLACK, AMBER)?;
    body(&mut k.display, 444, "self-contained kiosk")?;
    k.led.set(true, false, false);

    // Poll until granted or timeout.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(POLL_SECS);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let path = format!("/ln-invoice?quote={quote}");
        let (status, resp) = http_get(&k.backend, &path)?;
        if status != 200 {
            continue;
        }
        let v: serde_json::Value = serde_json::from_slice(&resp)?;
        if v["access_granted"].as_bool() == Some(true) {
            let allotment = v["allotment"].as_u64().unwrap_or(0);
            let metric = v["metric"].as_str().unwrap_or("bytes").to_string();
            println!("kiosk: PAID — {INVOICE_AMOUNT} sats, {allotment} {metric} granted");
            draw_paid(&mut k.display, INVOICE_AMOUNT, allotment, &metric)?;
            k.led.set(false, true, false);
            return Ok(());
        }
    }
    println!("kiosk: invoice {quote} not granted in {POLL_SECS}s — cycling");
    Ok(())
}

// ---- drawing (lifted from the proven terminal_demo implementation) ----

fn clear(d: &mut Display) -> anyhow::Result<()> {
    wrap(d.fill_solid(&Rectangle::new(Point::new(0, 0), Size::new(W, H)), BLACK))
}

fn header(d: &mut Display, title: &str) -> anyhow::Result<()> {
    wrap(d.fill_solid(&Rectangle::new(Point::new(0, 0), Size::new(W, 40)), BLACK))?;
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(WHITE)
        .background_color(BLACK)
        .build();
    wrap(Text::with_text_style(
        title,
        Point::new(8, 8),
        style,
        TextStyleBuilder::new().baseline(Baseline::Top).build(),
    )
    .draw(d))
}

fn chip(d: &mut Display, y: i32, text: &str, fg: Rgb565, bg: Rgb565) -> anyhow::Result<()> {
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, y), Size::new(W - 16, 36)), bg))?;
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(fg)
        .background_color(bg)
        .build();
    wrap(Text::with_text_style(
        text,
        Point::new(16, y + 8),
        style,
        TextStyleBuilder::new().baseline(Baseline::Top).build(),
    )
    .draw(d))
}

fn body(d: &mut Display, y: i32, text: &str) -> anyhow::Result<()> {
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(WHITE)
        .background_color(BLACK)
        .build();
    wrap(Text::with_text_style(
        text,
        Point::new(8, y),
        style,
        TextStyleBuilder::new().baseline(Baseline::Top).build(),
    )
    .draw(d))
}

fn draw_status(d: &mut Display, title: &str, detail: &str, accent: Rgb565) -> anyhow::Result<()> {
    clear(d)?;
    header(d, "TOLLGATE TERMINAL")?;
    chip(d, 60, title, WHITE, accent)?;
    body(d, 130, detail)?;
    Ok(())
}

fn draw_qr(d: &mut Display, top: i32, code: &qrcode::QrCode) -> anyhow::Result<()> {
    let n = code.width() as u32;
    let scale = (W - 32) / n;
    let px_size = n * scale;
    let x0 = ((W - px_size) / 2) as i32;
    wrap(d.fill_solid(
        &Rectangle::new(Point::new(x0 - 8, top - 8), Size::new(px_size + 16, px_size + 16)),
        WHITE,
    ))?;
    struct LivePx<'a> {
        code: &'a qrcode::QrCode,
        n: u32,
        scale: u32,
        row: u32,
        rep: u32,
        col: u32,
        k: u32,
    }
    impl Iterator for LivePx<'_> {
        type Item = Rgb565;
        fn next(&mut self) -> Option<Rgb565> {
            if self.row >= self.n {
                return None;
            }
            let dark = self.code[(self.col as usize, self.row as usize)] == qrcode::Color::Dark;
            let px = if dark { BLACK } else { WHITE };
            self.k += 1;
            if self.k >= self.scale {
                self.k = 0;
                self.col += 1;
                if self.col >= self.n {
                    self.col = 0;
                    self.rep += 1;
                    if self.rep >= self.scale {
                        self.rep = 0;
                        self.row += 1;
                        std::thread::yield_now();
                    }
                }
            }
            Some(px)
        }
    }
    let area = Rectangle::new(Point::new(x0, top), Size::new(px_size, px_size));
    wrap(d.fill_contiguous(&area, LivePx { code, n, scale, row: 0, rep: 0, col: 0, k: 0 }))
}

fn draw_paid(d: &mut Display, amount: u64, allotment: u64, metric: &str) -> anyhow::Result<()> {
    clear(d)?;
    header(d, "SESSION ACTIVE")?;
    chip(d, 60, "PAID - access granted", WHITE, GREEN)?;
    body(d, 130, &format!("paid {amount} sats via Lightning"))?;
    body(d, 150, &format!("allotment {allotment} {metric}"))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 180), Size::new(304, 2)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 208), Size::new(304, 2)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 180), Size::new(2, 30)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(310, 180), Size::new(2, 30)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(10, 182), Size::new(300, 26)), GREEN))?;
    body(d, 240, "invoice created by this terminal")?;
    body(d, 256, "QR rendered on-device")?;
    body(d, 272, "granted without any rig bridge")?;
    Ok(())
}

// ---- HTTP (the wallet's proven EspHttpConnection pattern, minimal) ----

fn http_request(
    method: embedded_svc::http::Method,
    base: &str,
    path: &str,
    json: Option<&serde_json::Value>,
) -> anyhow::Result<(u16, Vec<u8>)> {
    let config = esp_idf_svc::http::client::Configuration {
        buffer_size: Some(4096),
        buffer_size_tx: Some(2048),
        timeout: Some(core::time::Duration::from_secs(10)),
        ..Default::default()
    };
    let url = format!("{base}{path}");
    let mut connection = EspHttpConnection::new(&config)?;
    let mut client = HttpClient::wrap(connection);
    let headers: &[(&str, &str)] = if json.is_some() {
        &[("Content-Type", "application/json")]
    } else {
        &[]
    };
    let mut request = client.request(method, &url, headers)?;
    if let Some(body) = json {
        use embedded_svc::io::Write as _;
        request.write_all(&serde_json::to_vec(body)?)?;
    }
    let response = request.submit()?;
    let status = response.status();
    let mut body = Vec::new();
    {
        use embedded_svc::io::Read as _;
        let mut reader = response;
        let mut chunk = [0u8; 1024];
        loop {
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
            if body.len() > 64 * 1024 {
                anyhow::bail!("response exceeds 64 KiB");
            }
        }
    }
    Ok((status, body))
}

fn http_get(base: &str, path: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    http_request(embedded_svc::http::Method::Get, base, path, None)
}

fn http_post(base: &str, path: &str, json: &serde_json::Value) -> anyhow::Result<(u16, Vec<u8>)> {
    http_request(embedded_svc::http::Method::Post, base, path, Some(json))
}
