//! NUT-29: Batched Minting
//!
//! Batch quote state checks and multi-quote mint requests.
//!
//! Reference: https://github.com/cashubtc/nuts/blob/main/29.md

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::nut00::BlindedMessage;
use minicbor::{Decode, Encode};

/// Maximum quotes accepted in one batch request (the `max_batch_size`
/// advertised in the mint's NUT-06 info).
pub const MAX_BATCH_QUOTES: usize = 50;

/// Maximum outputs accepted in one batch mint request.
pub const MAX_BATCH_OUTPUTS: usize = 1000;

/// Request body for `POST /v1/mint/quote/{method}/check` (NUT-29).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
// NUT #29: {
// NUT #29:   "quotes": <Array[str]>
// NUT #29: }
pub struct BatchCheckMintQuoteRequest {
    /// Unique mint quote IDs to look up.
    #[n(0)]
    pub quotes: Vec<String>,
}

/// Request body for `POST /v1/mint/{method}/batch` (NUT-29).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
// NUT #29: {
// NUT #29:   "quotes": <Array[str]>,
// NUT #29:   "quote_amounts": <Array[int]|null>, // Optional
// NUT #29:   "outputs": <Array[BlindedMessage]>,
// NUT #29:   "signatures": <Array[string]|null> // Optional
// NUT #29: }
pub struct BatchMintRequest {
    /// Unique quote IDs to mint in one atomic operation.
    #[n(0)]
    pub quotes: Vec<String>,
    /// Expected amounts to mint per quote, in `quotes` order. Required for
    /// amount-demanding methods (bolt12); optional for bolt11.
    #[n(1)]
    pub quote_amounts: Option<Vec<u64>>,
    /// The consolidated set of blinded messages to sign.
    #[n(2)]
    pub outputs: Vec<BlindedMessage>,
    /// Per-quote NUT-20 signatures (`quotes[i]` ↔ `signatures[i]`, `None`
    /// entries for unlocked quotes). `None` overall when every quote is
    /// unlocked.
    #[n(3)]
    pub signatures: Option<Vec<Option<String>>>,
}
