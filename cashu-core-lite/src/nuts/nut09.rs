//! NUT-09: Token Restoration
//!
//! Allows wallets to recover proofs from their Y values (`Y = hash_to_curve(secret)`).
//! The mint returns matching outputs with their blind signatures for re-unblinding.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/09.md
//!
//! Demo shortcut: v1.0 Restore is stateless — proofs only recoverable within current
//! mint session. No persistence; proofs lost on restart.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::keypair::PublicKey;
use crate::nuts::nut00::BlindSignature;
use minicbor::{Decode, Encode};

/// Request body for `POST /v1/restore` (NUT-09).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RestoreRequest {
    /// `Y` values: `hash_to_curve(secret)` for each proof to restore.
    #[n(0)]
    pub outputs: Vec<PublicKey>,
}

/// A restored output with its blind signature (NUT-09).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RestoreOutput {
    /// `Y = hash_to_curve(secret)` for this output.
    #[n(0)]
    pub y: PublicKey,
    /// The blind signature for this output.
    #[n(1)]
    pub signature: BlindSignature,
}

/// Response body for `POST /v1/restore` (NUT-09).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RestoreResponse {
    /// Matched outputs with their signatures.
    #[n(0)]
    pub outputs: Vec<RestoreOutput>,
}