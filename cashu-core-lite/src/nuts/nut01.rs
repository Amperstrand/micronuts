//! NUT-01: Mint Public Keys
//!
//! Defines the response format for `GET /v1/keys` — the mint's active
//! public keyset(s), mapping denomination amounts to public keys.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/01.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::keypair::PublicKey;
use minicbor::{Decode, Encode};

/// A single denomination-to-public-key mapping within a keyset.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct KeyPair {
    /// Power-of-two denomination (e.g. 1, 2, 4, 8, …).
    #[n(0)]
    pub amount: u64,
    /// The mint's public key for this denomination.
    #[n(1)]
    pub pubkey: PublicKey,
}

/// A complete keyset with its ID, unit, and denomination keys (NUT-01).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct KeySet {
    /// Keyset identifier (derived per NUT-02).
    #[n(0)]
    pub id: String,
    /// Unit of the keyset (e.g. "sat").
    #[n(1)]
    pub unit: String,
    /// The denomination keys in this keyset, sorted by amount ascending.
    #[n(2)]
    pub keys: Vec<KeyPair>,
}

/// Response for `GET /v1/keys` (NUT-01).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct KeysResponse {
    #[n(0)]
    pub keysets: Vec<KeySet>,
}
