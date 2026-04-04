//! Keyset generation and management for the demo mint.
//!
//! NUT-01: Mint public keys — each denomination has its own keypair.
//! NUT-02: Keyset ID is derived from sorted compressed public keys.
//!
//! Demo shortcut: keys are derived deterministically from a fixed seed.
//! A real mint would derive keys from a mnemonic per NUT-13.

use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut01::{KeyPair, KeySet};
use cashu_core_lite::nuts::nut02::{derive_keyset_id, KeysetInfo};
use sha2::{Digest, Sha256};

/// Power-of-two denominations supported by this demo keyset.
/// Covers amounts from 1 to 128 sats (7 denominations).
pub const DENOMINATIONS: &[u64] = &[1, 2, 4, 8, 16, 32, 64, 128];

/// A demo keyset holding both private and public keys for each denomination.
pub struct DemoKeyset {
    /// The NUT-02 keyset ID (16 hex chars).
    pub id: String,
    /// Unit for this keyset.
    pub unit: String,
    /// Denomination → (private key, public key) pairs, sorted by amount ascending.
    pub keys: Vec<(u64, SecretKey, PublicKey)>,
}

impl DemoKeyset {
    /// Create a new demo keyset with deterministic keys derived from the given seed.
    ///
    /// Demo shortcut: keys are derived as `SHA256(seed || "cashu-key" || index_be)`.
    /// A real mint would use BIP-32 derivation from a mnemonic (NUT-13).
    pub fn new(seed: &[u8], unit: &str) -> Self {
        let mut keys = Vec::with_capacity(DENOMINATIONS.len());

        for (i, &amount) in DENOMINATIONS.iter().enumerate() {
            // Deterministic key derivation
            let key_bytes = Sha256::new()
                .chain_update(seed)
                .chain_update(b"cashu-key")
                .chain_update((i as u64).to_be_bytes())
                .finalize();

            let sk =
                SecretKey::from_slice(&key_bytes).expect("SHA-256 output is a valid scalar seed");
            let pk = sk.public_key();
            keys.push((amount, sk, pk));
        }

        // NUT-02: derive keyset ID from sorted public keys
        let sorted_pubkeys: Vec<PublicKey> = keys.iter().map(|(_, _, pk)| pk.clone()).collect();
        let id = derive_keyset_id(&sorted_pubkeys);

        Self {
            id,
            unit: unit.to_string(),
            keys,
        }
    }

    /// Create the keyset from the default demo seed.
    pub fn demo_default() -> Self {
        let seed = Sha256::new()
            .chain_update(b"micronuts-demo-mint-seed")
            .finalize();
        Self::new(&seed, "sat")
    }

    /// NUT-01: Export as a public KeySet (no private keys).
    pub fn to_public_keyset(&self) -> KeySet {
        KeySet {
            id: self.id.clone(),
            unit: self.unit.clone(),
            keys: self
                .keys
                .iter()
                .map(|(amount, _, pk)| KeyPair {
                    amount: *amount,
                    pubkey: pk.clone(),
                })
                .collect(),
        }
    }

    /// NUT-02: Export keyset metadata.
    pub fn to_keyset_info(&self) -> KeysetInfo {
        KeysetInfo {
            id: self.id.clone(),
            unit: self.unit.clone(),
            active: true,
            // Demo shortcut: no input fees
            input_fee_ppk: 0,
        }
    }

    /// Look up the private key for a given denomination amount.
    pub fn get_secret_key(&self, amount: u64) -> Option<&SecretKey> {
        self.keys
            .iter()
            .find(|(a, _, _)| *a == amount)
            .map(|(_, sk, _)| sk)
    }

    /// Look up the public key for a given denomination amount.
    pub fn get_public_key(&self, amount: u64) -> Option<&PublicKey> {
        self.keys
            .iter()
            .find(|(a, _, _)| *a == amount)
            .map(|(_, _, pk)| pk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_keyset_deterministic() {
        let ks1 = DemoKeyset::demo_default();
        let ks2 = DemoKeyset::demo_default();
        assert_eq!(ks1.id, ks2.id, "keyset IDs should be deterministic");
        assert_eq!(ks1.id.len(), 16, "keyset ID should be 16 hex chars");
        assert!(ks1.id.starts_with("00"), "keyset ID should start with version 00");
    }

    #[test]
    fn test_denominations_coverage() {
        let ks = DemoKeyset::demo_default();
        assert_eq!(ks.keys.len(), DENOMINATIONS.len());
        for &d in DENOMINATIONS {
            assert!(ks.get_secret_key(d).is_some(), "missing key for denomination {}", d);
            assert!(ks.get_public_key(d).is_some(), "missing pubkey for denomination {}", d);
        }
    }

    #[test]
    fn test_public_keyset_export() {
        let ks = DemoKeyset::demo_default();
        let public = ks.to_public_keyset();
        assert_eq!(public.id, ks.id);
        assert_eq!(public.unit, "sat");
        assert_eq!(public.keys.len(), DENOMINATIONS.len());
    }
}
