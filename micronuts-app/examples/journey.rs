//! Device-screen journey fixtures — the Trezor UI-regression model
//! driven through the REAL command path: each step is a `handle_command`
//! call against a deterministic in-process device (null scanner,
//! xorshift RNG, framebuffer display), and the screen the handler
//! renders is snapshotted and SHA-256-hashed against
//! `screens.fixtures.tsv`.
//!
//! Unlike the wallet's fixtures (host system fonts — CI-runner canonical
//! environment), every pixel here is deterministic: fonts are compiled
//! in, the blinder comes from a fixed xorshift seed, and the demo-mint
//! DLEQ nonces are fixed — hashes are stable across machines.
//!
//! Default mode compares and exits non-zero on drift (also asserting
//! each command's response status — a functional smoke test);
//! `SHOTS_RECORD=1` re-records after an intentional UI change. PNGs
//! land in `target/shots/journey-*.png` (gitignored review artifacts).

extern crate alloc;

use std::path::{Path, PathBuf};

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb565, Rgb888},
    prelude::*,
    Pixel,
};
use sha2::{Digest, Sha256};

use micronuts_app::command_handler::handle_command;
use micronuts_app::display::{self, HEIGHT, WIDTH};
use micronuts_app::hardware::{ScanError, Scanner, TouchPoint};
use micronuts_app::protocol::{Command, Frame, Response, Status};
use micronuts_app::state::FirmwareState;

const FIXTURES_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/screens.fixtures.tsv");

// ---------------------------------------------------------------------------
// Deterministic device: framebuffer display + xorshift RNG + null scanner
// ---------------------------------------------------------------------------

struct Framebuffer {
    px: Vec<u16>,
}

impl Framebuffer {
    fn new() -> Self {
        Self {
            px: vec![0u16; (WIDTH * HEIGHT) as usize],
        }
    }

    fn rgb_bytes(&self) -> Vec<u8> {
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
        rgb
    }

