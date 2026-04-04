//! Host-side loopback byte transport for mint RPC.
//!
//! The wallet side encodes one CBOR RPC request, this transport forwards the
//! bytes into a mint-side `MintRpcHandler`, and returns the encoded response.
//! This proves the serialized RPC boundary without introducing networking.

use std::cell::RefCell;

use cashu_core_lite::error::CashuError;
use cashu_core_lite::rpc::{MintRpcHandler, RpcByteTransport, RpcMintClient};

use crate::DemoMint;

/// Host-side loopback transport that exchanges encoded RPC frames with a local
/// mint-side handler.
pub struct LoopbackTransport<S> {
    handler: RefCell<MintRpcHandler<S>>,
}

impl<S> LoopbackTransport<S> {
    /// Create a new loopback transport from a mint-side RPC handler.
    pub fn new(handler: MintRpcHandler<S>) -> Self {
        Self {
            handler: RefCell::new(handler),
        }
    }

    /// Borrow the wrapped handler.
    pub fn handler(&self) -> std::cell::Ref<'_, MintRpcHandler<S>> {
        self.handler.borrow()
    }
}

impl LoopbackTransport<DemoMint> {
    /// Convenience constructor for the common demo-mint loopback case.
    pub fn from_demo_mint(mint: DemoMint) -> Self {
        Self::new(MintRpcHandler::new(mint))
    }
}

impl<S: cashu_core_lite::rpc::MintService> RpcByteTransport for LoopbackTransport<S> {
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, CashuError> {
        self.handler.borrow_mut().handle_bytes(request)
    }
}

/// Default concrete wallet-side RPC client used by the host demo and tests.
pub type DemoRpcClient = RpcMintClient<LoopbackTransport<DemoMint>>;
