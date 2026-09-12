//! Print a valid Cashu V4 token (against a fresh in-process DemoMint)
//! for use in the browser e2e suite — any such token is foreign to the
//! wallet's embedded demo mint, exercising the foreign-mint review path.

use std::sync::{Arc, Mutex};

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut02, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::rpc::RpcMintClient;
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::transport::MintClient;
use micronuts_mint::{DemoMint, LoopbackTransport};
use micronuts_wallet::engine::WalletEngine;

#[derive(Clone)]
struct SharedMint(Arc<Mutex<RpcMintClient<LoopbackTransport<DemoMint>>>>);

impl MintClient for SharedMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        self.0.lock().unwrap().get_info()
    }
    fn get_keys(&mut self) -> Result<cashu_core_lite::nuts::nut01::KeysResponse, CashuError> {
        self.0.lock().unwrap().get_keys()
    }
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        self.0.lock().unwrap().get_keysets()
    }
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.0.lock().unwrap().post_mint_quote(request)
    }
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.0.lock().unwrap().get_mint_quote(quote_id)
    }
    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        self.0.lock().unwrap().post_mint(request)
    }
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.0.lock().unwrap().post_melt_quote(request)
    }
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.0.lock().unwrap().get_melt_quote(quote_id)
    }
    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        self.0.lock().unwrap().post_melt(request)
    }
    fn post_swap(
        &mut self,
        request: cashu_core_lite::nuts::nut03::SwapRequest,
    ) -> Result<cashu_core_lite::nuts::nut03::SwapResponse, CashuError> {
        self.0.lock().unwrap().post_swap(request)
    }
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        self.0.lock().unwrap().post_check_state(request)
    }
    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        self.0.lock().unwrap().post_restore(request)
    }
}

fn main() {
    let client = SharedMint(Arc::new(Mutex::new(RpcMintClient::new(
        LoopbackTransport::from_demo_mint(DemoMint::new()),
    ))));
    let mut wallet = WalletEngine::new(
        "https://mint.example",
        client,
        MemoryStore::new(),
        [0x42; 32],
        Vec::new(),
    )
    .unwrap();
    wallet.connect().unwrap();
    let quote = wallet.mint_via_invoice(64).unwrap();
    let quote_id = quote.quote.clone();
    drop(quote);
    let amount = wallet.poll_mint_quote(&quote_id).unwrap().amount;
    wallet.mint_paid_quote(&quote_id, amount).unwrap();
    println!("{}", wallet.send_token(21, Some("e2e")).unwrap());
}
