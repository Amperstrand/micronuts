//! Error types for Cashu operations.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use core::fmt;

/// Errors that can occur during Cashu wallet/mint operations.
#[derive(Debug, Clone)]
pub enum CashuError {
    /// Transport-level error (network, USB, serial, etc.).
    Transport(String),
    /// Protocol-level error (malformed request/response).
    Protocol(String),
    /// Cryptographic error (invalid key, bad signature, etc.).
    Crypto(String),
    /// Requested amount is invalid (zero, overflow, etc.).
    InvalidAmount,
    /// Quote not found on the mint.
    QuoteNotFound,
    /// Quote exists but hasn't been paid yet.
    QuoteNotPaid,
    /// Quote has already been issued.
    QuoteAlreadyIssued,
    /// Sum of input proofs is insufficient for the request.
    InsufficientInputs,
    /// Proof failed verification.
    InvalidProof,
    /// Keyset ID not recognized by the mint.
    KeysetNotFound,
    /// Input and output amounts don't balance (NUT-03).
    AmountMismatch,
    /// Generic / uncategorized error.
    Unknown(String),
}

impl fmt::Display for CashuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "transport error: {}", msg),
            Self::Protocol(msg) => write!(f, "protocol error: {}", msg),
            Self::Crypto(msg) => write!(f, "crypto error: {}", msg),
            Self::InvalidAmount => write!(f, "invalid amount"),
            Self::QuoteNotFound => write!(f, "quote not found"),
            Self::QuoteNotPaid => write!(f, "quote not paid"),
            Self::QuoteAlreadyIssued => write!(f, "quote already issued"),
            Self::InsufficientInputs => write!(f, "insufficient inputs"),
            Self::InvalidProof => write!(f, "invalid proof"),
            Self::KeysetNotFound => write!(f, "keyset not found"),
            Self::AmountMismatch => write!(f, "input/output amount mismatch"),
            Self::Unknown(msg) => write!(f, "unknown error: {}", msg),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CashuError {}
