//! NUT-03: Swap (Split)
//!
//! Swap existing proofs for new blinded outputs. The mint verifies the input
//! proofs, checks amounts balance, and returns blind signatures on the outputs.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/03.md

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::nut00::{BlindSignature, BlindedMessage, Proof};
use minicbor::{Decode, Encode};

/// Request body for `POST /v1/swap` (NUT-03).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SwapRequest {
    /// Proofs to be swapped (consumed).
    #[n(0)]
    pub inputs: Vec<Proof>,
    /// Blinded messages for new outputs.
    #[n(1)]
    pub outputs: Vec<BlindedMessage>,
}

/// Response body for `POST /v1/swap` (NUT-03).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SwapResponse {
    /// Blind signatures on the requested outputs.
    #[n(0)]
    pub signatures: Vec<BlindSignature>,
}
