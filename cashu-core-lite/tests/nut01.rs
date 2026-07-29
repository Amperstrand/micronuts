//! Tests for NUT-01: Mint Public Keys
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut01::{KeyPair, KeySet, KeysResponse};

fn sample_public_key_bytes(offset: u8) -> [u8; 33] {
    use cashu_core_lite::keypair::SecretKey;
    SecretKey::from_slice(&[offset.wrapping_add(1); 32])
        .expect("valid secret")
        .public_key()
        .to_bytes()
}

fn sample_public_key(offset: u8) -> PublicKey {
    PublicKey::from_bytes(&sample_public_key_bytes(offset)).expect("valid public key")
}

#[test]
fn test_keypair_cbor_roundtrip() {
    let kp = KeyPair {
        amount: 8,
        pubkey: sample_public_key(0),
    };

    let mut buf = vec![];
    minicbor::encode(&kp, &mut buf).expect("encode keypair");
    let decoded: KeyPair = minicbor::decode(&buf).expect("decode keypair");

    assert_eq!(decoded.amount, kp.amount);
}

#[test]
fn test_keyset_cbor_roundtrip() {
    let keyset = KeySet {
        id: "009a1f293253e41e".to_string(),
        unit: "sat".to_string(),
        keys: vec![
            KeyPair {
                amount: 1,
                pubkey: sample_public_key(1),
            },
            KeyPair {
                amount: 2,
                pubkey: sample_public_key(2),
            },
            KeyPair {
                amount: 4,
                pubkey: sample_public_key(3),
            },
            KeyPair {
                amount: 8,
                pubkey: sample_public_key(4),
            },
        ],
    };

    let mut buf = vec![];
    minicbor::encode(&keyset, &mut buf).expect("encode keyset");
    let decoded: KeySet = minicbor::decode(&buf).expect("decode keyset");

    assert_eq!(decoded.id, keyset.id);
    assert_eq!(decoded.unit, keyset.unit);
    assert_eq!(decoded.keys.len(), 4);
}

#[test]
fn test_keys_response_cbor_roundtrip() {
    let response = KeysResponse {
        keysets: vec![KeySet {
            id: "009a1f293253e41e".to_string(),
            unit: "sat".to_string(),
            keys: vec![
                KeyPair {
                    amount: 1,
                    pubkey: sample_public_key(1),
                },
                KeyPair {
                    amount: 2,
                    pubkey: sample_public_key(2),
                },
            ],
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode keys response");
    let decoded: KeysResponse = minicbor::decode(&buf).expect("decode keys response");

    assert_eq!(decoded.keysets.len(), 1);
    assert_eq!(decoded.keysets[0].id, "009a1f293253e41e");
}

#[test]
fn test_keyset_sorted_by_amount() {
    let keyset = KeySet {
        id: "00".to_string(),
        unit: "sat".to_string(),
        keys: vec![
            KeyPair {
                amount: 1,
                pubkey: sample_public_key(1),
            },
            KeyPair {
                amount: 2,
                pubkey: sample_public_key(2),
            },
            KeyPair {
                amount: 4,
                pubkey: sample_public_key(3),
            },
            KeyPair {
                amount: 8,
                pubkey: sample_public_key(4),
            },
        ],
    };

    let amounts: Vec<u64> = keyset.keys.iter().map(|k| k.amount).collect();
    let mut sorted = amounts.clone();
    sorted.sort();
    assert_eq!(amounts, sorted, "keys should be sorted by amount ascending");
}

#[test]
fn test_keyset_different_units() {
    let sat_keyset = KeySet {
        id: "00sat".to_string(),
        unit: "sat".to_string(),
        keys: vec![KeyPair {
            amount: 1,
            pubkey: sample_public_key(0),
        }],
    };

    let msat_keyset = KeySet {
        id: "00msat".to_string(),
        unit: "msat".to_string(),
        keys: vec![KeyPair {
            amount: 1000,
            pubkey: sample_public_key(1),
        }],
    };

    assert_ne!(sat_keyset.unit, msat_keyset.unit);
    assert_ne!(sat_keyset.id, msat_keyset.id);
}

#[test]
fn test_empty_keyset() {
    let keyset = KeySet {
        id: "00empty".to_string(),
        unit: "sat".to_string(),
        keys: vec![],
    };

    let mut buf = vec![];
    minicbor::encode(&keyset, &mut buf).expect("encode empty keyset");
    let decoded: KeySet = minicbor::decode(&buf).expect("decode empty keyset");

    assert_eq!(decoded.keys.len(), 0);
}

#[test]
fn test_large_keyset() {
    let keys: Vec<KeyPair> = (0..32)
        .map(|i| KeyPair {
            amount: 1u64 << i,
            pubkey: sample_public_key(i as u8),
        })
        .collect();

    let keyset = KeySet {
        id: "00large".to_string(),
        unit: "sat".to_string(),
        keys,
    };

    assert_eq!(keyset.keys.len(), 32);
    assert_eq!(keyset.keys[0].amount, 1);
    assert_eq!(keyset.keys[31].amount, 1u64 << 31);
}

#[test]
fn test_keypair_equality() {
    let kp1 = KeyPair {
        amount: 4,
        pubkey: sample_public_key(0),
    };
    let kp2 = KeyPair {
        amount: 4,
        pubkey: sample_public_key(0),
    };
    assert_eq!(kp1, kp2);
}
