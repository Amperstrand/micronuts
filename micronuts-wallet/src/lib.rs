//! Micronuts wallet: a host Cashu wallet with a Slint UI.
//!
//! Crate layout:
//! - `http`: REST implementation of the `MintClient` transport trait
//! - `engine`: wallet operations on top of `PersistentWallet`
//! - `state`: file-backed persistence (`ProofStore` + wallet metadata)
//! - `ui`: Slint application wiring

pub mod demo_mint;
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub mod gm65;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;
pub mod qr_decode;
pub mod state;
pub mod ui;

/// Browser entry: same UI, embedded demo mint, no worker thread.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
    register_embedded_fonts();
    let dir = std::path::PathBuf::from(":browser:");
    if let Err(err) = ui::run(dir) {
        wasm_bindgen::throw_str(&format!("micronuts-wallet: {err}"));
    }
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
