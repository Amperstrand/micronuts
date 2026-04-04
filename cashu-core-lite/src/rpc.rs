//! Compact mint RPC boundary for host, serial, and future microfips transports.
//!
//! This keeps request/response shapes close to the existing NUT structs while
//! wrapping them in a small `minicbor`-encoded envelope suitable for any byte
//! transport.

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use minicbor::{Decode, Encode};

use crate::error::CashuError;
use crate::nuts::{nut01, nut02, nut03, nut04, nut05, nut06, nut07};
use crate::transport::MintClient;

/// NUT-04 quote lookup by quote id for the RPC layer.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MintQuoteLookupRequest {
    /// NUT-04 quote id.
    #[n(0)]
    pub quote: String,
}

/// NUT-05 quote lookup by quote id for the RPC layer.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MeltQuoteLookupRequest {
    /// NUT-05 quote id.
    #[n(0)]
    pub quote: String,
}

/// Wire-level mint RPC request envelope.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MintRpcRequest {
    /// Correlation id for request/response pairing over byte transports.
    #[n(0)]
    pub id: u32,
    /// Requested mint operation.
    #[n(1)]
    pub method: MintRpcMethod,
}

/// Wire-level mint RPC response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct MintRpcResponse {
    /// Correlation id copied from the request.
    #[n(0)]
    pub id: u32,
    /// Successful result or protocol/service error.
    #[n(1)]
    pub payload: MintRpcPayload,
}

