//! Tests for NUT-04: Mint Tokens (Bolt11)
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut00::{BlindSignature, BlindedMessage};
use cashu_core_lite::nuts::nut04::{
    MintQuoteRequest, MintQuoteResponse, MintRequest, MintResponse,
};

fn sample_public_key() -> PublicKey {
    let bytes = [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ];
    PublicKey::from_bytes(&bytes).expect("valid public key")
}

#[test]
fn test_mint_quote_request_cbor_roundtrip() {
    let request = MintQuoteRequest {
        amount: 1000,
        unit: "sat".to_string(),
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MintQuoteRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.amount, 1000);
    assert_eq!(decoded.unit, "sat");
}

#[test]
fn test_mint_quote_response_cbor_roundtrip() {
    let response = MintQuoteResponse {
        quote: "quote_abc123".to_string(),
        request: "lnbc1000n1...".to_string(),
        paid: false,
        state: "UNPAID".to_string(),
        expiry: 1893456000,
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: MintQuoteResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.quote, "quote_abc123");
    assert_eq!(decoded.paid, false);
    assert_eq!(decoded.state, "UNPAID");
}

#[test]
fn test_mint_request_cbor_roundtrip() {
    let request = MintRequest {
        quote: "quote_xyz".to_string(),
        outputs: vec![BlindedMessage {
            amount: 64,
            id: "009a1f293253e41e".to_string(),
            b: sample_public_key(),
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MintRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.quote, "quote_xyz");
    assert_eq!(decoded.outputs.len(), 1);
}

#[test]
fn test_mint_response_cbor_roundtrip() {
    let response = MintResponse {
        signatures: vec![BlindSignature {
            amount: 64,
            id: "009a1f293253e41e".to_string(),
            c: sample_public_key(),
                dleq: None,
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: MintResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.signatures.len(), 1);
}

#[test]
fn test_state_constants() {
    use cashu_core_lite::nuts::nut04::state;

    assert_eq!(state::UNPAID, "UNPAID");
    assert_eq!(state::PAID, "PAID");
    assert_eq!(state::ISSUED, "ISSUED");
}

#[test]
fn test_mint_quote_different_units() {
    let sat_request = MintQuoteRequest {
        amount: 1000,
        unit: "sat".to_string(),
    };

    let msat_request = MintQuoteRequest {
        amount: 1000000,
        unit: "msat".to_string(),
    };

    assert_ne!(sat_request.unit, msat_request.unit);
}

#[test]
fn test_mint_quote_state_transitions() {
    let unpaid = MintQuoteResponse {
        quote: "q1".to_string(),
        request: "req".to_string(),
        paid: false,
        state: "UNPAID".to_string(),
        expiry: 0,
    };

    let paid = MintQuoteResponse {
        quote: "q1".to_string(),
        request: "req".to_string(),
        paid: true,
        state: "PAID".to_string(),
        expiry: 0,
    };

    let issued = MintQuoteResponse {
        quote: "q1".to_string(),
        request: "req".to_string(),
        paid: true,
        state: "ISSUED".to_string(),
        expiry: 0,
    };

    assert!(!unpaid.paid);
    assert!(paid.paid);
    assert_eq!(issued.state, "ISSUED");
}

#[test]
fn test_mint_request_multiple_outputs() {
    let request = MintRequest {
        quote: "multi_quote".to_string(),
        outputs: vec![
            BlindedMessage {
                amount: 1,
                id: "00".to_string(),
                b: sample_public_key(),
            },
            BlindedMessage {
                amount: 2,
                id: "00".to_string(),
                b: sample_public_key(),
            },
            BlindedMessage {
                amount: 4,
                id: "00".to_string(),
                b: sample_public_key(),
            },
        ],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MintRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.outputs.len(), 3);

    let total: u64 = decoded.outputs.iter().map(|o| o.amount).sum();
    assert_eq!(total, 7);
}
