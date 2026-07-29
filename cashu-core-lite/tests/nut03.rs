//! Tests for NUT-03: Swap
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut00::{BlindSignature, BlindedMessage, Proof};
use cashu_core_lite::nuts::nut03::{SwapRequest, SwapResponse};

fn sample_public_key() -> PublicKey {
    let bytes = [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ];
    PublicKey::from_bytes(&bytes).expect("valid public key")
}

#[test]
fn test_swap_request_cbor_roundtrip() {
    let request = SwapRequest {
        inputs: vec![Proof {
            amount: 8,
            id: "009a1f293253e41e".to_string(),
            secret: "deadbeef".to_string(),
            c: sample_public_key(),
        }],
        outputs: vec![BlindedMessage {
            amount: 4,
            id: "009a1f293253e41e".to_string(),
            b: sample_public_key(),
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode swap request");
    let decoded: SwapRequest = minicbor::decode(&buf).expect("decode swap request");

    assert_eq!(decoded.inputs.len(), 1);
    assert_eq!(decoded.outputs.len(), 1);
}

#[test]
fn test_swap_response_cbor_roundtrip() {
    let response = SwapResponse {
        signatures: vec![BlindSignature {
            amount: 4,
            id: "009a1f293253e41e".to_string(),
            c: sample_public_key(),
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode swap response");
    let decoded: SwapResponse = minicbor::decode(&buf).expect("decode swap response");

    assert_eq!(decoded.signatures.len(), 1);
}

#[test]
fn test_swap_request_multiple_inputs_outputs() {
    let request = SwapRequest {
        inputs: vec![
            Proof {
                amount: 4,
                id: "00".to_string(),
                secret: "s1".to_string(),
                c: sample_public_key(),
            },
            Proof {
                amount: 4,
                id: "00".to_string(),
                secret: "s2".to_string(),
                c: sample_public_key(),
            },
        ],
        outputs: vec![BlindedMessage {
            amount: 8,
            id: "00".to_string(),
            b: sample_public_key(),
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: SwapRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.inputs.len(), 2);
    assert_eq!(decoded.outputs.len(), 1);

    let input_sum: u64 = decoded.inputs.iter().map(|p| p.amount).sum();
    let output_sum: u64 = decoded.outputs.iter().map(|o| o.amount).sum();
    assert_eq!(input_sum, output_sum);
}

#[test]
fn test_swap_request_empty_inputs() {
    let request = SwapRequest {
        inputs: vec![],
        outputs: vec![],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: SwapRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.inputs.len(), 0);
    assert_eq!(decoded.outputs.len(), 0);
}

#[test]
fn test_swap_response_empty_signatures() {
    let response = SwapResponse { signatures: vec![] };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: SwapResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.signatures.len(), 0);
}

#[test]
fn test_swap_preserves_keyset_id() {
    let keyset_id = "009a1f293253e41e".to_string();

    let request = SwapRequest {
        inputs: vec![Proof {
            amount: 1,
            id: keyset_id.clone(),
            secret: "s".to_string(),
            c: sample_public_key(),
        }],
        outputs: vec![BlindedMessage {
            amount: 1,
            id: keyset_id.clone(),
            b: sample_public_key(),
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: SwapRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.inputs[0].id, keyset_id);
    assert_eq!(decoded.outputs[0].id, keyset_id);
}
