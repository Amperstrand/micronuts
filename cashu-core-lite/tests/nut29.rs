//! Tests for NUT-29 wire types: CBOR roundtrips through the RPC boundary.

use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut00::BlindedMessage;
use cashu_core_lite::nuts::nut29::{BatchCheckMintQuoteRequest, BatchMintRequest};

fn sample_point(seed: u8) -> PublicKey {
    SecretKey::from_slice(&[seed; 32]).unwrap().public_key()
}

fn sample_output(amount: u64, seed: u8) -> BlindedMessage {
    BlindedMessage {
        amount,
        id: "009a1f293253e41e".to_string(),
        b: sample_point(seed),
    }
}

#[test]
fn batch_check_request_cbor_roundtrip() {
    let request = BatchCheckMintQuoteRequest {
        quotes: vec![
            "019e6d5a-2347-7000-8037-b42dae20f1fe".to_string(),
            "019e6d5a-2347-7000-868a-08e59a1c6716".to_string(),
        ],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: BatchCheckMintQuoteRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded, request);
}

#[test]
fn batch_mint_request_cbor_roundtrip() {
    let request = BatchMintRequest {
        quotes: vec!["q1".to_string(), "q2".to_string()],
        quote_amounts: Some(vec![100, 50]),
        outputs: vec![sample_output(128, 1), sample_output(16, 2)],
        signatures: Some(vec![Some("ab".repeat(64)), None]),
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: BatchMintRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded, request);
}

#[test]
fn batch_mint_request_minimal_fields_roundtrip() {
    let request = BatchMintRequest {
        quotes: vec!["q1".to_string()],
        quote_amounts: None,
        outputs: vec![sample_output(1, 3)],
        signatures: None,
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: BatchMintRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded, request);
}
