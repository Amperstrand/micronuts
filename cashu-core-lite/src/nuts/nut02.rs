//! NUT-02: Keysets and Keyset IDs
//!
//! Defines keyset metadata and the keyset ID derivation algorithm.
//! The keyset ID is: version_byte || hex(sha256(sorted_compressed_pubkeys))[0..14]
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/02.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::keypair::PublicKey;
use minicbor::{Decode, Encode};
use sha2::{Digest, Sha256};

/// Keyset metadata (NUT-02).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct KeysetInfo {
    /// Keyset identifier (16 hex chars, e.g. "009a1f293253e41e").
    #[n(0)]
    pub id: String,
    /// Unit (e.g. "sat").
    #[n(1)]
    pub unit: String,
    /// Whether this keyset is currently active for new signatures.
    #[n(2)]
    pub active: bool,
    /// Input fee in parts per thousand (NUT-02 fee field).
    /// Demo shortcut: typically 0 for demo mints.
    #[n(3)]
    pub input_fee_ppk: u64,
}

/// Response for `GET /v1/keysets` (NUT-02).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct KeysetsResponse {
    #[n(0)]
    pub keysets: Vec<KeysetInfo>,
}

/// Derive a NUT-02 keyset ID from a list of public keys.
///
/// Algorithm (NUT-02 §Keyset ID):
///   1. Sort public keys by their associated amount (caller must pass them sorted).
///   2. Concatenate all compressed (33-byte) public keys.
///   3. SHA-256 hash the concatenation.
///   4. Take first 7 bytes of hash → hex-encode → 14 hex chars.
///   5. Prepend version byte "00".
///   Result: 16-char hex string like "009a1f293253e41e".
pub fn derive_keyset_id(sorted_pubkeys: &[PublicKey]) -> String {
    let mut hasher = Sha256::new();
    for pk in sorted_pubkeys {
        let compressed = pk.to_encoded_point(true);
        Digest::update(&mut hasher, compressed.as_bytes());
    }
    let hash = hasher.finalize();

    // Take first 7 bytes, hex-encode to 14 chars, prepend "00"
    let mut id = String::with_capacity(16);
    id.push_str("00");
    for &byte in &hash[..7] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0x0F) as usize] as char);
    }
    id
}
