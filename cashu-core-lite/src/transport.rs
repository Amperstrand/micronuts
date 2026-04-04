//! Transport-neutral mint client interface.
//!
//! The `MintClient` trait defines how a wallet talks to a mint without
//! specifying the transport mechanism (HTTP, USB, microfips, in-process, …).
//!
//! Implementations:
//!   - `DirectTransport` (in `micronuts-mint`): calls the mint core directly
//!   - Future: USB adapter wrapping the existing CDC protocol
//!   - Future: microfips/FIPS adapter for embedded transport

use crate::error::CashuError;
use crate::nuts::{nut01, nut02, nut03, nut04, nut05, nut06, nut07};

/// Transport trait for wallet → mint communication (client side).
///
/// Each method corresponds to a Cashu NUT API endpoint. The wallet calls
/// these methods; the transport implementation routes them to the mint.
pub trait MintClient {
    /// NUT-06: Get mint information.
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError>;

    /// NUT-01: Get the mint's active public keys.
    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError>;

    /// NUT-02: Get keyset metadata.
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError>;

    /// NUT-04: Request a mint quote for the given amount and unit.
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError>;

    /// NUT-04: Check the state of an existing mint quote.
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError>;

    /// NUT-04: Mint ecash by providing blinded outputs against a paid quote.
    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError>;

    /// NUT-05: Request a melt quote for paying a Lightning invoice.
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError>;

    /// NUT-05: Check the state of an existing melt quote.
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError>;

    /// NUT-05: Execute a melt (spend proofs to pay a Lightning invoice).
    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError>;

    /// NUT-03: Swap existing proofs for new blinded outputs.
    fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError>;

    /// NUT-07: Check the spent state of proofs (by their Y values).
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError>;
}
