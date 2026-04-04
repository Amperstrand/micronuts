//! Direct (in-process) transport adapter.
//!
//! `DirectTransport` implements `MintClient` by calling the `DemoMint`
//! directly. No network, no serialization — just function calls.
//!
//! This is the simplest transport for testing and demo purposes.
//! Future adapters (USB, HTTP, microfips) would implement `MintClient`
//! with real transport underneath, but the wallet code stays the same.

use std::cell::RefCell;

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut01, nut02, nut03, nut04, nut05, nut06, nut07};
use cashu_core_lite::transport::MintClient;

use crate::DemoMint;

/// In-process transport that forwards all calls to a `DemoMint` instance.
///
/// Uses `RefCell` for interior mutability since the mint needs `&mut self`
/// but the transport trait uses `&mut self` too.
pub struct DirectTransport {
    mint: RefCell<DemoMint>,
}

impl DirectTransport {
    /// Wrap an existing `DemoMint` in a direct transport.
    pub fn new(mint: DemoMint) -> Self {
        Self {
            mint: RefCell::new(mint),
        }
    }

    /// Access the underlying mint (for inspection in tests).
    pub fn mint(&self) -> std::cell::Ref<'_, DemoMint> {
        self.mint.borrow()
    }
}

impl MintClient for DirectTransport {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        self.mint.borrow().get_info()
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        self.mint.borrow().get_keys()
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        self.mint.borrow().get_keysets()
    }

    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.mint.borrow_mut().post_mint_quote(request)
    }

    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.mint.borrow().get_mint_quote(quote_id)
    }

    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        self.mint.borrow_mut().post_mint(request)
    }

    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.mint.borrow_mut().post_melt_quote(request)
    }

    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.mint.borrow().get_melt_quote(quote_id)
    }

    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        self.mint.borrow_mut().post_melt(request)
    }

    fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        self.mint.borrow_mut().post_swap(request)
    }

    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        self.mint.borrow().post_check_state(request)
    }
}