    fn write_png(&self, path: &Path, rgb: &[u8]) {
        let file = std::fs::File::create(path).expect("create png");
        let w = &mut std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(w, WIDTH, HEIGHT);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgb).expect("png pixels");
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

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

struct JourneyHardware {
    display: Framebuffer,
    rng: XorShift64,
}

impl JourneyHardware {
    fn new() -> Self {
        Self {
            display: Framebuffer::new(),
            rng: XorShift64(0x9E37_79B9_7F4A_7C15),
        }
    }
}

impl Scanner for JourneyHardware {
    async fn trigger(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
    async fn read_scan(&mut self) -> Option<Vec<u8>> {
        None
    }
    async fn stop(&mut self) {}
    fn is_connected(&self) -> bool {
        false
    }
    async fn set_aim(&mut self, _enabled: bool) -> Result<(), ScanError> {
        Ok(())
    }
    fn debug_dump_settings(&mut self) {}
    async fn deep_sleep_reboot(&mut self) -> bool {
        true
    }
    async fn reinit_scanner(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
    async fn factory_heal(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
}

impl micronuts_app::hardware::MicronutsHardware for JourneyHardware {
    type Display = Framebuffer;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.display
    }
    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        let mut offset = 0;
        while offset < dest.len() {
            let chunk = self.rng.next().to_le_bytes();
            let take = (dest.len() - offset).min(chunk.len());
            dest[offset..offset + take].copy_from_slice(&chunk[..take]);
            offset += take;
        }
    }
    async fn transport_recv_frame(&mut self) -> Option<Frame> {
        None
    }
    async fn transport_send(&mut self, _response: &Response) {}
    fn touch_get(&mut self) -> Option<TouchPoint> {
        None
    }
    async fn delay_ms(&mut self, _ms: u32) {}
}

// ---------------------------------------------------------------------------
// Deterministic demo-mint signer (the pinned device key, fixed DLEQ
// nonces — mirrors gate_verify's selftest prover)
// ---------------------------------------------------------------------------

fn pinned_secret() -> cashu_core_lite::SecretKey {
    cashu_core_lite::SecretKey::from_slice(&Sha256::digest(b"demo://micronuts")).unwrap()
}

fn sk(seed: u8) -> cashu_core_lite::SecretKey {
    cashu_core_lite::SecretKey::from_slice(&[seed; 32]).unwrap()
}

/// [C' 33B || e 32B || s 32B] per blinded output — DLEQ-provable against
/// the pinned key with a FIXED nonce so the export QR is byte-stable.
fn sign_blinded(blinded: &[u8]) -> Vec<u8> {
    use cashu_core_lite::nuts::nut12;
    let mint = pinned_secret();
    let mut out = Vec::with_capacity(blinded.len() / 33 * 97);
    for (i, chunk) in blinded.chunks(33).enumerate() {
        let b = cashu_core_lite::PublicKey::from_sec1_bytes(chunk).expect("blinded point");
        let c = cashu_core_lite::sign_message(&mint, &b);
        let dleq = nut12::prove_dleq(&b, &mint, Some(sk(0xA0 + i as u8))).expect("dleq");
        out.extend_from_slice(c.to_encoded_point(true).as_bytes());
        out.extend_from_slice(&dleq.e.to_secret_bytes());
        out.extend_from_slice(&dleq.s.to_secret_bytes());
    }
    out
}

fn demo_token_wire() -> Vec<u8> {
    let amounts = [16u64, 4, 1];
    cashu_core_lite::encode_token_wire(&cashu_core_lite::TokenV4 {
        mint: alloc::string::String::from("demo://micronuts"),
        unit: alloc::string::String::from("sat"),
        memo: Some(alloc::string::String::from("Journey fixture")),
        tokens: vec![cashu_core_lite::TokenV4Token {
            keyset_id: alloc::string::String::from("00"),
            proofs: amounts
                .iter()
                .map(|&a| cashu_core_lite::Proof {
                    amount: a,
                    keyset_id: alloc::string::String::from("00"),
                    secret: alloc::format!("journey-secret-{}", a),
                    c: vec![0x02; 33],
                    dleq: None,
                })
                .collect(),
        }],
    })
    .expect("encode wire")
    .into_bytes()
}

// ---------------------------------------------------------------------------
// Journey steps: clear panel, run the command, assert, snapshot
// ---------------------------------------------------------------------------

struct Ctx {
    hw: JourneyHardware,
    state: FirmwareState,
    last_scan: Option<Vec<u8>>,
    recorded: Vec<(String, String)>,
    out_dir: PathBuf,
}

impl Ctx {
    fn step(&mut self, name: &str, command: Command, payload: &[u8], expect: Status) {
        // Cleared panel per step: the fixture pins exactly what the
        // handler renders, independent of the previous screen.
        self.hw.display.px.fill(0);
        let response = embassy_futures::block_on(handle_command(
            command,
            payload,
            &mut self.state,
            &mut self.hw,
            &mut self.last_scan,
        ));
        assert_eq!(
            response.status,
            expect,
            "journey step {name}: device responded {actual:?}",
            actual = response.status
        );
        self.snapshot(name);
    }

    fn command_only(&mut self, name: &str, command: Command, payload: &[u8], expect: Status) {
        // Wire-only command: assert the response, pin no screen.
        self.hw.display.px.fill(0);
        let response = embassy_futures::block_on(handle_command(
            command,
            payload,
            &mut self.state,
            &mut self.hw,
            &mut self.last_scan,
        ));
        assert_eq!(
            response.status,
            expect,
            "journey step {name}: device responded {actual:?}",
            actual = response.status
        );
    }

    fn snapshot(&mut self, name: &str) {
        let rgb = self.hw.display.rgb_bytes();
        let path = self.out_dir.join(format!("journey-{name}.png"));
        self.hw.display.write_png(&path, &rgb);
        println!("wrote {}", path.display());
        let hash = hex::encode(Sha256::digest(&rgb));
        self.recorded.push((name.to_string(), hash));
    }
}

fn load_fixtures() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(FIXTURES_PATH).unwrap_or_default();
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let hash = parts.next()?.to_string();
            Some((name, hash))
        })
        .collect()
}

