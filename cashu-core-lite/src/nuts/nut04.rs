//! NUT-04: Mint Tokens (Bolt11)
//!
//! Request a mint quote (returns a Lightning invoice), then mint ecash once paid.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/04.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::nut00::{BlindSignature, BlindedMessage};

/// Quote states as defined in NUT-04.
pub mod state {
    pub const UNPAID: &str = "UNPAID";
    pub const PAID: &str = "PAID";
    pub const ISSUED: &str = "ISSUED";
}

/// Request body for `POST /v1/mint/quote/bolt11` (NUT-04).
#[derive(Debug, Clone)]
pub struct MintQuoteRequest {
    /// Amount to mint in the specified unit.
    pub amount: u64,
    /// Unit (e.g. "sat").
    pub unit: String,
}

/// Response body for mint quote endpoints (NUT-04).
#[derive(Debug, Clone)]
pub struct MintQuoteResponse {
    /// Unique quote identifier.
    pub quote: String,
    /// Payment request string (e.g. a Lightning invoice).
    /// Demo shortcut: this is a dummy string, not a real invoice.
    pub request: String,
    /// Whether the quote has been paid.
    pub paid: bool,
    /// Current state: "UNPAID", "PAID", or "ISSUED".
    pub state: String,
    /// Expiry timestamp (unix seconds). Demo shortcut: set far in the future.
    pub expiry: u64,
}

/// Request body for `POST /v1/mint/bolt11` (NUT-04).
#[derive(Debug, Clone)]
pub struct MintRequest {
    /// The quote ID referencing a paid mint quote.
    pub quote: String,
    /// Blinded messages (outputs) to be signed by the mint.
    pub outputs: Vec<BlindedMessage>,
}

/// Response body for `POST /v1/mint/bolt11` (NUT-04).
#[derive(Debug, Clone)]
pub struct MintResponse {
    /// Blind signatures on the outputs.
    pub signatures: Vec<BlindSignature>,
}
