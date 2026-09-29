//! micronuts × TollGate terminal demo — CYD (ESP32-3248S035, ST7796 320×480).
//!
//! Display-only kiosk demo (micronuts#79): renders the terminal UI with a
//! REAL BOLT11 invoice QR (the 21-sat fakewallet invoice from the tmbr
//! PR #25 E2E run — genuine format, no money attached), the paid/session
//! state, and a wallet status screen. RGB LED tracks the state. No WiFi:
//! the TollGate AP comes later; `tollgate join/pay` lives in the main
//! wallet bin. Serial console logs screen transitions for rig verification.
//!
//! Pinout: labgrid-verified recipe (embassy-hello-esp32 ground truth,
//! re-verified 2026-09-28) — display on HSPI SCK=14/MOSI=13, CS=15, DC=2,
//! BL=27; RGB LED R=4 G=16 B=17 active-low; this unit has no touch.

use embedded_graphics::{
    mono_font::{
        MonoTextStyleBuilder,
        ascii::{FONT_10X20, FONT_6X10},
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Rectangle, PrimitiveStyleBuilder},
    text::{Baseline, Text, TextStyleBuilder},
};
use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{Gpio12, PinDriver},
    peripherals::Peripherals,
    spi::{
        Dma, SpiDeviceDriver, SpiDriver,
        config::{Config, DriverConfig},
    },
    units::FromValueType,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7796,
    options::{ColorInversion, ColorOrder, Orientation},
};

const W: u32 = 320;
const H: u32 = 480;

// QR matrix generated host-side (python qrcode, ERROR_CORRECT_L, border=0)
// from the 21-sat invoice below — regenerate if the invoice changes.
// Version 10-L, 57x57 modules, 5px/module = 285px.
const QR_SIZE: u32 = 57;
const QR_SCALE: u32 = 5;
const QR_ROWS: [u64; 57] = [    0x1FC54AFB5A3E67F,
    0x1056200F85E1241,
    0x1742CE65D65FE5D,
    0x175BAE986FEE25D,
    0x17485067F98E25D,
    0x105C7E0447E0C41,
    0x1FD55555555557F,
    0x000918AC7273B00,
    0x1F769F2FE1C46AA,
    0x0E0E60DA6AB5999,
    0x1BEB3BB885D86E6,
    0x1515A031787739F,
    0x14CC1B8ECB0EAA2,
    0x1D898437D18E383,
    0x107E35372DC8DE2,
    0x128AEC01FA5F33F,
    0x15CF9D8CCDA88C8,
    0x1489EA78E895397,
    0x1E63CF3825685CE,
    0x120548937ABF31D,
    0x1AD0C43C8B86E2B,
    0x059D2B5773AC181,
    0x0A6C493B9752E5A,
    0x1001A3917CB7915,
    0x064C9F22238A4C9,
    0x0EB8A8D57125BBB,
    0x15FA2757E5E87F2,
    0x1B110DE47697D1F,
    0x095657656D06D50,
    0x13174C9C420C311,
    0x19F0C70FE7C9FF6,
    0x1DB02454BE9FFD6,
    0x19E51D2701E4D18,
    0x1A26CAF4533F9A1,
    0x0C790CCA5FE8432,
    0x089B94DE9ABD7EC,
    0x0344AE91ABCED99,
    0x119B9862D02E1E7,
    0x0C7E72CB55C2603,
    0x0A30D46E929F3CC,
    0x0A40BD0D6308DF8,
    0x14848AF8301D047,
    0x18D3EBB65FCB412,
    0x042EC2309A373EC,
    0x0BEC1BA7E504431,
    0x07904414A31700F,
    0x14C64DECD5C842E,
    0x1F073453B2932F7,
    0x00727FA7E5201FA,
    0x0017485C4885B11,
    0x1FD3E42564C855E,
    0x10420E145EB791E,
    0x175AC4B7E38C5FB,
    0x175E6B5D688D854,
    0x175C85385540720,
    0x1055EB1C5CB5E9C,
    0x1FD2BF20A74A1EA,
];

static mut SPI_BUFFER: [u8; 4096] = [0u8; 4096];

const BLACK: Rgb565 = Rgb565::BLACK;
const WHITE: Rgb565 = Rgb565::WHITE;
const GREEN: Rgb565 = Rgb565::CSS_DARK_GREEN;
const AMBER: Rgb565 = Rgb565::new(60, 44, 2);
const BLUE: Rgb565 = Rgb565::CSS_DARK_BLUE;
const RED: Rgb565 = Rgb565::RED;

