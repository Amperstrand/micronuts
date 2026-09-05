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
// NUT #05: Melting tokens is the opposite of minting tokens (see [NUT-04][04]). Like minting tokens, melting is a two-step process: requesting a melt quote and melting tokens. This document describes the general flow that applies to all payment methods, with specifics for each supported payment method provided in dedicated method-specific NUTs.
// NUT #05: To request a melt quote, the wallet of `Alice` makes a `POST /v1/melt/quote/{method}` request where `method` is the payment method requested (e.g., `bolt11`, `bolt12`, etc.). `method` **MUST** match `[a-z0-9_-]+`.
// NUT #05: "request": <str>,
// NUT #05: "unit": <str_enum[UNIT]>,
// NUT #05: "amount": <int>   // Optional
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
    /// The payment request being paid (bolt11 string).
    #[n(6)]
    pub request: String,
    /// Unit of the quote (e.g. "sat").
    #[n(7)]
    pub unit: String,
    /// Payment method of the quote (NUT-05: response MUST carry it).
    #[n(8)]
    pub method: String,
}

/// Request body for `POST /v1/melt/bolt11` (NUT-05).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
// NUT #05: "quote": <str>,
// NUT #05: "inputs": <Array[Proof]>,
// NUT #05: "prefer_async": <bool> // optional: false if omitted
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
    /// Quote identifier (NUT-05: melt responses carry the quote fields).
    #[n(4)]
    pub quote: String,
    /// Melted amount in the quote's unit.
    #[n(5)]
    pub amount: u64,
    /// Fee reserve quoted for the payment.
    #[n(6)]
    pub fee_reserve: u64,
    /// Unit of the quote (e.g. "sat").
    #[n(7)]
    pub unit: String,
    /// Quote expiry (unix seconds).
    #[n(8)]
    pub expiry: u64,
    /// The payment request that was paid (bolt11 string).
    #[n(9)]
    pub request: String,
    /// Payment method of the melt (quote-shaped response, NUT-05).
    #[n(10)]
    pub method: String,
}
