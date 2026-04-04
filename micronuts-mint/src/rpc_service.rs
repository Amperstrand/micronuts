//! Mint-side RPC service adapter for the demo mint.
//!
//! This is the host-side service entrypoint that future serial or microfips
//! framing can call after decoding a request frame.

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut01, nut02, nut03, nut04, nut05, nut06, nut07};
use cashu_core_lite::rpc::MintService;

use crate::DemoMint;

impl MintService for DemoMint {
    /// NUT-06: forward mint info requests to the demo mint core.
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        DemoMint::get_info(self)
    }

    /// NUT-01: forward active key requests to the demo mint core.
    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        DemoMint::get_keys(self)
    }

    /// NUT-02: forward keyset metadata requests to the demo mint core.
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        DemoMint::get_keysets(self)
    }

    /// NUT-04: forward mint quote requests to the demo mint core.
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        DemoMint::post_mint_quote(self, request)
    }

    /// NUT-04: forward mint quote lookups to the demo mint core.
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        DemoMint::get_mint_quote(self, quote_id)
    }

    /// NUT-04: forward blind-sign mint requests to the demo mint core.
    fn post_mint(&mut self, request: nut04::MintRequest) -> Result<nut04::MintResponse, CashuError> {
        DemoMint::post_mint(self, request)
    }

    /// NUT-05: forward melt quote requests to the demo mint core.
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        DemoMint::post_melt_quote(self, request)
    }

    /// NUT-05: forward melt quote lookups to the demo mint core.
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        DemoMint::get_melt_quote(self, quote_id)
    }

    /// NUT-05: forward melt spend requests to the demo mint core.
    fn post_melt(&mut self, request: nut05::MeltRequest) -> Result<nut05::MeltResponse, CashuError> {
        DemoMint::post_melt(self, request)
    }

    /// NUT-03: forward swap requests to the demo mint core.
    fn post_swap(&mut self, request: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError> {
        DemoMint::post_swap(self, request)
    }

    /// NUT-07: forward state checks to the demo mint core.
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        DemoMint::post_check_state(self, request)
    }
}