fn write_fixtures(fixtures: &[(String, String)]) {
    let mut text = String::from(
        "# Device journey screen-hash fixtures — regenerate with\n\
         # SHOTS_RECORD=1 after an intentional UI change; review\n\
         # target/shots/journey-*.png diffs first.\n",
    );
    for (name, hash) in fixtures {
        text.push_str(&format!("{name} {hash}\n"));
    }
    std::fs::write(FIXTURES_PATH, text).expect("write fixtures");
}

fn compare_fixtures(recorded: &[(String, String)]) -> bool {
    let expected = load_fixtures();
    if expected.is_empty() {
        eprintln!(
            "journey fixtures: {FIXTURES_PATH} is missing — run with SHOTS_RECORD=1 to create it"
        );
        return false;
    }
    let mut failures = 0;
    for (name, hash) in recorded {
        match expected.iter().find(|(n, _)| n == name) {
            Some((_, expected_hash)) if expected_hash == hash => {
                println!("fixture ok: {name}");
            }
            Some((_, expected_hash)) => {
                failures += 1;
                eprintln!(
                    "fixture MISMATCH: {name}\n  expected {expected_hash}\n  actual   {hash}\n  (PNG written to target/shots for visual diff)"
                );
            }
            None => {
                failures += 1;
                eprintln!("fixture MISSING for journey step: {name}");
            }
        }
    }
    let recorded_names: Vec<&str> = recorded.iter().map(|(n, _)| n.as_str()).collect();
    for (name, _) in &expected {
        if !recorded_names.contains(&name.as_str()) {
            failures += 1;
            eprintln!("fixture STALE: {name} no longer rendered — re-record");
        }
    }
    if failures == 0 {
        println!(
            "journey fixtures OK ({} screens, {} hashes)",
            recorded.len(),
            expected.len()
        );
        true
    } else {
        eprintln!(
            "{failures} journey fixture failure(s) — re-record with SHOTS_RECORD=1 if intentional"
        );
        false
    }
}

fn main() {
    let out_dir = PathBuf::from("target/shots");
    std::fs::create_dir_all(&out_dir).expect("shots dir");
    let mut ctx = Ctx {
        hw: JourneyHardware::new(),
        state: FirmwareState::new(),
        last_scan: None,
        recorded: Vec::new(),
        out_dir,
    };

    // The full swap arc, through the real handlers.
    ctx.step(
        "import-token",
        Command::ImportToken,
        &demo_token_wire(),
        Status::Ok,
    );
    ctx.step("blinded", Command::GetBlinded, &[], Status::Ok);

    let blinded = ctx.state.blinded_messages.as_ref().unwrap();
    let signatures = sign_blinded(
        &blinded
            .iter()
            .flat_map(|bm| bm.blinded.to_encoded_point(true).as_bytes().to_vec())
            .collect::<Vec<u8>>(),
    );
    ctx.step("signed", Command::SendSignatures, &signatures, Status::Ok);

    // GetProofs renders nothing (the panel keeps the last screen on
    // hardware); the export-QR step right after covers the visual.
    ctx.command_only("export-proofs", Command::GetProofs, &[], Status::Ok);

    // The device's proof-QR screen renders the SAME export token (#29).
    ctx.hw.display.px.fill(0);
    display::render_export_qr(&mut ctx.hw.display, &ctx.state);
    ctx.snapshot("export-qr");

    // Failure + scanner surfaces.
    ctx.step(
        "invalid-token",
        Command::ImportToken,
        b"not-a-token",
        Status::InvalidPayload,
    );
    ctx.step("scan-trigger", Command::ScannerTrigger, &[], Status::Ok);

    ctx.last_scan = Some(demo_token_wire());
    ctx.step("scan-cashu", Command::ScannerData, &[], Status::Ok);

    ctx.last_scan = Some(b"hello@nut.cash".to_vec());
    ctx.step("scan-text", Command::ScannerData, &[], Status::Ok);

    if std::env::var_os("SHOTS_RECORD").is_some() {
        write_fixtures(&ctx.recorded);
        println!(
            "recorded {} journey fixtures to {FIXTURES_PATH}",
            ctx.recorded.len()
        );
        return;
    }
    if !compare_fixtures(&ctx.recorded) {
        std::process::exit(1);
    }
}
