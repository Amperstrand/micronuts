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
// NUT #03: The swap operation is the most important component of the Cashu system. A swap operation consists of multiple inputs (`Proofs`) and outputs (`BlindedMessages`). Mints verify and invalidate the inputs and issue new promises (`BlindSignatures`). These are then used by the wallet to generate new `Proofs` (see [NUT-00][00]).
// NUT #03: "inputs": <Array[Proof]>,
// NUT #03: "outputs": <Array[BlindedMessage]>,
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
// NUT #03: "signatures": <Array[BlindSignature]>
pub struct SwapResponse {
    /// Blind signatures on the requested outputs.
    #[n(0)]
    pub signatures: Vec<BlindSignature>,
}
