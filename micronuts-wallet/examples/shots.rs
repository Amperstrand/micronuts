//! Headless screenshots of every wallet screen — custom software-renderer
//! platform, no X server, no event loop. Writes `target/shots/wallet-*.png`.

use std::cell::RefCell;
use std::rc::Rc;

use slint::platform::software_renderer::{
    PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, VecModel};

slint::include_modules!();

const WIDTH: u32 = 480;
const HEIGHT: u32 = 800;

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

fn write_png(path: &std::path::Path, pixels: &[PremultipliedRgbaColor]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
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
    writer.write_image_data(&rgba).expect("png data");
}

fn shot(ui: &MainWindow, page: Page, name: &str, out_dir: &std::path::Path) {
    ui.set_current_page(page);
    let renderer = RENDERER.with(|slot| slot.borrow().clone().expect("renderer"));
    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
    let mut buffer = vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize];
    renderer.render(&mut buffer, WIDTH as usize);
    let path = out_dir.join(format!("wallet-{name}.png"));
    write_png(&path, &buffer);
    println!("wrote {}", path.display());
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

    shot(&ui, Page::Home, "home", out_dir);
    shot(&ui, Page::History, "history", out_dir);
    shot(&ui, Page::Mints, "mints", out_dir);
    shot(&ui, Page::Backup, "backup", out_dir);

    logic.set_token_check_text("healthy: 3 unspent proofs".into());
    shot(&ui, Page::Receive, "receive-token", out_dir);

    logic.set_receive_tab(1);
    logic.set_token_check_text("".into());
    logic.set_invoice("lnbc1280u1psl9wmepp5yqzp3zx3q…".into());
    logic.set_invoice_quote_id("cmQx".into());
    logic.set_invoice_state("PAID".into());
    logic.set_invoice_amount(128);
    logic.set_invoice_amount_text("128 sat".into());
    if let Some(image) = micronuts_wallet::ui::qr_image("lnbc1280u1demo") {
        logic.set_invoice_qr(image);
    }
    shot(&ui, Page::Receive, "receive-lightning", out_dir);

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
    shot(&ui, Page::Send, "send-token", out_dir);

    logic.set_send_tab(1);
    logic.set_token_out("".into());
    logic.set_token_qr(slint::Image::default());
    logic.set_melt_quote_info("pay 30 sat (fee reserve 0 sat) — press Pay to confirm".into());
    shot(&ui, Page::Send, "send-lightning", out_dir);
}
