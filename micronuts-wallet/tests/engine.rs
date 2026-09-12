//! WalletEngine integration tests against the real `DemoMint` (FakeWallet
//! auto-settlement, demo keyset, fee 0) behind a shareable RPC client.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut02, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::persistent::MeltOutcome;
use cashu_core_lite::rpc::RpcMintClient;
use cashu_core_lite::store::{MemoryStore, ProofStore};
use cashu_core_lite::token::{
    decode_token, encode_token_wire, Proof as TokenProof, TokenV4, TokenV4Token,
};
use cashu_core_lite::transport::MintClient;
use micronuts_mint::{DemoMint, LoopbackTransport};
use micronuts_wallet::engine::{HistoryKind, WalletEngine};

/// `MintClient` over an `Arc<Mutex<..>>`-shared demo-mint RPC client, so
/// the engine's two transport handles (metadata + wallet) hit one mint.
#[derive(Clone)]
struct SharedMintClient(Arc<Mutex<RpcMintClient<LoopbackTransport<DemoMint>>>>);

fn shared_mint() -> SharedMintClient {
    SharedMintClient(Arc::new(Mutex::new(RpcMintClient::new(
        LoopbackTransport::from_demo_mint(DemoMint::new()),
    ))))
}

impl MintClient for SharedMintClient {
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

fn engine(
    client: &SharedMintClient,
    seed: [u8; 32],
) -> WalletEngine<SharedMintClient, MemoryStore> {
    WalletEngine::new(
        "https://mint.example",
        client.clone(),
        MemoryStore::new(),
        seed,
        Vec::new(),
    )
    .unwrap()
}

const SEED: [u8; 32] = [0x42; 32];

/// 2*G — a valid curve point that is definitely not any mint signature.
const G2: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

fn point(hex_point: &str) -> cashu_core_lite::PublicKey {
    let bytes: [u8; 33] = hex::decode(hex_point).unwrap().try_into().unwrap();
    cashu_core_lite::PublicKey::from_bytes(&bytes).unwrap()
}

/// Wraps a shared mint client; when armed, `post_mint` responses carry a
/// valid-but-wrong `C_` point (forged signature) or lose their dleq.
#[derive(Clone)]
struct TamperMint {
    inner: SharedMintClient,
    forged_c: Rc<Cell<bool>>,
    strip_dleq: Rc<Cell<bool>>,
}

impl TamperMint {
    fn new(inner: SharedMintClient) -> Self {
        Self {
            inner,
            forged_c: Rc::new(Cell::new(false)),
            strip_dleq: Rc::new(Cell::new(false)),
        }
    }
}

impl MintClient for TamperMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        self.inner.get_info()
    }
    fn get_keys(&mut self) -> Result<cashu_core_lite::nuts::nut01::KeysResponse, CashuError> {
        self.inner.get_keys()
    }
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        self.inner.get_keysets()
    }
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.inner.post_mint_quote(request)
    }
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.inner.get_mint_quote(quote_id)
    }
    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        let mut response = self.inner.post_mint(request)?;
        if self.forged_c.get() {
            for sig in &mut response.signatures {
                sig.c = point(G2);
            }
        }
        if self.strip_dleq.get() {
            for sig in &mut response.signatures {
                sig.dleq = None;
            }
        }
        Ok(response)
    }
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.inner.post_melt_quote(request)
    }
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.inner.get_melt_quote(quote_id)
    }
    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        self.inner.post_melt(request)
    }
    fn post_swap(
        &mut self,
        request: cashu_core_lite::nuts::nut03::SwapRequest,
    ) -> Result<cashu_core_lite::nuts::nut03::SwapResponse, CashuError> {
        self.inner.post_swap(request)
    }
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        self.inner.post_check_state(request)
    }
    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        self.inner.post_restore(request)
    }
}

