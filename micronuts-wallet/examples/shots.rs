//! Headless screenshots of every wallet screen — custom software-renderer
//! platform, no X server, no event loop. Writes `target/shots/wallet-*.png`.
//!
//! Screen-hash fixtures (Trezor's `--ui=record`/`--ui=test` model):
//! every rendered screen is SHA-256-hashed and compared against
//! `screens.fixtures.tsv` (committed; PNGs are gitignored review
//! artifacts). Default mode compares and exits non-zero on drift;
//! `SHOTS_RECORD=1` re-records after an intentional UI change.
//!
//! Font caveat: rendering uses the host's system fonts (DejaVu on this
//! box and on CI's ubuntu runners). A font-package bump can shift every
//! hash — review the PNG diff and re-record. The deterministic upgrade
//! path is enabling slint's `unstable-fontique-07` natively and
//! registering the bundled DejaVu (as the wasm build already does).

use std::cell::RefCell;
use std::rc::Rc;

use sha2::{Digest, Sha256};
use slint::platform::software_renderer::{
    PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, VecModel};

slint::include_modules!();

const WIDTH: u32 = 480;
const HEIGHT: u32 = 800;

const FIXTURES_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/screens.fixtures.tsv");

thread_local! {
    static RENDERER: RefCell<Option<Rc<SoftwareRenderer>>> =
        const { RefCell::new(None) };
}

struct HeadlessAdapter {
    window: slint::Window,
    renderer: Rc<SoftwareRenderer>,
}

impl WindowAdapter for HeadlessAdapter {
    fn window(&self) -> &slint::Window {
        &self.window
    }
    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &*self.renderer
    }
    fn size(&self) -> slint::PhysicalSize {
        slint::PhysicalSize::new(WIDTH, HEIGHT)
    }
}

struct HeadlessPlatform;

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let renderer = Rc::new(SoftwareRenderer::new());
        let adapter = Rc::new_cyclic(|weak| HeadlessAdapter {
            window: slint::Window::new(weak.clone() as _),
            renderer: renderer.clone(),
        });
        RENDERER.with(|slot| *slot.borrow_mut() = Some(renderer));
        Ok(adapter)
    }
}

fn write_png(path: &std::path::Path, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}

fn shot(ui: &MainWindow, page: Page, name: &str, out_dir: &std::path::Path) -> String {
    ui.set_current_page(page);
    let renderer = RENDERER.with(|slot| slot.borrow().clone().expect("renderer"));
    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
    let mut buffer = vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize];
    renderer.render(&mut buffer, WIDTH as usize);
    let mut rgba = Vec::with_capacity(buffer.len() * 4);
    for pixel in &buffer {
        let alpha = u16::from(pixel.alpha);
        if alpha == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else if alpha == 255 {
            rgba.extend_from_slice(&[pixel.red, pixel.green, pixel.blue, 255]);
        } else {
            let half = alpha / 2;
            rgba.push(((u16::from(pixel.red) * 255 + half) / alpha).min(255) as u8);
            rgba.push(((u16::from(pixel.green) * 255 + half) / alpha).min(255) as u8);
            rgba.push(((u16::from(pixel.blue) * 255 + half) / alpha).min(255) as u8);
            rgba.push(pixel.alpha);
        }
    }
    let path = out_dir.join(format!("wallet-{name}.png"));
    write_png(&path, &rgba);
    println!("wrote {}", path.display());
    hex::encode(Sha256::digest(&rgba))
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
        "# Screen-hash fixtures — regenerate with SHOTS_RECORD=1 after an\n\
         # intentional UI change; review target/shots/*.png diffs first.\n",
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
            "screen fixtures: {FIXTURES_PATH} is missing — run with SHOTS_RECORD=1 to create it"
        );
        return false;
    }
    let lookup = |name: &str| {
        expected
            .iter()
            .find(|(expected_name, _)| expected_name == name)
            .map(|(_, hash)| hash.clone())
    };
    let mut failures = 0;
    for (name, hash) in recorded {
        match lookup(name) {
            Some(expected_hash) if expected_hash == *hash => {
                println!("fixture ok: {name}");
            }
            Some(expected_hash) => {
                failures += 1;
                eprintln!(
                    "fixture MISMATCH: {name}\n  expected {expected_hash}\n  actual   {hash}\n  (PNG written to target/shots for visual diff)"
                );
            }
            None => {
                failures += 1;
                eprintln!("fixture MISSING for screen: {name}");
            }
        }
    }
    let recorded_names: Vec<&str> = recorded.iter().map(|(name, _)| name.as_str()).collect();
    for (expected_name, _) in &expected {
        if !recorded_names.contains(&expected_name.as_str()) {
            failures += 1;
            eprintln!(
                "fixture STALE: {expected_name} is in fixtures but no longer rendered — re-record"
            );
        }
    }
    if failures == 0 {
        println!(
            "screen fixtures OK ({} screens, {} hashes)",
            recorded.len(),
            expected.len()
        );
        true
    } else {
        eprintln!("{failures} fixture failure(s) — re-record with SHOTS_RECORD=1 if intentional");
        false
    }
}

