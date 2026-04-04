//! NUT-05: Melt Tokens (Bolt11)
//!
//! Melt (redeem) ecash by paying a Lightning invoice. The wallet provides
//! proofs and the mint pays the invoice, returning a payment preimage.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/05.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::nut00::{BlindSignature, BlindedMessage, Proof};

/// Quote states as defined in NUT-05.
pub mod state {
    pub const UNPAID: &str = "UNPAID";
    pub const PENDING: &str = "PENDING";
    pub const PAID: &str = "PAID";
}

/// Request body for `POST /v1/melt/quote/bolt11` (NUT-05).
#[derive(Debug, Clone)]
pub struct MeltQuoteRequest {
    /// Lightning invoice (bolt11 string) to be paid.
    /// Demo shortcut: accepts any string as a dummy invoice.
    pub request: String,
    /// Unit (e.g. "sat").
    pub unit: String,
}

/// Response body for melt quote endpoints (NUT-05).
#[derive(Debug, Clone)]
pub struct MeltQuoteResponse {
    /// Unique quote identifier.
    pub quote: String,
    /// Amount to be melted.
    pub amount: u64,
    /// Fee reserve required for Lightning routing.
    /// Demo shortcut: always 0.
    pub fee_reserve: u64,
    /// Whether the payment has been made.
    pub paid: bool,
    /// Current state: "UNPAID", "PENDING", or "PAID".
    pub state: String,
    /// Expiry timestamp (unix seconds).
    pub expiry: u64,
}

/// Request body for `POST /v1/melt/bolt11` (NUT-05).
#[derive(Debug, Clone)]
pub struct MeltRequest {
    /// The quote ID referencing a melt quote.
    pub quote: String,
    /// Proofs (inputs) to spend for the payment.
    pub inputs: Vec<Proof>,
    /// Optional blinded messages for change outputs.
    pub outputs: Option<Vec<BlindedMessage>>,
}

/// Response body for `POST /v1/melt/bolt11` (NUT-05).
#[derive(Debug, Clone)]
pub struct MeltResponse {
    /// Whether the Lightning payment succeeded.
    pub paid: bool,
    /// Current state after melt.
    pub state: String,
    /// Payment preimage from Lightning (hex-encoded).
    /// Demo shortcut: a dummy hex string.
    pub payment_preimage: Option<String>,
    /// Blind signatures on any change outputs.
    pub change: Option<Vec<BlindSignature>>,
}