fn tamper_engine(client: &SharedMintClient) -> (WalletEngine<TamperMint, MemoryStore>, TamperMint) {
    let tamper = TamperMint::new(client.clone());
    let mut engine = WalletEngine::new(
        "https://mint.example",
        tamper.clone(),
        MemoryStore::new(),
        SEED,
        Vec::new(),
    )
    .unwrap();
    engine.connect().unwrap();
    (engine, tamper)
}

#[test]
fn forged_mint_signatures_are_rejected_with_rollback() {
    let client = shared_mint();
    let (mut wallet, tamper) = tamper_engine(&client);
    let quote = wallet.mint_via_invoice(21).unwrap();
    let quote = wallet.poll_mint_quote(&quote.quote).unwrap();
    tamper.forged_c.set(true);

    let err = wallet.mint_paid_quote(&quote.quote, 21).unwrap_err();
    assert!(matches!(err, CashuError::Crypto(_)), "{err:?}");
    assert_eq!(
        wallet.balance(),
        0,
        "unverifiable proofs must not stay in the wallet"
    );
}

#[test]
fn dleq_less_mint_signatures_are_rejected() {
    let client = shared_mint();
    let (mut wallet, tamper) = tamper_engine(&client);
    let quote = wallet.mint_via_invoice(21).unwrap();
    let quote = wallet.poll_mint_quote(&quote.quote).unwrap();
    tamper.strip_dleq.set(true);

    let err = wallet.mint_paid_quote(&quote.quote, 21).unwrap_err();
    assert!(matches!(err, CashuError::Crypto(_)), "{err:?}");
    assert_eq!(wallet.balance(), 0);
}

fn fund<S: ProofStore>(engine: &mut WalletEngine<SharedMintClient, S>, amount: u64) {
    let quote = engine.mint_via_invoice(amount).unwrap();
    let quote = engine.poll_mint_quote(&quote.quote).unwrap();
    assert_eq!(quote.state, "PAID", "FakeWallet settles on first poll");
    let minted = engine.mint_paid_quote(&quote.quote, amount).unwrap();
    assert_eq!(minted, amount);
}

#[test]
fn full_cycle_mint_send_receive_melt() {
    let client = shared_mint();
    let mut wallet = engine(&client, SEED);

    wallet.connect().unwrap();
    assert!(!wallet.mint_name().is_empty());
    assert_eq!(wallet.fee_ppk(), 0);

    fund(&mut wallet, 100);
    assert_eq!(wallet.balance(), 100);

    let token = wallet.send_token(21, Some("coffee")).unwrap();
    assert!(token.starts_with("cashuB"), "wire form: {token}");
    let decoded = decode_token(token.as_bytes()).unwrap();
    assert_eq!(decoded.total_amount(), 21);
    assert_eq!(decoded.memo.as_deref(), Some("coffee"));
    assert_eq!(wallet.balance(), 79, "send deducts exactly 21");

    let received = wallet.receive_token(&token).unwrap();
    assert_eq!(received, 21);
    assert_eq!(wallet.balance(), 100);

    let (quote, outcome) = wallet.melt("lnbcdemo30sat1micronuts").unwrap();
    assert_eq!(quote.amount, 30);
    assert_eq!(
        outcome,
        MeltOutcome {
            paid: true,
            preimage: outcome.preimage.clone(),
            change_sats: 0,
        }
    );
    assert_eq!(
        wallet.balance(),
        70,
        "exact-composition melt burns no overshoot"
    );

    let kinds: Vec<HistoryKind> = wallet.history().iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        vec![
            HistoryKind::Mint,
            HistoryKind::Send,
            HistoryKind::Receive,
            HistoryKind::Melt
        ]
    );
}

#[test]
fn send_more_than_balance_fails_without_state_change() {
    let client = shared_mint();
    let mut wallet = engine(&client, SEED);
    wallet.connect().unwrap();
    fund(&mut wallet, 100);

    let err = wallet.send_token(1000, None).unwrap_err();
    assert_eq!(err, CashuError::InsufficientInputs);
    assert_eq!(wallet.balance(), 100);
    assert_eq!(
        wallet.history().len(),
        1,
        "only the funding mint is recorded"
    );
}

