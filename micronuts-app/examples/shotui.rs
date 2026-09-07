//! Headless screenshot generator — renders every app screen into PNG
//! files with no SDL2/window system required.
//!
//! Usage:
//! ```text
//! cargo run -p micronuts-app --example shotui --features std [-- <out-dir>]
//! ```
//!
//! Default output directory: `target/shots`. Used by the graphics
//! review loop (z.ai vision critique) and README previews.

extern crate alloc;

use std::path::PathBuf;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb565, Rgb888},
    prelude::*,
    Pixel,
};

use micronuts_app::display::{self, HEIGHT, WIDTH};
use micronuts_app::qr::QrPayload;
use micronuts_app::state::FirmwareState;

/// In-memory RGB565 framebuffer (the panel format), blitted to PNG as RGB8.
struct Framebuffer {
    px: Vec<u16>,
}

impl Framebuffer {
    fn new() -> Self {
        Self {
            px: vec![0u16; (WIDTH * HEIGHT) as usize],
        }
    }

    /// Encode the RGB565 pixels as a PNG (RGB, 8 bits/channel).
    fn write_png(&self, path: &std::path::Path) {
        let mut rgb = vec![0u8; self.px.len() * 3];
        for (i, &p) in self.px.iter().enumerate() {
            let (r, g, b) = (
                ((p >> 11) & 0x1F) as u8,
                ((p >> 5) & 0x3F) as u8,
                (p & 0x1F) as u8,
            );
            rgb[i * 3] = (r << 3) | (r >> 2);
            rgb[i * 3 + 1] = (g << 2) | (g >> 4);
            rgb[i * 3 + 2] = (b << 3) | (b >> 2);
        }
        let file = std::fs::File::create(path).expect("create png");
        let w = &mut std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(w, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&rgb).expect("png pixels");
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Framebuffer {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x >= 0 && coord.y >= 0 {
                let (x, y) = (coord.x as u32, coord.y as u32);
                if x < WIDTH && y < HEIGHT {
                    self.px[(y * WIDTH + x) as usize] = Rgb565::from(color).into_storage();
                }
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.px.fill(Rgb565::from(color).into_storage());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sample data (std-only, mirrors the display unit-test fixtures)
// ---------------------------------------------------------------------------

fn sample_token() -> cashu_core_lite::TokenV4 {
    let amounts = [1u64, 2, 4, 8, 16, 32, 64, 128];
    cashu_core_lite::TokenV4 {
        mint: alloc::string::String::from("https://mint.example.com"),
        unit: alloc::string::String::from("sat"),
        memo: Some(alloc::string::String::from("Coffee run")),
        tokens: vec![cashu_core_lite::TokenV4Token {
            keyset_id: alloc::string::String::from("009a1f"),
            proofs: amounts
                .iter()
                .map(|&a| cashu_core_lite::Proof {
                    amount: a,
                    keyset_id: alloc::string::String::from("009a1f"),
                    secret: alloc::format!("secret-{}", a),
                    c: vec![0x02; 33],
                    dleq: Some(dleq()),
                })
                .collect(),
        }],
    }
}

fn dleq() -> cashu_core_lite::nuts::nut12::ProofDleq {
    cashu_core_lite::nuts::nut12::ProofDleq::new(
        cashu_core_lite::SecretKey::from_slice(&[0x11; 32]).unwrap(),
        cashu_core_lite::SecretKey::from_slice(&[0x22; 32]).unwrap(),
        cashu_core_lite::SecretKey::from_slice(&[0x33; 32]).unwrap(),
    )
}

fn proofs_ready_state() -> FirmwareState {
    let mut state = FirmwareState::new();
    state.imported_token = Some(cashu_core_lite::TokenV4 {
        mint: alloc::string::String::from("https://mint.example.com"),
        unit: alloc::string::String::from("sat"),
        memo: Some(alloc::string::String::from("Coffee run")),
        tokens: vec![cashu_core_lite::TokenV4Token {
            keyset_id: alloc::string::String::from("009a1f"),
            proofs: vec![],
        }],
    });
    state.new_proofs = Some(
        [16u64, 4, 1]
            .iter()
            .map(|&a| cashu_core_lite::Proof {
                amount: a,
                keyset_id: alloc::string::String::from("009a1f"),
                secret: alloc::format!("secret-{}", a),
                c: vec![0x02; 33],
                dleq: Some(dleq()),
            })
            .collect(),
    );
    state
}

fn cashu_payload() -> QrPayload {
    let token = sample_token();
    let wire = cashu_core_lite::encode_token_wire(&token).expect("encode wire");
    QrPayload::CashuV4 {
        encoded: wire.into_bytes(),
    }
}

// ---------------------------------------------------------------------------
// Screen registry
// ---------------------------------------------------------------------------

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/shots"));
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    type Shot = (&'static str, Box<dyn Fn(&mut Framebuffer)>);
    let shots: Vec<Shot> = vec![
        (
            "01_home",
            Box::new(|fb| display::render_home(fb, true, false)),
        ),
        (
            "02_home_last_scan",
            Box::new(|fb| display::render_home(fb, true, true)),
        ),
        (
            "03_scanning",
            Box::new(|fb| {
                display::draw_scanning(fb, true);
                display::draw_scanning_progress(fb, 2, 10);
            }),
        ),
        (
            "04_scanning_retry",
            Box::new(|fb| {
                display::draw_scanning(fb, false);
                display::draw_scanning_retry(fb);
            }),
        ),
        (
            "05_scan_result_cashu",
            Box::new(|fb| display::render_decoded_scan(fb, &cashu_payload())),
        ),
        (
            "06_scan_result_text",
            Box::new(|fb| {
                display::render_decoded_scan(fb, &QrPayload::PlainText(b"hello@nut.cash".to_vec()))
            }),
        ),
        ("07_waiting_token", Box::new(display::render_waiting_token)),
        (
            "08_token_info",
            Box::new(|fb| display::render_token_info(fb, &sample_token())),
        ),
        (
            "09_export_qr",
            Box::new(|fb| {
                let state = proofs_ready_state();
                display::render_export_qr(fb, &state);
            }),
        ),
        (
            "10_error",
            Box::new(|fb| display::render_error(fb, "Token decode failed: truncated CBOR")),
        ),
        (
            "11_status",
            Box::new(|fb| display::render_status(fb, "Proofs ready")),
        ),
    ];

    for (name, render) in &shots {
        let mut fb = Framebuffer::new();
        render(&mut fb);
        let path = out_dir.join(format!("{}.png", name));
        fb.write_png(&path);
        println!("wrote {}", path.display());
    }
    println!("done: {} screenshots -> {}", shots.len(), out_dir.display());
}
