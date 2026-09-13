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
fn reconcile_prunes_proofs_spent_elsewhere() {
    let client = shared_mint();

    let mut original = engine(&client, SEED);
    original.connect().unwrap();
    fund(&mut original, 63);

    // A second device restores the same seed into its own store.
    let mut restored = engine(&client, SEED);
    restored.connect().unwrap();
    let count = restored.restore().unwrap();
    assert!(count > 0, "restore re-fetches the minted outputs");
    assert_eq!(restored.balance(), 63);

    // The original device spends everything — the mint marks those
    // secrets spent; the restored copy still counts them.
    let token = original.send_token(63, None).unwrap();
    assert_eq!(original.balance(), 0);
    let _ = token;

    let pruned = restored.reconcile_spent().unwrap();
    assert!(pruned > 0, "stale proofs must be pruned");
    assert_eq!(restored.balance(), 0);
    assert_eq!(restored.reconcile_spent().unwrap(), 0, "idempotent");
}

#[test]
fn reconcile_on_empty_wallet_is_free() {
    let client = shared_mint();
    let mut wallet = engine(&client, SEED);
    wallet.connect().unwrap();
    assert_eq!(wallet.reconcile_spent().unwrap(), 0);
    assert_eq!(wallet.balance(), 0);
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

#[test]
fn inspect_valid_token_reaches_review() {
    let client = shared_mint();
    let mut sender = engine(&client, [0x11; 32]);
    sender.connect().unwrap();
    fund(&mut sender, 63);
    let token = sender.send_token(21, Some("tea")).unwrap();

    let mut receiver = engine(&client, [0x22; 32]);
    receiver.connect().unwrap();
    let inspection = receiver.inspect_token(&token).unwrap();
    assert_eq!(inspection.summary.amount, 21);
    assert_eq!(inspection.summary.memo.as_deref(), Some("tea"));
    assert_eq!(
        inspection.summary.proof_count,
        inspection.summary.amount.count_ones() as usize
    );
    assert_eq!(inspection.spent, 0, "fresh token has no spent proofs");
    assert!(!inspection.all_spent());
    // Review line follows the UX contract: amount + mint, no jargon.
    let line = micronuts_wallet::flow::ReceiveEcashPhase::Review(inspection.clone()).user_line();
    assert!(line.contains("21 sats"), "{line}");
}

#[test]
fn inspect_after_receive_reports_already_spent() {
    let client = shared_mint();
    let mut sender = engine(&client, [0x31; 32]);
    sender.connect().unwrap();
    fund(&mut sender, 63);
    let token = sender.send_token(21, None).unwrap();

    let mut receiver = engine(&client, [0x32; 32]);
    receiver.connect().unwrap();
    receiver.receive_token(&token).unwrap();

    // The same token again: the mint reports every proof spent.
    let inspection = receiver.inspect_token(&token).unwrap();
    assert!(inspection.all_spent(), "{inspection:?}");
    let verdict = if inspection.all_spent() {
        micronuts_wallet::flow::FlowFailure::AlreadySpent
    } else {
        micronuts_wallet::flow::FlowFailure::Recoverable(String::new())
    };
    assert!(verdict.user_line().contains("already"));
}

#[test]
fn inspect_foreign_mint_token_is_classified() {
    let client = shared_mint();
    let mut sender = engine(&client, [0x41; 32]);
    sender.connect().unwrap();
    fund(&mut sender, 63);
    let token = sender.send_token(21, None).unwrap();

    // Re-address the token to a mint the receiver is not connected to.
    let mut decoded = decode_token(token.as_bytes()).unwrap();
    decoded.mint = String::from("https://elsewhere.example");
    let foreign = encode_token_wire(&decoded).unwrap();

    let mut receiver = engine(&client, [0x42; 32]);
    receiver.connect().unwrap();
    match receiver.inspect_token(&foreign) {
        Err(micronuts_wallet::flow::FlowFailure::ForeignMint { mint }) => {
            assert_eq!(mint, "https://elsewhere.example");
        }
        other => panic!("expected ForeignMint, got {other:?}"),
    }
}

#[test]
fn inspect_malformed_input_is_terminal() {
    let client = shared_mint();
    let mut wallet = engine(&client, [0x51; 32]);
    wallet.connect().unwrap();
    match wallet.inspect_token("cashuBnotarealtoken") {
        Err(micronuts_wallet::flow::FlowFailure::InvalidToken) => {}
        other => panic!("expected InvalidToken, got {other:?}"),
    }
}

// --- M4: ecash send lifecycle ---------------------------------------------

fn funded_sender(
    amount: u64,
) -> (
    SharedMintClient,
    WalletEngine<SharedMintClient, MemoryStore>,
) {
    let client = shared_mint();
    let mut wallet = engine(&client, [0x61; 32]);
    wallet.connect().unwrap();
    fund(&mut wallet, amount);
    (client, wallet)
}

#[test]
fn send_records_pending_not_complete() {
    let (_client, mut wallet) = funded_sender(100);
    let token = wallet.send_token(21, None).unwrap();

    assert_eq!(wallet.balance(), 79, "proofs leave at hand-over");
    assert_eq!(wallet.pending_sends().len(), 1);
    let pending = &wallet.pending_sends()[0];
    assert_eq!(pending.amount, 21);
    assert_eq!(pending.token, token);
    let entry = &wallet.history()[pending.history_idx];
    assert_eq!(entry.kind, HistoryKind::Send);
    assert_eq!(
        entry.status,
        micronuts_wallet::engine::HistoryStatus::Pending
    );
}

#[test]
fn pending_check_awaiting_then_claimed() {
    let (client, mut sender) = funded_sender(100);
    let token = sender.send_token(21, None).unwrap();

    // Before anyone redeems: awaiting.
    assert_eq!(
        sender.check_pending_send(0).unwrap(),
        micronuts_wallet::engine::PendingStatus::Awaiting
    );
    assert_eq!(sender.pending_sends().len(), 1, "awaiting stays pending");

    // A second wallet claims the token.
    let mut receiver = engine(&client, [0x62; 32]);
    receiver.connect().unwrap();
    assert_eq!(receiver.receive_token(&token).unwrap(), 21);

    // Now the sender's check sees SPENT and finalizes.
    assert_eq!(
        sender.check_pending_send(0).unwrap(),
        micronuts_wallet::engine::PendingStatus::Claimed
    );
    assert!(sender.pending_sends().is_empty(), "claimed drops pending");
    let entry = &sender.history()[0];
    assert_eq!(
        entry.status,
        micronuts_wallet::engine::HistoryStatus::Complete,
        "history stops lying after claim"
    );
}

#[test]
fn reclaim_restores_balance_and_kills_token() {
    let (client, mut sender) = funded_sender(100);
    let token = sender.send_token(21, None).unwrap();
    let send_idx = sender.pending_sends()[0].history_idx;

    let reclaimed = sender.reclaim_pending(0).unwrap();
    assert_eq!(reclaimed, 21, "zero-fee keyset reclaims exactly");
    assert_eq!(sender.balance(), 100, "balance restored");
    assert!(sender.pending_sends().is_empty(), "reclaim drops pending");
    assert_eq!(
        sender.history()[send_idx].status,
        micronuts_wallet::engine::HistoryStatus::Reclaimed
    );

    // The handed-out token is dead: its secrets were rotated at the mint.
    let mut receiver = engine(&client, [0x63; 32]);
    receiver.connect().unwrap();
    let inspection = receiver.inspect_token(&token).unwrap();
    assert!(
        inspection.all_spent(),
        "old token fully spent after reclaim"
    );
    assert!(receiver.receive_token(&token).is_err());
}

#[test]
fn reclaim_after_recipient_claims_fails_truthfully() {
    let (client, mut sender) = funded_sender(100);
    let token = sender.send_token(21, None).unwrap();

    let mut receiver = engine(&client, [0x64; 32]);
    receiver.connect().unwrap();
    receiver.receive_token(&token).unwrap();

    // Reclaim races a completed claim: must fail and finalize as claimed.
    let err = sender.reclaim_pending(0).unwrap_err();
    assert!(format!("{err:?}").contains("already claimed"));
    assert!(sender.pending_sends().is_empty());
    assert_eq!(
        sender.history()[0].status,
        micronuts_wallet::engine::HistoryStatus::Complete
    );
    assert_eq!(sender.balance(), 79, "no double credit");
}

#[test]
fn pending_sends_survive_restart() {
    use micronuts_wallet::state::FileStore;
    let client = shared_mint();
    let dir = std::env::temp_dir().join(format!(
        "micronuts-m4-restart-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let store_path = dir.join("proofs.bin");

    let mut wallet = WalletEngine::new(
        "https://mint.example",
        client.clone(),
        FileStore::new(&store_path).unwrap(),
        SEED,
        Vec::new(),
    )
    .unwrap();
    wallet.connect().unwrap();
    fund(&mut wallet, 100);
    let token = wallet.send_token(21, None).unwrap();
    let history = wallet.history().to_vec();
    let pending = wallet.pending_sends().to_vec();
    assert_eq!(pending.len(), 1);

    // "Restart": fresh engine over the SAME store + persisted state.
    let mut reopened = WalletEngine::with_pending(
        "https://mint.example",
        client,
        FileStore::new(&store_path).unwrap(),
        SEED,
        history,
        pending,
    )
    .unwrap();
    reopened.connect().unwrap();
    assert_eq!(reopened.pending_sends().len(), 1);
    assert_eq!(reopened.pending_sends()[0].token, token);
    assert_eq!(reopened.balance(), 79, "store survived the restart");
    // And the restarted wallet can still reclaim.
    assert_eq!(reopened.reclaim_pending(0).unwrap(), 21);
    assert_eq!(reopened.balance(), 100);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_all_pending_mixed_states() {
    let (client, mut sender) = funded_sender(200);
    let token_a = sender.send_token(21, None).unwrap();
    let _token_b = sender.send_token(32, None).unwrap();

    let mut receiver = engine(&client, [0x65; 32]);
    receiver.connect().unwrap();
    receiver.receive_token(&token_a).unwrap();

    let statuses = sender.check_all_pending().unwrap();
    assert_eq!(statuses.len(), 2);
    assert_eq!(
        statuses[0],
        micronuts_wallet::engine::PendingStatus::Claimed
    );
    assert_eq!(
        statuses[1],
        micronuts_wallet::engine::PendingStatus::Awaiting
    );
    assert_eq!(
        sender.pending_sends().len(),
        1,
        "only the awaiting one stays"
    );
    assert_eq!(sender.pending_sends()[0].amount, 32);
}
