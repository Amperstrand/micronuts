//! Tests for NUT-07: Token State Check
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::keypair::PublicKey;
use cashu_core_lite::nuts::nut07::{CheckStateRequest, CheckStateResponse, ProofState};

fn sample_public_key() -> PublicKey {
    let bytes = [
        0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87,
        0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16,
        0xf8, 0x17, 0x98,
    ];
    PublicKey::from_bytes(&bytes).expect("valid public key")
}

#[test]
fn test_state_constants() {
    use cashu_core_lite::nuts::nut07::state;

    assert_eq!(state::UNSPENT, "UNSPENT");
    assert_eq!(state::SPENT, "SPENT");
    assert_eq!(state::PENDING, "PENDING");
}

#[test]
fn test_check_state_request_cbor_roundtrip() {
    let request = CheckStateRequest {
        ys: vec![sample_public_key()],
    };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: CheckStateRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.ys.len(), 1);
}

#[test]
fn test_proof_state_cbor_roundtrip() {
    let state = ProofState {
        y: sample_public_key(),
        state: "UNSPENT".to_string(),
        witness: None,
    };

    let mut buf = vec![];
    minicbor::encode(&state, &mut buf).expect("encode");
    let decoded: ProofState = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.state, "UNSPENT");
    assert!(decoded.witness.is_none());
}

#[test]
fn test_proof_state_with_witness() {
    let state = ProofState {
        y: sample_public_key(),
        state: "SPENT".to_string(),
        witness: Some("witness_data_hex".to_string()),
    };

    let mut buf = vec![];
    minicbor::encode(&state, &mut buf).expect("encode");
    let decoded: ProofState = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.witness, Some("witness_data_hex".to_string()));
}

#[test]
fn test_check_state_response_cbor_roundtrip() {
    let response = CheckStateResponse {
        states: vec![ProofState {
            y: sample_public_key(),
            state: "UNSPENT".to_string(),
            witness: None,
        }],
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: CheckStateResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.states.len(), 1);
}

#[test]
fn test_check_state_multiple_proofs() {
    let pk1 = sample_public_key();
    let pk2 = sample_public_key();

    let request = CheckStateRequest { ys: vec![pk1, pk2] };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: CheckStateRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.ys.len(), 2);
}

#[test]
fn test_check_state_response_mixed_states() {
    let pk = sample_public_key();

    let response = CheckStateResponse {
        states: vec![
            ProofState {
                y: pk.clone(),
                state: "UNSPENT".to_string(),
                witness: None,
            },
            ProofState {
                y: pk,
                state: "SPENT".to_string(),
                witness: Some("proof_used".to_string()),
            },
        ],
    };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: CheckStateResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.states.len(), 2);
    assert_eq!(decoded.states[0].state, "UNSPENT");
    assert_eq!(decoded.states[1].state, "SPENT");
}

#[test]
fn test_check_state_empty_request() {
    let request = CheckStateRequest { ys: vec![] };

    let mut buf = vec![];
    minicbor::encode(&request, &mut buf).expect("encode");
    let decoded: CheckStateRequest = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.ys.len(), 0);
}

#[test]
fn test_check_state_empty_response() {
    let response = CheckStateResponse { states: vec![] };

    let mut buf = vec![];
    minicbor::encode(&response, &mut buf).expect("encode");
    let decoded: CheckStateResponse = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.states.len(), 0);
}
