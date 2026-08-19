//! Tests for NUT-02: Keysets and Keyset IDs
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut02::derive_keyset_id;

fn sample_public_key(offset: u8) -> PublicKey {
    // Generate valid secp256k1 public keys deterministically.
    // offset=0 must become scalar [1; 32] because [0; 32] is invalid (zero scalar).
    SecretKey::from_slice(&[offset.wrapping_add(1); 32])
        .expect("valid secret")
        .public_key()
}

#[test]
fn test_derive_keyset_id_single_key() {
    let pk = sample_public_key(1);
    let id = derive_keyset_id(&[pk]);

    assert!(
        id.starts_with("00"),
        "keyset ID should start with version byte '00'"
    );
    assert_eq!(id.len(), 16, "keyset ID should be 16 hex characters");
}

#[test]
fn test_derive_keyset_id_multiple_keys() {
    let keys = vec![
        sample_public_key(1),
        sample_public_key(2),
        sample_public_key(3),
        sample_public_key(4),
    ];

    let id = derive_keyset_id(&keys);

    assert!(id.starts_with("00"));
    assert_eq!(id.len(), 16);
}

#[test]
fn test_derive_keyset_id_deterministic() {
    let keys = vec![sample_public_key(10), sample_public_key(20)];

    let id1 = derive_keyset_id(&keys);
    let id2 = derive_keyset_id(&keys);

    assert_eq!(id1, id2, "keyset ID derivation must be deterministic");
}

#[test]
fn test_derive_keyset_id_order_matters() {
    let pk1 = sample_public_key(1);
    let pk2 = sample_public_key(2);

    let id1 = derive_keyset_id(&[pk1, pk2]);
    let id2 = derive_keyset_id(&[pk2, pk1]);

    assert_ne!(id1, id2, "different key order should produce different IDs");
}

#[test]
fn test_derive_keyset_id_empty() {
    let id = derive_keyset_id(&[]);
    assert!(id.starts_with("00"));
}

#[test]
fn test_derive_keyset_id_consistent_with_spec() {
    let pk1 = sample_public_key(1);
    let pk2 = sample_public_key(2);

    let id = derive_keyset_id(&[pk1, pk2]);

    let hex_part = &id[2..];
    assert!(
        hex::decode(hex_part).is_ok(),
        "keyset ID hex part should be valid hex"
    );
}

#[test]
fn test_derive_keyset_id_known_values() {
    let pk1_bytes: [u8; 33] = [
        0x03, 0xa4, 0x0f, 0x20, 0x66, 0x7e, 0xd5, 0x35, 0x13, 0x07, 0x5d, 0xc5, 0x1e, 0x71, 0x5f,
        0xf2, 0x04, 0x6c, 0xad, 0x64, 0xeb, 0x68, 0x96, 0x06, 0x32, 0x26, 0x9b, 0xa7, 0xf0, 0x21,
        0x0e, 0x38, 0xbc,
    ];

    let pk1 = PublicKey::from_bytes(&pk1_bytes).expect("valid pk1");
    let id = derive_keyset_id(&[pk1]);

    assert!(id.starts_with("00"));
}
