//! Tests for NUT-00: Notation, ID, and Units
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut00::{
    decompose_amount, BlindSignature, BlindedMessage, ErrorResponse, Proof,
};
fn sample_public_key_bytes() -> [u8; 33] {
    [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ]
}

fn sample_public_key() -> PublicKey {
    PublicKey::from_bytes(&sample_public_key_bytes()).expect("valid public key")
}

#[test]
fn test_decompose_amount_zero() {
    assert_eq!(decompose_amount(0), Vec::<u64>::new());
}

#[test]
fn test_decompose_amount_powers_of_two() {
    assert_eq!(decompose_amount(1), vec![1]);
    assert_eq!(decompose_amount(2), vec![2]);
    assert_eq!(decompose_amount(4), vec![4]);
    assert_eq!(decompose_amount(8), vec![8]);
    assert_eq!(decompose_amount(64), vec![64]);
    assert_eq!(decompose_amount(128), vec![128]);
}

#[test]
fn test_decompose_amount_composite() {
    assert_eq!(decompose_amount(3), vec![2, 1]);
    assert_eq!(decompose_amount(5), vec![4, 1]);
    assert_eq!(decompose_amount(6), vec![4, 2]);
    assert_eq!(decompose_amount(7), vec![4, 2, 1]);
    assert_eq!(decompose_amount(13), vec![8, 4, 1]);
    assert_eq!(decompose_amount(100), vec![64, 32, 4]);
}

#[test]
fn test_decompose_amount_large_values() {
    assert_eq!(decompose_amount(1u64 << 32), vec![1u64 << 32]);
    assert_eq!(decompose_amount(1u64 << 63), vec![1u64 << 63]);
}

#[test]
fn test_decompose_amount_max() {
    let expected: Vec<u64> = (0..64).rev().map(|bit| 1u64 << bit).collect();
    assert_eq!(decompose_amount(u64::MAX), expected);
}

#[test]
fn test_decompose_amount_sums_correctly() {
    for amount in [1, 2, 3, 5, 7, 13, 42, 100, 255, 1000, 65535, 1000000] {
        let parts = decompose_amount(amount);
        let sum: u64 = parts.iter().sum();
        assert_eq!(sum, amount, "decompose_amount({}) sums to {}", amount, sum);
    }
}

#[test]
fn test_decompose_amount_powers_unique() {
    for amount in [3, 7, 15, 31, 63, 127, 255, 511, 1023] {
        let parts = decompose_amount(amount);
        let mut seen = std::collections::HashSet::new();
        for &p in &parts {
            assert!(
                seen.insert(p),
                "duplicate power in decompose_amount({}): {:?}",
                amount,
                parts
            );
        }
    }
}

#[test]
fn test_proof_cbor_roundtrip() {
    let proof = Proof {
        amount: 64,
        id: "009a1f293253e41e".to_string(),
        secret: "test_secret_hex".to_string(),
        c: sample_public_key(),
        dleq: None,
    };

    let mut buf = vec![];
    minicbor::encode(&proof, &mut buf).expect("encode proof");
    let decoded: Proof = minicbor::decode(&buf).expect("decode proof");

    assert_eq!(decoded.amount, proof.amount);
    assert_eq!(decoded.id, proof.id);
    assert!(
        decoded.dleq.is_none(),
        "absent dleq stays absent across CBOR"
    );
    assert_eq!(decoded.secret, proof.secret);
}

#[test]
fn test_blinded_message_cbor_roundtrip() {
    let msg = BlindedMessage {
        amount: 32,
        id: "009a1f293253e41e".to_string(),
        b: sample_public_key(),
    };

    let mut buf = vec![];
    minicbor::encode(&msg, &mut buf).expect("encode blinded message");
    let decoded: BlindedMessage = minicbor::decode(&buf).expect("decode blinded message");

    assert_eq!(decoded.amount, msg.amount);
    assert_eq!(decoded.id, msg.id);
}

#[test]
fn test_blind_signature_cbor_roundtrip() {
    let sig = BlindSignature {
        amount: 16,
        id: "009a1f293253e41e".to_string(),
        c: sample_public_key(),
        dleq: None,
    };

    let mut buf = vec![];
    minicbor::encode(&sig, &mut buf).expect("encode blind signature");
    let decoded: BlindSignature = minicbor::decode(&buf).expect("decode blind signature");

    assert_eq!(decoded.amount, sig.amount);
    assert_eq!(decoded.id, sig.id);
}

#[test]
fn test_error_response_cbor_roundtrip() {
    let err = ErrorResponse {
        detail: "insufficient funds".to_string(),
        code: 10001,
    };

    let mut buf = vec![];
    minicbor::encode(&err, &mut buf).expect("encode error response");
    let decoded: ErrorResponse = minicbor::decode(&buf).expect("decode error response");

    assert_eq!(decoded.detail, err.detail);
    assert_eq!(decoded.code, err.code);
}

#[test]
fn test_proof_different_amounts_different_cbor() {
    let p1 = Proof {
        amount: 1,
        id: "00".to_string(),
        secret: "s".to_string(),
        c: sample_public_key(),
        dleq: None,
    };
    let p2 = Proof {
        amount: 2,
        id: "00".to_string(),
        secret: "s".to_string(),
        c: sample_public_key(),
        dleq: None,
    };

    let mut buf1 = vec![];
    let mut buf2 = vec![];
    minicbor::encode(&p1, &mut buf1).unwrap();
    minicbor::encode(&p2, &mut buf2).unwrap();

    assert_ne!(buf1, buf2);
}

#[test]
fn test_blind_signature_equality() {
    let pk = sample_public_key();
    let s1 = BlindSignature {
        amount: 8,
        id: "00".to_string(),
        c: pk,
        dleq: None,
    };
    let s2 = BlindSignature {
        amount: 8,
        id: "00".to_string(),
        c: pk,
        dleq: None,
    };
    assert_eq!(s1, s2);
}