/// Supported mint RPC methods for the demo NUT subset.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum MintRpcMethod {
    /// NUT-06: `GET /v1/info`
    #[n(0)]
    GetInfo,
    /// NUT-01: `GET /v1/keys`
    #[n(1)]
    GetKeys,
    /// NUT-02: `GET /v1/keysets`
    #[n(2)]
    GetKeysets,
    /// NUT-04: `POST /v1/mint/quote/bolt11`
    #[n(3)]
    MintQuote(#[n(0)] nut04::MintQuoteRequest),
    /// NUT-04: `GET /v1/mint/quote/bolt11/:quote`
    #[n(4)]
    GetMintQuote(#[n(0)] MintQuoteLookupRequest),
    /// NUT-04: `POST /v1/mint/bolt11`
    #[n(5)]
    Mint(#[n(0)] nut04::MintRequest),
    /// NUT-05: `POST /v1/melt/quote/bolt11`
    #[n(6)]
    MeltQuote(#[n(0)] nut05::MeltQuoteRequest),
    /// NUT-05: `GET /v1/melt/quote/bolt11/:quote`
    #[n(7)]
    GetMeltQuote(#[n(0)] MeltQuoteLookupRequest),
    /// NUT-05: `POST /v1/melt/bolt11`
    #[n(8)]
    Melt(#[n(0)] nut05::MeltRequest),
    /// NUT-03: `POST /v1/swap`
    #[n(9)]
    Swap(#[n(0)] nut03::SwapRequest),
    /// NUT-07: `POST /v1/checkstate`
    #[n(10)]
    CheckState(#[n(0)] nut07::CheckStateRequest),
}

/// RPC response payload.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum MintRpcPayload {
    /// Successful service response.
    #[n(0)]
    Success(#[n(0)] MintRpcResult),
    /// Explicit service/protocol failure.
    #[n(1)]
    Error(#[n(0)] CashuError),
}

/// Successful RPC results for the supported demo NUT subset.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum MintRpcResult {
    /// NUT-06: mint info.
    #[n(0)]
    GetInfo(#[n(0)] nut06::MintInfo),
    /// NUT-01: active keys.
    #[n(1)]
    GetKeys(#[n(0)] nut01::KeysResponse),
    /// NUT-02: keyset metadata.
    #[n(2)]
    GetKeysets(#[n(0)] nut02::KeysetsResponse),
    /// NUT-04: mint quote.
    #[n(3)]
    MintQuote(#[n(0)] nut04::MintQuoteResponse),
    /// NUT-04: mint quote lookup.
    #[n(4)]
    GetMintQuote(#[n(0)] nut04::MintQuoteResponse),
    /// NUT-04: mint blinded outputs.
    #[n(5)]
    Mint(#[n(0)] nut04::MintResponse),
    /// NUT-05: melt quote.
    #[n(6)]
    MeltQuote(#[n(0)] nut05::MeltQuoteResponse),
    /// NUT-05: melt quote lookup.
    #[n(7)]
    GetMeltQuote(#[n(0)] nut05::MeltQuoteResponse),
    /// NUT-05: melt spend.
    #[n(8)]
    Melt(#[n(0)] nut05::MeltResponse),
    /// NUT-03: swap.
    #[n(9)]
    Swap(#[n(0)] nut03::SwapResponse),
    /// NUT-07: spent-state check.
    #[n(10)]
    CheckState(#[n(0)] nut07::CheckStateResponse),
}

/// Minimal service trait for mint-side RPC handling.
///
/// This mirrors the existing NUT-shaped operations but lives on the server side.
pub trait MintService {
    /// NUT-06: get mint info.
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError>;
    /// NUT-01: get active keys.
    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError>;
    /// NUT-02: get keysets.
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError>;
    /// NUT-04: create mint quote.
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError>;
    /// NUT-04: fetch mint quote state.
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError>;
    /// NUT-04: mint blinded outputs.
    fn post_mint(&mut self, request: nut04::MintRequest) -> Result<nut04::MintResponse, CashuError>;
    /// NUT-05: create melt quote.
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError>;
    /// NUT-05: fetch melt quote state.
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError>;
    /// NUT-05: melt proofs.
    fn post_melt(&mut self, request: nut05::MeltRequest) -> Result<nut05::MeltResponse, CashuError>;
    /// NUT-03: swap proofs.
    fn post_swap(&mut self, request: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError>;
    /// NUT-07: check proof states.
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError>;
}

/// Encodes/decodes RPC envelopes and dispatches them to a mint service.
pub struct MintRpcHandler<S> {
    service: S,
}

impl<S> MintRpcHandler<S> {
    /// Create a new RPC handler around a mint service implementation.
    pub fn new(service: S) -> Self {
        Self { service }
    }

    /// Borrow the wrapped service.
    pub fn service(&self) -> &S {
        &self.service
    }

    /// Borrow the wrapped service mutably.
    pub fn service_mut(&mut self) -> &mut S {
        &mut self.service
    }
}

impl<S: MintService> MintRpcHandler<S> {
    /// Handle a decoded RPC request and return the matching response envelope.
    pub fn handle_request(&mut self, request: MintRpcRequest) -> MintRpcResponse {
        let payload = match request.method {
            MintRpcMethod::GetInfo => {
                rpc_success(self.service.get_info(), MintRpcResult::GetInfo)
            }
            MintRpcMethod::GetKeys => {
                rpc_success(self.service.get_keys(), MintRpcResult::GetKeys)
            }
            MintRpcMethod::GetKeysets => {
                rpc_success(self.service.get_keysets(), MintRpcResult::GetKeysets)
            }
            MintRpcMethod::MintQuote(body) => {
                rpc_success(self.service.post_mint_quote(body), MintRpcResult::MintQuote)
            }
            MintRpcMethod::GetMintQuote(body) => rpc_success(
                self.service.get_mint_quote(&body.quote),
                MintRpcResult::GetMintQuote,
            ),
            MintRpcMethod::Mint(body) => {
                rpc_success(self.service.post_mint(body), MintRpcResult::Mint)
            }
            MintRpcMethod::MeltQuote(body) => rpc_success(
                self.service.post_melt_quote(body),
                MintRpcResult::MeltQuote,
            ),
            MintRpcMethod::GetMeltQuote(body) => rpc_success(
                self.service.get_melt_quote(&body.quote),
                MintRpcResult::GetMeltQuote,
            ),
            MintRpcMethod::Melt(body) => {
                rpc_success(self.service.post_melt(body), MintRpcResult::Melt)
            }
            MintRpcMethod::Swap(body) => {
                rpc_success(self.service.post_swap(body), MintRpcResult::Swap)
            }
            MintRpcMethod::CheckState(body) => rpc_success(
                self.service.post_check_state(body),
                MintRpcResult::CheckState,
            ),
        };

        MintRpcResponse {
            id: request.id,
            payload,
        }
    }

    /// Decode request bytes, dispatch to the service, and encode the response.
    pub fn handle_bytes(&mut self, request_bytes: &[u8]) -> Result<Vec<u8>, CashuError> {
        let request = decode_rpc_request(request_bytes)?;
        let response = self.handle_request(request);
        encode_rpc_response(&response)
    }
}

/// Minimal synchronous byte transport for RPC exchange.
///
/// The same trait can later be implemented over serial framing or microfips
/// frames without changing the wallet-side RPC client.
pub trait RpcByteTransport {
    /// Send one request frame and return one response frame.
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, CashuError>;
}

/// Wallet-side mint client that speaks CBOR RPC over a byte transport.
pub struct RpcMintClient<T: RpcByteTransport> {
    transport: T,
    next_id: u32,
}

impl<T: RpcByteTransport> RpcMintClient<T> {
    /// Create a new RPC mint client over the provided byte transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: 1,
        }
    }

    /// Access the wrapped byte transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Mutable access to the wrapped byte transport.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn call(&mut self, method: MintRpcMethod) -> Result<MintRpcResult, CashuError> {
        let request_id = self.next_request_id();
        let request = MintRpcRequest {
            id: request_id,
            method,
        };
        let request_bytes = encode_rpc_request(&request)?;
        let response_bytes = self.transport.exchange(&request_bytes)?;
        let response = decode_rpc_response(&response_bytes)?;

        if response.id != request_id {
            return Err(CashuError::Protocol("rpc response id mismatch".to_string()));
        }

        match response.payload {
            MintRpcPayload::Success(result) => Ok(result),
            MintRpcPayload::Error(err) => Err(err),
        }
    }

    fn next_request_id(&mut self) -> u32 {
        let current = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        current
    }
}

impl<T: RpcByteTransport> MintClient for RpcMintClient<T> {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        match self.call(MintRpcMethod::GetInfo)? {
            MintRpcResult::GetInfo(info) => Ok(info),
            other => Err(unexpected_result("get_info", other)),
        }
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        match self.call(MintRpcMethod::GetKeys)? {
            MintRpcResult::GetKeys(keys) => Ok(keys),
            other => Err(unexpected_result("get_keys", other)),
        }
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        match self.call(MintRpcMethod::GetKeysets)? {
            MintRpcResult::GetKeysets(keysets) => Ok(keysets),
            other => Err(unexpected_result("get_keysets", other)),
        }
    }

    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        match self.call(MintRpcMethod::MintQuote(request))? {
            MintRpcResult::MintQuote(resp) => Ok(resp),
            other => Err(unexpected_result("post_mint_quote", other)),
        }
    }

    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        match self.call(MintRpcMethod::GetMintQuote(MintQuoteLookupRequest {
            quote: quote_id.to_string(),
        }))? {
            MintRpcResult::GetMintQuote(resp) => Ok(resp),
            other => Err(unexpected_result("get_mint_quote", other)),
        }
    }

    fn post_mint(&mut self, request: nut04::MintRequest) -> Result<nut04::MintResponse, CashuError> {
        match self.call(MintRpcMethod::Mint(request))? {
            MintRpcResult::Mint(resp) => Ok(resp),
            other => Err(unexpected_result("post_mint", other)),
        }
    }

    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        match self.call(MintRpcMethod::MeltQuote(request))? {
            MintRpcResult::MeltQuote(resp) => Ok(resp),
            other => Err(unexpected_result("post_melt_quote", other)),
        }
    }

    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        match self.call(MintRpcMethod::GetMeltQuote(MeltQuoteLookupRequest {
            quote: quote_id.to_string(),
        }))? {
            MintRpcResult::GetMeltQuote(resp) => Ok(resp),
            other => Err(unexpected_result("get_melt_quote", other)),
        }
    }

    fn post_melt(&mut self, request: nut05::MeltRequest) -> Result<nut05::MeltResponse, CashuError> {
        match self.call(MintRpcMethod::Melt(request))? {
            MintRpcResult::Melt(resp) => Ok(resp),
            other => Err(unexpected_result("post_melt", other)),
        }
    }

    fn post_swap(&mut self, request: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError> {
        match self.call(MintRpcMethod::Swap(request))? {
            MintRpcResult::Swap(resp) => Ok(resp),
            other => Err(unexpected_result("post_swap", other)),
        }
    }

    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        match self.call(MintRpcMethod::CheckState(request))? {
            MintRpcResult::CheckState(resp) => Ok(resp),
            other => Err(unexpected_result("post_check_state", other)),
        }
    }
}

/// Encode a request envelope to a compact CBOR frame.
pub fn encode_rpc_request(request: &MintRpcRequest) -> Result<Vec<u8>, CashuError> {
    minicbor::to_vec(request)
        .map_err(|err| CashuError::Protocol(format!("failed to encode rpc request: {err}")))
}

/// Decode a request envelope from a CBOR frame.
pub fn decode_rpc_request(bytes: &[u8]) -> Result<MintRpcRequest, CashuError> {
    minicbor::decode(bytes)
        .map_err(|err| CashuError::Protocol(format!("failed to decode rpc request: {err}")))
}

/// Encode a response envelope to a compact CBOR frame.
pub fn encode_rpc_response(response: &MintRpcResponse) -> Result<Vec<u8>, CashuError> {
    minicbor::to_vec(response)
        .map_err(|err| CashuError::Protocol(format!("failed to encode rpc response: {err}")))
}

/// Decode a response envelope from a CBOR frame.
pub fn decode_rpc_response(bytes: &[u8]) -> Result<MintRpcResponse, CashuError> {
    minicbor::decode(bytes)
        .map_err(|err| CashuError::Protocol(format!("failed to decode rpc response: {err}")))
}

fn rpc_success<T>(
    result: Result<T, CashuError>,
    success: impl FnOnce(T) -> MintRpcResult,
) -> MintRpcPayload {
    match result {
        Ok(value) => MintRpcPayload::Success(success(value)),
        Err(err) => MintRpcPayload::Error(err),
    }
}

fn unexpected_result(method: &str, result: MintRpcResult) -> CashuError {
    CashuError::Protocol(format!(
        "unexpected rpc result for {method}: {:?}",
        result
    ))
}