fn main() {
    slint::platform::set_platform(Box::new(HeadlessPlatform)).expect("platform");

    let ui = MainWindow::new().expect("window");
    let logic = ui.global::<WalletLogic>();
    logic.set_connected(true);
    logic.set_has_active_mint(true);
    logic.set_mint_name("Micronuts Demo Mint".into());
    logic.set_mint_url("http://127.0.0.1:3030".into());
    logic.set_balance_text("128 sat".into());
    logic.set_status_message("connected".into());
    logic.set_seed("9f2c1a44bd7e5a30c8f61d0b47a9e35c2b18d6f4039a7c5e81b2d4f6a8c0e375".into());
    logic.set_history(ModelRc::new(VecModel::from(vec![
        HistoryItem {
            line1: "↓ 100 sat · mint".into(),
            line2: "quote cmQx".into(),
        },
        HistoryItem {
            line1: "↑ 21 sat · send".into(),
            line2: "ecash token".into(),
        },
        HistoryItem {
            line1: "↑ 30 sat · melt".into(),
            line2: "preimage 00f4c1".into(),
        },
    ])));
    logic.set_mints(ModelRc::new(VecModel::from(vec![
        MintItem {
            url: "http://127.0.0.1:3030".into(),
            name: "Micronuts Demo Mint".into(),
            active: true,
        },
        MintItem {
            url: "https://other-mint.example".into(),
            name: "Community Mint".into(),
            active: false,
        },
    ])));

    ui.show().expect("show");
    let out_dir = std::path::Path::new("target/shots");
    std::fs::create_dir_all(out_dir).expect("shots dir");
    let mut recorded: Vec<(String, String)> = Vec::new();
    let mut shoot = |page: Page, name: &str| {
        let hash = shot(&ui, page, name, out_dir);
        recorded.push((name.to_string(), hash));
    };

    shoot(Page::Home, "home");
    shoot(Page::Activity, "history");
    shoot(Page::Settings, "settings");
    shoot(Page::Mints, "mints");
    shoot(Page::Backup, "backup");

    logic.set_token_in("cashuBpGFtcGRvaHR0cHM6Ly9taW50LmV4YW1wbGU".into());
    logic.set_receive_ecash_state("review".into());
    logic.set_receive_review_line("21 sats from https://mint.example".into());
    logic.set_receive_review_fee("No fee".into());
    shoot(Page::Receive, "receive-token");

    logic.set_receive_tab(1);
    logic.set_receive_ecash_state("input".into());
    logic.set_receive_review_line(String::new().into());
    logic.set_receive_review_fee(String::new().into());
    logic.set_invoice("lnbc1280u1psl9wmepp5yqzp3zx3q…".into());
    logic.set_invoice_quote_id("cmQx".into());
    logic.set_invoice_state("PAID".into());
    logic.set_invoice_status_text(
        micronuts_wallet::flow::ReceiveLightningPhase::from_quote_state("PAID", 128)
            .user_line()
            .into(),
    );
    logic.set_invoice_amount(128);
    logic.set_invoice_amount_text("128 sats".into());
    if let Some(image) = micronuts_wallet::ui::qr_image("lnbc1280u1demo") {
        logic.set_invoice_qr(image);
    }
    shoot(Page::Receive, "receive-lightning");

    logic.set_receive_tab(0);
    logic.set_invoice("".into());
    logic.set_invoice_qr(slint::Image::default());
    logic.set_invoice_state("".into());

    logic.set_token_out(
        "cashuBeyJtIjpbIjAwYWExYjJjM2Q0ZTZmNyJdLCJ1Ijoic2F0IiwibSI6Imh0dHA6Ly8xMjcuMC4wLjE6MzAzMCJ9"
            .into(),
    );
    if let Some(image) = micronuts_wallet::ui::qr_image(
        "cashuBeyJtIjpbIjAwYWExYjJjM2Q0ZTZmNyJdLCJ1Ijoic2F0IiwibSI6Imh0dHA6Ly8xMjcuMC4wLjE6MzAzMCJ9",
    ) {
        logic.set_token_qr(image);
    }
    shoot(Page::Send, "send-token");

    logic.set_send_tab(1);
    logic.set_token_out("".into());
    logic.set_token_qr(slint::Image::default());
    logic.set_melt_quote_info("pay 30 sat (fee reserve 0 sat) — press Pay to confirm".into());
    shoot(Page::Send, "send-lightning");

    if std::env::var_os("SHOTS_RECORD").is_some() {
        write_fixtures(&recorded);
        println!(
            "recorded {} screen fixtures to {FIXTURES_PATH}",
            recorded.len()
        );
        return;
    }
    if !compare_fixtures(&recorded) {
        std::process::exit(1);
    }
}