type OutPin = PinDriver<'static, esp_idf_svc::hal::gpio::Output>;
type Display = mipidsi::Display<
    SpiInterface<'static, SpiDeviceDriver<'static, SpiDriver<'static>>, OutPin>,
    ST7796,
    mipidsi::NoResetPin,
>;

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();
    std::thread::Builder::new()
        .stack_size(32 * 1024)
        .spawn(run)?
        .join()
        .map_err(|_| anyhow::anyhow!("demo thread panicked"))?
}

struct Leds {
    r: OutPin,
    g: OutPin,
    b: OutPin,
}

impl Leds {
    /// CYD RGB LED is active-low.
    fn set(&mut self, r: bool, g: bool, b: bool) {
        let _ = if r { self.r.set_low() } else { self.r.set_high() };
        let _ = if g { self.g.set_low() } else { self.g.set_high() };
        let _ = if b { self.b.set_low() } else { self.b.set_high() };
    }
}

fn wrap<T, E: core::fmt::Debug>(r: Result<T, E>) -> anyhow::Result<()> {
    r.map(|_| ()).map_err(|e| anyhow::anyhow!("{e:?}"))
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

    println!("terminal demo: mipidsi init (ST7796 320x480, BGR)");
    let mut display: Display = Builder::new(ST7796, di)
        .display_size(W as u16, H as u16)
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::default().flip_horizontal())
        .init(&mut FreeRtos)
        .map_err(|e| anyhow::anyhow!("init: {e:?}"))?;

    println!("TERMINAL READY");
    use std::io::BufRead as _;
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        if stdin.read_line(&mut line).is_err() || line.trim().is_empty() {
            continue;
        }
        let t = line.trim();
        let (cmd, rest) = t.split_once(' ').unwrap_or((t, ""));
        match cmd {
            "invoice" => {
                let (amount, bolt11) = rest.split_once(' ').unwrap_or((rest, ""));
                if bolt11.is_empty() {
                    println!("ERR invoice <amount> <bolt11>");
                    continue;
                }
                match draw_invoice_live(&mut display, amount, bolt11) {
                    Ok(()) => {
                        led.set(true, false, false);
                        println!("OK invoice {amount}");
                    }
                    Err(e) => println!("ERR draw: {e}"),
                }
            }
            "paid" => match draw_paid(&mut display) {
                Ok(()) => {
                    led.set(false, true, false);
                    println!("OK paid {rest}");
                }
                Err(e) => println!("ERR draw: {e}"),
            },
            "wallet" => match draw_wallet(&mut display) {
                Ok(()) => {
                    led.set(false, false, true);
                    println!("OK wallet");
                }
                Err(e) => println!("ERR draw: {e}"),
            },
            "demo" => {
                for (f, (r, g, b)) in [
                    (&draw_invoice as &dyn Fn(&mut Display) -> anyhow::Result<()>, (true, false, false)),
                    (&draw_paid as &dyn Fn(&mut Display) -> anyhow::Result<()>, (false, true, false)),
                    (&draw_wallet as &dyn Fn(&mut Display) -> anyhow::Result<()>, (false, false, true)),
                ] {
                    if let Err(e) = f(&mut display) {
                        println!("ERR demo: {e}");
                    }
                    led.set(r, g, b);
                    std::thread::sleep(std::time::Duration::from_millis(2500));
                }
                println!("OK demo");
            }
            _ => println!("ERR unknown (invoice|paid|wallet|demo)"),
        }
    }
}
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

