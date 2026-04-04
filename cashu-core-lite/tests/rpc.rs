use cashu_core_lite::nuts::{nut00, nut03, nut04, nut05, nut06, nut07};
use cashu_core_lite::rpc::{
    decode_rpc_request, decode_rpc_response, encode_rpc_request, encode_rpc_response,
    MeltQuoteLookupRequest, MintQuoteLookupRequest, MintRpcMethod, MintRpcPayload,
    MintRpcRequest, MintRpcResponse, MintRpcResult,
};
use cashu_core_lite::{CashuError, PublicKey, SecretKey};

fn sample_public_key(seed: u8) -> PublicKey {
    let mut secret_bytes = [0u8; 32];
    secret_bytes[31] = seed.max(1);
    SecretKey::from_slice(&secret_bytes)
        .expect("valid secret")
        .public_key()
}

fn sample_blinded_message(seed: u8) -> nut00::BlindedMessage {
    nut00::BlindedMessage {
        amount: u64::from(seed),
        id: format!("keyset-{seed}"),
        b: sample_public_key(seed),
    }
}

fn sample_blind_signature(seed: u8) -> nut00::BlindSignature {
    nut00::BlindSignature {
        amount: u64::from(seed),
        id: format!("keyset-{seed}"),
        c: sample_public_key(seed.saturating_add(10)),
    }
}

fn sample_proof(seed: u8) -> nut00::Proof {
    nut00::Proof {
        amount: u64::from(seed),
        id: format!("keyset-{seed}"),
        secret: format!("{seed:02x}{seed:02x}"),
        c: sample_public_key(seed.saturating_add(20)),
    }
}

#[test]
fn rpc_request_roundtrip_covers_all_demo_methods() {
    let requests = vec![
        MintRpcRequest {
            id: 1,
            method: MintRpcMethod::GetInfo,
        },
        MintRpcRequest {
            id: 2,
            method: MintRpcMethod::GetKeys,
        },
        MintRpcRequest {
            id: 3,
            method: MintRpcMethod::GetKeysets,
        },
        MintRpcRequest {
            id: 4,
            method: MintRpcMethod::MintQuote(nut04::MintQuoteRequest {
                amount: 100,
                unit: "sat".to_string(),
            }),
        },
        MintRpcRequest {
            id: 5,
            method: MintRpcMethod::GetMintQuote(MintQuoteLookupRequest {
                quote: "mint-quote-1".to_string(),
            }),
        },
        MintRpcRequest {
            id: 6,
            method: MintRpcMethod::Mint(nut04::MintRequest {
                quote: "mint-quote-1".to_string(),
                outputs: vec![sample_blinded_message(4), sample_blinded_message(8)],
            }),
        },
        MintRpcRequest {
            id: 7,
            method: MintRpcMethod::MeltQuote(nut05::MeltQuoteRequest {
                request: "lnbcdemo64sat1micronuts".to_string(),
                unit: "sat".to_string(),
            }),
        },
        MintRpcRequest {
            id: 8,
            method: MintRpcMethod::GetMeltQuote(MeltQuoteLookupRequest {
                quote: "melt-quote-1".to_string(),
            }),
        },
        MintRpcRequest {
            id: 9,
            method: MintRpcMethod::Melt(nut05::MeltRequest {
                quote: "melt-quote-1".to_string(),
                inputs: vec![sample_proof(32), sample_proof(16)],
                outputs: Some(vec![sample_blinded_message(1)]),
            }),
        },
        MintRpcRequest {
            id: 10,
            method: MintRpcMethod::Swap(nut03::SwapRequest {
                inputs: vec![sample_proof(8)],
                outputs: vec![sample_blinded_message(4), sample_blinded_message(4)],
            }),
        },
        MintRpcRequest {
            id: 11,
            method: MintRpcMethod::CheckState(nut07::CheckStateRequest {
                ys: vec![sample_public_key(1), sample_public_key(2)],
            }),
        },
    ];

    for request in requests {
        let encoded = encode_rpc_request(&request).expect("encode request");
        let decoded = decode_rpc_request(&encoded).expect("decode request");
        assert_eq!(decoded, request);
    }
}

#[test]
fn rpc_response_roundtrip_success() {
    let response = MintRpcResponse {
        id: 77,
        payload: MintRpcPayload::Success(MintRpcResult::GetInfo(nut06::MintInfo {
            name: "Micronuts Demo Mint".to_string(),
            pubkey: "021234".to_string(),
            version: "micronuts-mint/0.1.0".to_string(),
            description: "Demo".to_string(),
            contact: vec![],
            nuts: nut06::NutSupport {
                supported: vec![0, 1, 2, 3, 4, 5, 6, 7],
            },
        })),
    };

    let encoded = encode_rpc_response(&response).expect("encode response");
    let decoded = decode_rpc_response(&encoded).expect("decode response");
    assert_eq!(decoded, response);
}

#[test]
fn rpc_response_roundtrip_error() {
    let response = MintRpcResponse {
        id: 99,
        payload: MintRpcPayload::Error(CashuError::Protocol(
            "bad rpc frame".to_string(),
        )),
    };

    let encoded = encode_rpc_response(&response).expect("encode response");
    let decoded = decode_rpc_response(&encoded).expect("decode response");
    assert_eq!(decoded, response);
}

#[test]
fn rpc_response_roundtrip_swap_result() {
    let response = MintRpcResponse {
        id: 123,
        payload: MintRpcPayload::Success(MintRpcResult::Swap(nut03::SwapResponse {
            signatures: vec![sample_blind_signature(1), sample_blind_signature(2)],
        })),
    };

    let encoded = encode_rpc_response(&response).expect("encode response");
    let decoded = decode_rpc_response(&encoded).expect("decode response");
    assert_eq!(decoded, response);
}
