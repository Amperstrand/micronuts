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
// NUT #09: Mints must store the `BlindedMessage` and the corresponding `BlindSignature` in their database every time they issue a `BlindSignature`. Wallets provide the `BlindedMessage` for which they request the `BlindSignature`. Mints only respond with a `BlindSignature`, if they have previously signed the `BlindedMessage`.
// NUT #09: "outputs": <Array[BlindedMessages]>
pub struct RestoreRequest {
    /// Blinded messages to restore (the spec's `outputs` list). Full
    /// BlindedMessage structs — mints validate `id` as a keyset ID
    /// (Amperstrand/micronuts#165: a bare B' hex list is rejected at
    /// deserialize). `amount` is unknown at restore time (0 is
    /// accepted); the mint matches on `B_` and the response carries the
    /// true amounts.
    #[n(0)]
    pub outputs: Vec<crate::nuts::nut00::BlindedMessage>,
}

/// A restored output with its blind signature (NUT-09).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct RestoreOutput {
    /// The blinded message `B'` this signature belongs to (the spec names
    /// this field `y`, but it carries `B'`, not `hash_to_curve(secret)`).
    #[n(0)]
    pub y: PublicKey,
    /// The blind signature for this output.
    #[n(1)]
    pub signature: BlindSignature,
}

/// Response body for `POST /v1/restore` (NUT-09).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
// NUT #09: The returned arrays `outputs` and `signatures` are of the same length and for every entry `outputs[i]`, there is a corresponding entry `signatures[i]`.
pub struct RestoreResponse {
    /// Matched outputs with their signatures.
    #[n(0)]
    pub outputs: Vec<RestoreOutput>,
}