#[test]
fn receive_foreign_mint_token_fails() {
    let client = shared_mint();
    let mut wallet = engine(&client, SEED);
    wallet.connect().unwrap();
    fund(&mut wallet, 100);

    let foreign = encode_token_wire(&TokenV4 {
        mint: String::from("https://other.example"),
        unit: String::from("sat"),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: String::from("00deadbeefdeadbe"),
            proofs: vec![TokenProof {
                amount: 5,
                keyset_id: String::from("00deadbeefdeadbe"),
                secret: String::from("aa"),
                c: vec![0x02; 33],
                dleq: None,
            }],
        }],
    })
    .unwrap();

    let err = wallet.receive_token(&foreign).unwrap_err();
    match err {
        CashuError::Protocol(detail) => assert!(detail.contains("different mint")),
        other => panic!("expected Protocol, got {other:?}"),
    }
    assert_eq!(wallet.balance(), 100);
}

#[test]
fn double_receive_of_spent_token_fails() {
    let client = shared_mint();
    let mut sender = engine(&client, SEED);
    sender.connect().unwrap();
    fund(&mut sender, 100);

    let token = sender.send_token(21, None).unwrap();

    let mut receiver = engine(&client, [0x99; 32]);
    receiver.connect().unwrap();
    let received = receiver.receive_token(&token).unwrap();
    assert_eq!(received, 21);
    assert_eq!(receiver.balance(), 21);

    let err = receiver.receive_token(&token).unwrap_err();
    assert_eq!(err, CashuError::TokensAlreadySpent);
    assert_eq!(receiver.balance(), 21, "failed receive must not credit");
}

#[test]
fn check_token_state_reports_spent_after_receive() {
    let client = shared_mint();
    let mut sender = engine(&client, SEED);
    sender.connect().unwrap();
    fund(&mut sender, 100);
    let token = sender.send_token(21, None).unwrap();

    let mut receiver = engine(&client, [0x77; 32]);
    receiver.connect().unwrap();
    let states = receiver.check_token_state(&token).unwrap();
    assert!(!states.is_empty());
    assert!(states.iter().all(|s| s.state == "UNSPENT"));

    receiver.receive_token(&token).unwrap();
    let states = receiver.check_token_state(&token).unwrap();
    assert!(
        states.iter().all(|s| s.state == "SPENT"),
        "mint must mark redeemed secrets spent"
    );
}

#[test]
fn history_and_seed_survive_engine_rebuild_over_same_store() {
    let client = shared_mint();

    // A shared medium models a reopenable store (file-backed in production).
    #[derive(Clone, Default)]
    struct SharedMemory(Arc<Mutex<MemoryStore>>);
    impl ProofStore for SharedMemory {
        fn load(&mut self) -> Result<Option<Vec<u8>>, cashu_core_lite::store::StoreError> {
            self.0.lock().unwrap().load()
        }
        fn save(&mut self, blob: &[u8]) -> Result<(), cashu_core_lite::store::StoreError> {
            self.0.lock().unwrap().save(blob)
        }
    }

    let medium = SharedMemory::default();
    let mut first = WalletEngine::new(
        "https://mint.example",
        client.clone(),
        medium.clone(),
        SEED,
        Vec::new(),
    )
    .unwrap();
    first.connect().unwrap();
    fund(&mut first, 100);

    let mut reopened = WalletEngine::new(
        "https://mint.example",
        client,
        medium,
        SEED,
        first.history().to_vec(),
    )
    .unwrap();
    reopened.connect().unwrap();
    assert_eq!(reopened.balance(), 100);
    assert_eq!(reopened.history().len(), 1);
    assert_eq!(reopened.seed_hex(), hex::encode(SEED));
}