fn draw_qr(d: &mut Display, top: i32) -> anyhow::Result<()> {
    let x0 = ((W - QR_SIZE * QR_SCALE) / 2) as i32;
    wrap(
        d.fill_solid(
            &Rectangle::new(
                Point::new(x0 - 8, top - 8),
                Size::new(QR_SIZE * QR_SCALE + 16, QR_SIZE * QR_SCALE + 16),
            ),
            WHITE,
        ),
    )?;
    // One window, zero-heap streamed pixels: each QR row repeats QR_SCALE
    // times; each module stretches QR_SCALE pixels wide.
    struct QrPx {
        row: u32,
        rep: u32,
        col: u32,
        k: u32,
    }
    impl Iterator for QrPx {
        type Item = Rgb565;
        fn next(&mut self) -> Option<Rgb565> {
            loop {
                if self.row >= QR_SIZE {
                    return None;
                }
                let dark = (QR_ROWS[self.row as usize] >> (QR_SIZE - 1 - self.col)) & 1 == 1;
                let px = if dark { BLACK } else { WHITE };
                self.k += 1;
                if self.k >= QR_SCALE {
                    self.k = 0;
                    self.col += 1;
                    if self.col >= QR_SIZE {
                        self.col = 0;
                        self.rep += 1;
                        if self.rep >= QR_SCALE {
                            self.rep = 0;
                            self.row += 1;
                            // scale-row boundary: a beat for the scheduler
                            std::thread::yield_now();
                        }
                    }
                }
                return Some(px);
            }
        }
    }
    let area = Rectangle::new(
        Point::new(x0, top),
        Size::new(QR_SIZE * QR_SCALE, QR_SIZE * QR_SCALE),
    );
    wrap(d.fill_contiguous(&area, QrPx { row: 0, rep: 0, col: 0, k: 0 }))
}

fn draw_qr_live(d: &mut Display, top: i32, code: &qrcode::QrCode) -> anyhow::Result<()> {
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

fn draw_invoice_live(d: &mut Display, amount: &str, bolt11: &str) -> anyhow::Result<()> {
    let code = qrcode::QrCode::with_error_correction_level(bolt11.as_bytes(), qrcode::EcLevel::L)
        .map_err(|e| anyhow::anyhow!("qr: {e:?}"))?;
    clear(d)?;
    header(d, "TOLLGATE")?;
    body(d, 48, &format!("{amount} sats — scan to pay"))?;
    draw_qr_live(d, 80, &code)?;
    chip(d, 400, "AWAITING PAYMENT", BLACK, AMBER)?;
    body(d, 444, "live invoice · rig-bridged")?;
    Ok(())
}

fn draw_invoice(d: &mut Display) -> anyhow::Result<()> {
    let t = |what: &str| println!("demo t={:?} {what}", std::time::SystemTime::now());
    t("invoice: clear");
    clear(d)?;
    t("invoice: header");
    header(d, "TOLLGATE")?;
    body(d, 48, "21 sats -> 60 s access")?;
    t("invoice: qr");
    draw_qr(d, 80)?;
    t("invoice: chip");
    chip(d, 400, "AWAITING PAYMENT", BLACK, AMBER)?;
    t("invoice: done");
    body(d, 444, "swatches: R  G  B  W")?;
    for (i, c) in [RED, GREEN, BLUE, WHITE].iter().enumerate() {
        wrap(d.fill_solid(
            &Rectangle::new(Point::new(8 + (i as i32) * 40, 460), Size::new(36, 18)),
            *c,
        ))?;
    }
    Ok(())
}

fn draw_paid(d: &mut Display) -> anyhow::Result<()> {
    clear(d)?;
    header(d, "SESSION ACTIVE")?;
    chip(d, 60, "PAID - access granted", WHITE, GREEN)?;
    body(d, 130, "allotment 462,422,016 bytes")?;
    body(d, 150, "used               0 bytes")?;
    // frame: 4 solid bars (fast path) instead of a styled outline
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 180), Size::new(304, 2)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 208), Size::new(304, 2)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(8, 180), Size::new(2, 30)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(310, 180), Size::new(2, 30)), WHITE))?;
    wrap(d.fill_solid(&Rectangle::new(Point::new(10, 182), Size::new(300, 26)), GREEN))?;
    body(d, 240, "lightning quote settled:")?;
    body(d, 256, "minted + granted exactly once")?;
    body(d, 272, "(crash-idempotent grant)")?;
    Ok(())
}

fn draw_wallet(d: &mut Display) -> anyhow::Result<()> {
    clear(d)?;
    header(d, "MICRONUTS WALLET")?;
    chip(d, 60, "nucula-mode  CYD", WHITE, BLUE)?;
    body(d, 130, "wallet core: cashu-core-lite")?;
    body(d, 150, "store: NVS ProofStore (OK)")?;
    body(d, 170, "tollgate client: join/pay/status")?;
    body(d, 200, "TollGate AP: not in range")?;
    body(d, 216, "(demo mode - display only)")?;
    Ok(())
}
