//! NUT-02: Keysets and Keyset IDs
//!
//! Defines keyset metadata and the keyset ID derivation algorithm.
//! The keyset ID is: version_byte || hex(sha256(sorted_compressed_pubkeys))[0..14]
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/02.md

#[cfg(not(feature = "std"))]
use alloc::format;
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
///
/// Result: 16-char hex string like "009a1f293253e41e".
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

/// Keyset ID version (NUT-02): first hex byte of the ID string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysetIdVersion {
    /// `00` — legacy: sha256(sorted compressed pubkeys), 14 hex chars.
    V0,
    /// `01` — current: sha256 of a structured keyset string, 64 hex chars.
    V1,
}

/// Parse a keyset ID's version from its first byte. `None` for unknown
/// versions or malformed IDs.
pub fn keyset_id_version(id: &str) -> Option<KeysetIdVersion> {
    match id.as_bytes().first()? {
        b'0' => match id.as_bytes().get(1) {
            Some(b'0') => Some(KeysetIdVersion::V0),
            Some(b'1') => Some(KeysetIdVersion::V1),
            _ => None,
        },
        _ => None,
    }
}

/// Denomination→key pair used for ID derivation.
#[derive(Debug, Clone)]
pub struct AmountKey<'a> {
    pub amount: u64,
    pub pubkey: &'a PublicKey,
}

/// NUT-02 v1 ID: `00 || hex(sha256(sorted-by-amount compressed pubkeys))[..14]`.
///
/// Matches CDK `Id::v1_from_keys` byte-for-byte (verified by test).
pub fn derive_keyset_id_v1(keys: &[AmountKey<'_>]) -> String {
    let mut sorted: Vec<&AmountKey<'_>> = keys.iter().collect();
    sorted.sort_by_key(|k| k.amount);

    let mut hasher = Sha256::new();
    for k in sorted {
        Digest::update(&mut hasher, k.pubkey.to_encoded_point(true).as_bytes());
    }
    let hash = hasher.finalize();

    let mut id = String::with_capacity(16);
    id.push_str("00");
    for &byte in &hash[..7] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0x0F) as usize] as char);
    }
    id
}

/// NUT-02 v2 ID: `01 || hex(sha256(keyset_string))[..64]` where
/// `keyset_string` is `"{amount}:{pubkey_hex},…"` sorted by amount, then
/// `|unit:{unit}`, then `|input_fee_ppk:{n}` (only when > 0) and
/// `|final_expiry:{n}` (only when Some and > 0).
///
/// Matches CDK `Id::v2_from_data` byte-for-byte (verified by test).
// NUT #02: 2 - concatenate each amount and its corresponding lowercase public key hex string (as "amount:publickey_hex") to a single byte array, separating each pair with a comma (",")
// NUT #02: 3 - add the lowercase UTF8-encoded unit string prefixed with "|unit:" to the byte array (e.g. "|unit:sat")
// NUT #02: 4 - If input_fee_ppk is specified and non-zero, add the UTF8-encoded string prefixed with "|input_fee_ppk:" (e.g. "|input_fee_ppk:100"). If input_fee_ppk is omitted, null, or 0, it MUST be omitted from the preimage.
pub fn derive_keyset_id_v2(
    keys: &[AmountKey<'_>],
    unit: &str,
    input_fee_ppk: u64,
    final_expiry: Option<u64>,
) -> String {
    let mut sorted: Vec<&AmountKey<'_>> = keys.iter().collect();
    sorted.sort_by_key(|k| k.amount);

    let mut data = String::new();
    for (i, k) in sorted.iter().enumerate() {
        if i > 0 {
            data.push(',');
        }
        data.push_str(&format!(
            "{}:{}",
            k.amount,
            hex::encode(k.pubkey.to_bytes())
        ));
    }
    data.push_str(&format!("|unit:{}", unit));
    if input_fee_ppk > 0 {
        data.push_str(&format!("|input_fee_ppk:{}", input_fee_ppk));
    }
    if let Some(expiry) = final_expiry {
        if expiry > 0 {
            data.push_str(&format!("|final_expiry:{}", expiry));
        }
    }

    let hash = Sha256::digest(data.as_bytes());
    format!("01{}", hex::encode(&hash[..32]))
}

/// Check that a keyset actually derives the ID it claims — the
/// rotation-safe binding check. A wallet or gate handed keys plus an ID
/// (e.g. from a mint's `/v1/keys/{id}`) verifies the two belong together
/// regardless of active/inactive status. For v2 IDs the mint-reported
/// `input_fee_ppk` and `final_expiry` participate in the derivation and
/// must be supplied as the mint reported them. `None` = unknown ID
/// version; `Some(false)` = keys/params definitely do not derive `id`.
pub fn verify_keyset_id(
    id: &str,
    keys: &[AmountKey<'_>],
    unit: &str,
    input_fee_ppk: u64,
    final_expiry: Option<u64>,
) -> Option<bool> {
    match keyset_id_version(id)? {
        KeysetIdVersion::V0 => Some(derive_keyset_id_v1(keys) == id),
        KeysetIdVersion::V1 => {
            Some(derive_keyset_id_v2(keys, unit, input_fee_ppk, final_expiry) == id)
        }
    }
}
