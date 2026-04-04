//! NUT-07: Token State Check
//!
//! Allows wallets to check whether proofs have been spent without revealing
//! the secrets — only the `Y = hash_to_curve(secret)` value is sent.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/07.md
//!
//! Demo shortcut: unlike NUT-07 production behavior, this build keeps no
//! durable spent-proof state. The in-memory set is only populated within
//! a single session and is lost on restart.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::keypair::PublicKey;
use minicbor::{Decode, Encode};

/// Proof state values as defined in NUT-07.
pub mod state {
    pub const UNSPENT: &str = "UNSPENT";
    pub const SPENT: &str = "SPENT";
    pub const PENDING: &str = "PENDING";
}

/// Request body for `POST /v1/checkstate` (NUT-07).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckStateRequest {
    /// `Y` values: `hash_to_curve(secret)` for each proof to check.
    #[n(0)]
    pub ys: Vec<PublicKey>,
}

/// Individual proof state entry in the response (NUT-07).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ProofState {
    /// `Y = hash_to_curve(secret)` for this proof.
    #[n(0)]
    pub y: PublicKey,
    /// Current state: "UNSPENT", "SPENT", or "PENDING".
    #[n(1)]
    pub state: String,
    /// Optional witness data.
    #[n(2)]
    pub witness: Option<String>,
}

/// Response body for `POST /v1/checkstate` (NUT-07).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct CheckStateResponse {
    #[n(0)]
    pub states: Vec<ProofState>,
}
