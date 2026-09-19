//! UI-free wallet core, shared by every micronuts wallet surface.
//!
//! `engine` — the money operations behind any UI (generic over
//! `MintClient + ProofStore`, carries no rendering or platform I/O).
//! `flow` — receive/send/pay phase machines driving the UI contract.
//! `state` — wallet state model (mints, balances, history).
//! `payload` — clipboard/QR payload classification.
//! `mint_wire` — the Cashu REST wire protocol over any `JsonTransport`.
//!
//! This crate is the portability boundary: `micronuts-wallet` (host +
//! wasm) and `micronuts-esp32-wallet` (device) both consume it,
//! supplying only their own transports and UI.

pub mod engine;
pub mod flow;
pub mod mint_wire;
pub mod payload;
pub mod state;
pub mod token_compat;

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
