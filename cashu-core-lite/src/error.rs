//! Error types for Cashu operations.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use core::fmt;
use minicbor::{Decode, Encode};

/// Errors that can occur during Cashu wallet/mint operations.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum CashuError {
    /// Transport-level error (network, USB, serial, etc.).
    #[n(0)]
    Transport(#[n(0)] String),
    /// Protocol-level error (malformed request/response).
    #[n(1)]
    Protocol(#[n(0)] String),
    /// Cryptographic error (invalid key, bad signature, etc.).
    #[n(2)]
    Crypto(#[n(0)] String),
    /// Requested amount is invalid (zero, overflow, etc.).
    #[n(3)]
    InvalidAmount,
    /// Quote not found on the mint.
    #[n(4)]
    QuoteNotFound,
    /// Quote exists but hasn't been paid yet.
    #[n(5)]
    QuoteNotPaid,
    /// Quote has already been issued.
    #[n(6)]
    QuoteAlreadyIssued,
    /// Sum of input proofs is insufficient for the request.
    #[n(7)]
    InsufficientInputs,
    /// Proof failed verification.
    #[n(8)]
    InvalidProof,
    /// Keyset ID not recognized by the mint.
    #[n(9)]
    KeysetNotFound,
    /// Input and output amounts don't balance (NUT-03).
    #[n(10)]
    AmountMismatch,
    /// Generic / uncategorized error.
    #[n(11)]
    Unknown(#[n(0)] String),
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
