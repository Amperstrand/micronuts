//! Tests for NUT-05: Melt Tokens (Bolt11)
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut00::{BlindSignature, BlindedMessage, Proof};
use cashu_core_lite::nuts::nut05::{
    MeltQuoteRequest, MeltQuoteResponse, MeltRequest, MeltResponse,
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
fn test_melt_quote_request_cbor_roundtrip() {
    let request = MeltQuoteRequest {
        request: "lnbc1000n1...".to_string(),
        unit: "sat".to_string(),
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MeltQuoteRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.request, "lnbc1000n1...");
    assert_eq!(decoded.unit, "sat");
}

#[test]
fn test_melt_quote_response_cbor_roundtrip() {
    let response = MeltQuoteResponse {
        quote: "melt_xyz".to_string(),
        amount: 1000,
        fee_reserve: 10,
        paid: false,
        state: "UNPAID".to_string(),
        expiry: 1893456000,
        request: "lnbcdemo10sat1micronuts".to_string(),
        unit: "sat".to_string(),
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: MeltQuoteResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.quote, "melt_xyz");
    assert_eq!(decoded.amount, 1000);
    assert_eq!(decoded.fee_reserve, 10);
}

#[test]
fn test_melt_request_cbor_roundtrip() {
    let request = MeltRequest {
        quote: "melt_abc".to_string(),
        inputs: vec![Proof {
            amount: 64,
            id: "009a1f293253e41e".to_string(),
            secret: "deadbeef".to_string(),
            c: sample_public_key(),

            dleq: None,
        }],
        outputs: None,
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MeltRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.quote, "melt_abc");
    assert_eq!(decoded.inputs.len(), 1);
    assert!(decoded.outputs.is_none());
}

#[test]
fn test_melt_response_cbor_roundtrip() {
    let response = MeltResponse {
        paid: true,
        state: "PAID".to_string(),
        payment_preimage: Some("preimage_hex".to_string()),
        change: None,
        quote: "q1".to_string(),
        amount: 100,
        fee_reserve: 0,
        unit: "sat".to_string(),
        expiry: 0,
        request: "lnbcdemo100sat1micronuts".to_string(),
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: MeltResponse = minicbor::decode(&buf).expect("decode");

    assert!(decoded.paid);
    assert_eq!(decoded.payment_preimage, Some("preimage_hex".to_string()));
}

#[test]
fn test_state_constants() {
    use cashu_core_lite::nuts::nut05::state;

    assert_eq!(state::UNPAID, "UNPAID");
    assert_eq!(state::PENDING, "PENDING");
    assert_eq!(state::PAID, "PAID");
}

#[test]
fn test_melt_request_with_change() {
    let request = MeltRequest {
        quote: "melt_change".to_string(),
        inputs: vec![Proof {
            amount: 64,
            id: "00".to_string(),
            secret: "s1".to_string(),
            c: sample_public_key(),

            dleq: None,
        }],
        outputs: Some(vec![BlindedMessage {
            amount: 10,
            id: "00".to_string(),
            b: sample_public_key(),
        }]),
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MeltRequest = minicbor::decode(&buf).expect("decode");

    assert!(decoded.outputs.is_some());
    assert_eq!(decoded.outputs.unwrap().len(), 1);
}

#[test]
fn test_melt_response_with_change() {
    let response = MeltResponse {
        paid: true,
        state: "PAID".to_string(),
        payment_preimage: Some("preimage".to_string()),
        quote: "q2".to_string(),
        amount: 100,
        fee_reserve: 1,
        unit: "sat".to_string(),
        expiry: 0,
        request: "lnbcdemo100sat1micronuts".to_string(),
        change: Some(vec![BlindSignature {
            amount: 10,
            id: "00".to_string(),
            c: sample_public_key(),
            dleq: None,
        }]),
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: MeltResponse = minicbor::decode(&buf).expect("decode");

    assert!(decoded.change.is_some());
    assert_eq!(decoded.change.unwrap().len(), 1);
}

#[test]
fn test_melt_multiple_inputs() {
    let request = MeltRequest {
        quote: "multi_melt".to_string(),
        inputs: vec![
            Proof {
                amount: 32,
                id: "00".to_string(),
                secret: "s1".to_string(),
                c: sample_public_key(),

                dleq: None,
            },
            Proof {
                amount: 32,
                id: "00".to_string(),
                secret: "s2".to_string(),
                c: sample_public_key(),

                dleq: None,
            },
        ],
        outputs: None,
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: MeltRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.inputs.len(), 2);

    let total: u64 = decoded.inputs.iter().map(|p| p.amount).sum();
    assert_eq!(total, 64);
}

#[test]
fn test_melt_state_transitions() {
    let unpaid = MeltQuoteResponse {
        quote: "q".to_string(),
        amount: 100,
        fee_reserve: 1,
        paid: false,
        state: "UNPAID".to_string(),
        expiry: 0,
        request: "lnbcdemo10sat1micronuts".to_string(),
        unit: "sat".to_string(),
    };

    let pending = MeltQuoteResponse {
        quote: "q".to_string(),
        amount: 100,
        fee_reserve: 1,
        paid: false,
        state: "PENDING".to_string(),
        expiry: 0,
        request: "lnbcdemo10sat1micronuts".to_string(),
        unit: "sat".to_string(),
    };

    let paid = MeltQuoteResponse {
        quote: "q".to_string(),
        amount: 100,
        fee_reserve: 1,
        paid: true,
        state: "PAID".to_string(),
        expiry: 0,
        request: "lnbcdemo10sat1micronuts".to_string(),
        unit: "sat".to_string(),
    };

    assert_eq!(unpaid.state, "UNPAID");
    assert_eq!(pending.state, "PENDING");
    assert_eq!(paid.state, "PAID");
}
