//! Micronuts wallet: a host Cashu wallet with a Slint UI.
//!
//! Crate layout:
//! - `http`: REST implementation of the `MintClient` transport trait
//! - `engine`: wallet operations on top of `PersistentWallet`
//! - `state`: file-backed persistence (`ProofStore` + wallet metadata)
//! - `ui`: Slint application wiring

#[cfg(target_arch = "wasm32")]
pub mod browser_store;
#[cfg(target_arch = "wasm32")]
pub mod camera;
pub mod demo_mint;
pub use micronuts_wallet_core::engine;
pub use micronuts_wallet_core::flow;
#[cfg(not(target_arch = "wasm32"))]
pub mod gm65;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;
pub use micronuts_wallet_core::mint_wire;
pub use micronuts_wallet_core::payload;
pub mod qr_decode;
pub use micronuts_wallet_core::state;
pub mod ui;
#[cfg(target_arch = "wasm32")]
pub mod wasm_http;

/// The one amount formatter (UX contract): thousands-separated integer
/// + " sats". Every surface renders amounts through this.
pub fn format_amount(amount: u64) -> String {
    let digits = amount.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped.push_str(" sats");
    grouped
}

/// Browser entry: same UI, embedded demo mint, no worker thread.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
    register_embedded_fonts();
    log_qr_self_test();
    let dir = std::path::PathBuf::from(":browser:");
    if let Err(err) = ui::run(dir) {
        wasm_bindgen::throw_str(&format!("micronuts-wallet: {err}"));
    }
}

/// Boot-time evidence that the in-wasm QR decoder works: encode →
/// decode round-trip, logged to the browser console.
#[cfg(target_arch = "wasm32")]
fn log_qr_self_test() {
    let token = format!(
        "cashuB{}",
        "pGFtdWh0dHA6Ly8xMjcuMC4wLjE6MzAzMHVpc2F0bQ"
            .chars()
            .cycle()
            .take(90)
            .collect::<String>()
    );
    let ok = match qr_decode::encode_luminance(&token, 4, 4) {
        Some((dim, luma)) => {
            qr_decode::decode_luminance(dim, dim, &luma).as_deref() == Some(token.as_str())
        }
        None => false,
    };
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(if ok {
        "micronuts-wallet qr self-test: ok"
    } else {
        "micronuts-wallet qr self-test: FAILED"
    }));
}

/// The web sandbox exposes no system fonts to Slint's font database, so
/// the browser build registers the bundled DejaVu fonts (see assets/) as
/// the Latin-script fallback.
#[cfg(target_arch = "wasm32")]
fn register_embedded_fonts() {
    use slint::fontique_07::fontique;
    for bytes in [
        include_bytes!("../assets/DejaVuSans.ttf").as_slice(),
        include_bytes!("../assets/DejaVuSans-Bold.ttf").as_slice(),
    ] {
        let blob = fontique::Blob::new(std::sync::Arc::new(bytes.to_vec()));
        let mut collection = slint::fontique_07::shared_collection();
        let fonts = collection.register_fonts(blob, None);
        collection.append_fallbacks(
            fontique::FallbackKey::new("Latn", None),
            fonts.iter().map(|(id, _)| *id),
        );
    }
}
