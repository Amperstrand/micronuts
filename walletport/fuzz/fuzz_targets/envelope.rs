#![no_main]
//! Fuzz the persistence envelope through the public API: a MemoryStore
//! pre-seeded with arbitrary bytes -> PersistentWallet::new must treat
//! any corrupt/foreign blob as a fresh wallet (CRC + version + seed
//! fingerprint checks) and never panic; balance/spend then touch the
//! decoded proofs and persist() exercises envelope re-encode.

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::persistent::PersistentWallet;
use cashu_core_lite::store::{MemoryStore, ProofStore};
use cashu_core_lite::transport::MintClient;
use libfuzzer_sys::fuzz_target;

struct NullTransport;

impl MintClient for NullTransport {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        Err(null())
    }
    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        Err(null())
    }
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        Err(null())
    }
    fn post_mint_quote(
        &mut self,
        _r: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(null())
    }
    fn get_mint_quote(&mut self, _q: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(null())
    }
    fn post_mint(&mut self, _r: nut04::MintRequest) -> Result<nut04::MintResponse, CashuError> {
        Err(null())
    }
    fn post_melt_quote(
        &mut self,
        _r: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(null())
    }
    fn get_melt_quote(&mut self, _q: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(null())
    }
    fn post_melt(&mut self, _r: nut05::MeltRequest) -> Result<nut05::MeltResponse, CashuError> {
        Err(null())
    }
    fn post_swap(&mut self, _r: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError> {
        Err(null())
    }
    fn post_check_state(
        &mut self,
        _r: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        Err(null())
    }
    fn post_restore(
        &mut self,
        _r: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        Err(null())
    }
}

fn null() -> CashuError {
    CashuError::Protocol(String::from("null transport"))
}

fuzz_target!(|data: &[u8]| {
    let mut store = MemoryStore::new();
    if store.save(data).is_err() {
        return;
    }
    if let Ok(mut w) = PersistentWallet::new("https://fuzz.mint.example", NullTransport, store, [7u8; 32])
    {
        let _ = w.balance();
        let _ = w.proof_count();
        let _ = w.spend(1);
    }
});
