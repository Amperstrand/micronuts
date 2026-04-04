use cashu_core_lite::nuts::nut04;
use cashu_core_lite::rpc::{
    decode_rpc_response, encode_rpc_request, MintRpcMethod, MintRpcPayload, MintRpcRequest,
    MintRpcResult,
};
use cashu_core_lite::CashuError;
use micronuts_mint::{demo_mint_handler, handle_demo_mint_hex_request_line, handle_demo_mint_request_bytes};

#[test]
fn mint_role_bytes_handler_returns_info_response() {
    let mut handler = demo_mint_handler();
    let request = MintRpcRequest {
        id: 7,
        method: MintRpcMethod::GetInfo,
    };
    let request_bytes = encode_rpc_request(&request).expect("encode request");

    let response_bytes =
        handle_demo_mint_request_bytes(&mut handler, &request_bytes).expect("handle request");
    let response = decode_rpc_response(&response_bytes).expect("decode response");

    assert_eq!(response.id, 7);
    match response.payload {
        MintRpcPayload::Success(MintRpcResult::GetInfo(info)) => {
            assert_eq!(info.name, "Micronuts Demo Mint");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn mint_role_hex_handler_roundtrips_serialized_error() {
    let mut handler = demo_mint_handler();
    let request = MintRpcRequest {
        id: 8,
        method: MintRpcMethod::MintQuote(nut04::MintQuoteRequest {
            amount: 0,
            unit: "sat".to_string(),
        }),
    };
    let request_hex = hex::encode(encode_rpc_request(&request).expect("encode request"));

    let response_hex =
        handle_demo_mint_hex_request_line(&mut handler, &request_hex).expect("handle request");
    let response = decode_rpc_response(&hex::decode(response_hex).expect("decode hex response"))
        .expect("decode rpc response");

    assert_eq!(response.id, 8);
    match response.payload {
        MintRpcPayload::Error(CashuError::InvalidAmount) => {}
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn mint_role_hex_handler_rejects_non_hex_input() {
    let mut handler = demo_mint_handler();
    let err = handle_demo_mint_hex_request_line(&mut handler, "not-hex").unwrap_err();
    match err {
        CashuError::Protocol(message) => assert!(message.contains("failed to decode hex")),
        other => panic!("unexpected error: {other:?}"),
    }
}
