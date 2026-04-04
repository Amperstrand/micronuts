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
use minicbor::{Decode, Encode};

/// Quote states as defined in NUT-05.
pub mod state {
    pub const UNPAID: &str = "UNPAID";
    pub const PENDING: &str = "PENDING";
    pub const PAID: &str = "PAID";
}

/// Request body for `POST /v1/melt/quote/bolt11` (NUT-05).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MeltQuoteRequest {
    /// Lightning invoice (bolt11 string) to be paid.
    /// Demo shortcut: accepts any string as a dummy invoice.
    #[n(0)]
    pub request: String,
    /// Unit (e.g. "sat").
    #[n(1)]
    pub unit: String,
}

/// Response body for melt quote endpoints (NUT-05).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MeltQuoteResponse {
    /// Unique quote identifier.
    #[n(0)]
    pub quote: String,
    /// Amount to be melted.
    #[n(1)]
    pub amount: u64,
    /// Fee reserve required for Lightning routing.
    /// Demo shortcut: always 0.
    #[n(2)]
    pub fee_reserve: u64,
    /// Whether the payment has been made.
    #[n(3)]
    pub paid: bool,
    /// Current state: "UNPAID", "PENDING", or "PAID".
    #[n(4)]
    pub state: String,
    /// Expiry timestamp (unix seconds).
    #[n(5)]
    pub expiry: u64,
}

/// Request body for `POST /v1/melt/bolt11` (NUT-05).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MeltRequest {
    /// The quote ID referencing a melt quote.
    #[n(0)]
    pub quote: String,
    /// Proofs (inputs) to spend for the payment.
    #[n(1)]
    pub inputs: Vec<Proof>,
    /// Optional blinded messages for change outputs.
    #[n(2)]
    pub outputs: Option<Vec<BlindedMessage>>,
}

/// Response body for `POST /v1/melt/bolt11` (NUT-05).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MeltResponse {
    /// Whether the Lightning payment succeeded.
    #[n(0)]
    pub paid: bool,
    /// Current state after melt.
    #[n(1)]
    pub state: String,
    /// Payment preimage from Lightning (hex-encoded).
    /// Demo shortcut: a dummy hex string.
    #[n(2)]
    pub payment_preimage: Option<String>,
    /// Blind signatures on any change outputs.
    #[n(3)]
    pub change: Option<Vec<BlindSignature>>,
}
