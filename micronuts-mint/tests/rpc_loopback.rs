use cashu_core_lite::nuts::nut04;
use cashu_core_lite::rpc::{
    decode_rpc_response, encode_rpc_request, MintRpcMethod, MintRpcPayload, MintRpcRequest,
    MintRpcResult, RpcByteTransport,
};
use cashu_core_lite::CashuError;
use micronuts_mint::{DemoMint, LoopbackTransport};

#[test]
fn loopback_transport_handles_rpc_bytes_for_get_info() {
    let mut transport = LoopbackTransport::from_demo_mint(DemoMint::new());
    let request = MintRpcRequest {
        id: 1,
        method: MintRpcMethod::GetInfo,
    };

    let request_bytes = encode_rpc_request(&request).expect("encode request");
    let response_bytes = transport.exchange(&request_bytes).expect("exchange succeeds");
    let response = decode_rpc_response(&response_bytes).expect("decode response");

    assert_eq!(response.id, 1);
    match response.payload {
        MintRpcPayload::Success(MintRpcResult::GetInfo(info)) => {
            assert_eq!(info.name, "Micronuts Demo Mint");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn loopback_transport_returns_serialized_errors() {
    let mut transport = LoopbackTransport::from_demo_mint(DemoMint::new());
    let request = MintRpcRequest {
        id: 2,
        method: MintRpcMethod::MintQuote(nut04::MintQuoteRequest {
            amount: 0,
            unit: "sat".to_string(),
        }),
    };

    let request_bytes = encode_rpc_request(&request).expect("encode request");
    let response_bytes = transport.exchange(&request_bytes).expect("exchange succeeds");
    let response = decode_rpc_response(&response_bytes).expect("decode response");

    assert_eq!(response.id, 2);
    match response.payload {
        MintRpcPayload::Error(CashuError::InvalidAmount) => {}
        other => panic!("unexpected payload: {other:?}"),
    }
}
