//! PersistentWallet integration tests against an in-process mock mint.
//!
//! The mock signs every blinded output with a per-denomination key and
//! remembers each `B'` it ever signed — which is exactly the NUT-09
//! restore contract — so the crash/wipe scenarios below exercise the real
//! crypto path end to end (blind → sign → unblind → persist → reload).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use cashu_core_lite::crypto::sign_message;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::persistent::PersistentWallet;
use cashu_core_lite::store::{MemoryStore, ProofStore, StoreError};
use cashu_core_lite::transport::MintClient;

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";

/// In-process mint: per-denomination keys, remembers every signed B'.
/// `Clone` shares the signed-outputs map so a "restarted" wallet still
/// talks to the same mint state.
#[derive(Clone)]
struct MockMint {
    keys: nut01::KeySet,
    privkeys: BTreeMap<u64, SecretKey>,
    signed: Rc<RefCell<BTreeMap<[u8; 33], nut00::BlindSignature>>>,
    /// Test hook: when set, post_melt reports PENDING/unpaid.
    melt_pending: Rc<std::cell::Cell<bool>>,
}

impl MockMint {
    fn new() -> Self {
        let mut keys = Vec::new();
        let mut privkeys = BTreeMap::new();
        for exp in 0..7u32 {
            let amount = 1u64 << exp;
            let sk = SecretKey::from_slice(&[amount as u8; 32]).expect("valid scalar");
            privkeys.insert(amount, sk.clone());
            keys.push(nut01::KeyPair {
                amount,
                pubkey: sk.public_key(),
            });
        }
        Self {
            keys: nut01::KeySet {
                id: String::from(KEYSET_ID),
                unit: String::from("sat"),
                keys,
            },
            privkeys,
            signed: Rc::new(RefCell::new(BTreeMap::new())),
            melt_pending: Rc::new(std::cell::Cell::new(false)),
        }
    }

    fn sign_blanks_returning_change(
        &self,
        outputs: &[nut00::BlindedMessage],
        fee_used: u64,
    ) -> Vec<nut00::BlindSignature> {
        // Mint policy: return the unspent reserve as change, greedily in
        // the requested blank denominations (amounts the wallet chose).
        let mut remaining: u64 = outputs
            .iter()
            .map(|o| o.amount)
            .sum::<u64>()
            .saturating_sub(fee_used);
        let mut change = Vec::new();
        for out in outputs {
            if remaining == 0 {
                break;
            }
            let give = out.amount.min(remaining);
            if give == 0 {
                continue;
            }
            let privkey = match self.privkeys.get(&give) {
                Some(k) => k,
                None => continue,
            };
            change.push(nut00::BlindSignature {
                amount: give,
                id: out.id.clone(),
                c: sign_message(privkey, &out.b),
                dleq: None,
            });
            remaining -= give;
        }
        change
    }
}

fn unused<T>(_t: T) -> CashuError {
    CashuError::Protocol(String::from("endpoint not used by these tests"))
}

impl MintClient for MockMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        Err(unused(()))
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        Ok(nut01::KeysResponse {
            keysets: vec![self.keys.clone()],
        })
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        Err(unused(()))
    }

    fn post_mint_quote(
        &mut self,
        _request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn get_mint_quote(&mut self, _quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        let mut signatures = Vec::with_capacity(request.outputs.len());
        for output in &request.outputs {
            let privkey = self
                .privkeys
                .get(&output.amount)
                .ok_or(CashuError::KeysetNotFound)?;
            let c = sign_message(privkey, &output.b);
            let sig = nut00::BlindSignature {
                amount: output.amount,
                id: output.id.clone(),
                c,
                dleq: None,
            };
            self.signed
                .borrow_mut()
                .insert(output.b.to_bytes(), sig.clone());
            signatures.push(sig);
        }
        Ok(nut04::MintResponse { signatures })
    }

    fn post_melt_quote(
        &mut self,
        _request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn get_melt_quote(&mut self, _quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        if self.melt_pending.get() {
            return Ok(nut05::MeltResponse {
                paid: false,
                state: String::from("PENDING"),
                payment_preimage: None,
                change: None,
            });
        }
        let fee_used = 1u64;
        let change = request
            .outputs
            .as_deref()
            .map(|outs| self.sign_blanks_returning_change(outs, fee_used))
            .filter(|c| !c.is_empty());
        Ok(nut05::MeltResponse {
            paid: true,
            state: String::from("PAID"),
            payment_preimage: Some(String::from("00preimage00")),
            change,
        })
    }

    fn post_swap(
        &mut self,
        _request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        Err(unused(()))
    }

    fn post_check_state(
        &mut self,
        _request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        Err(unused(()))
    }

    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        let signed = self.signed.borrow();
        let outputs = request
            .outputs
            .iter()
            .filter_map(|b| {
                signed.get(&b.to_bytes()).map(|sig| nut09::RestoreOutput {
                    y: *b,
                    signature: sig.clone(),
                })
            })
            .collect();
        Ok(nut09::RestoreResponse { outputs })
    }
}

const SEED: [u8; 32] = [0x2a; 32];

/// Store handle shared across wallet sessions — models a reopenable medium
/// (flash/NVS/file): every wallet built from a clone sees the same bytes.
/// A bare `MemoryStore::clone()` snapshots at clone time, which is not the
/// persistence contract under test.
#[derive(Clone)]
struct SharedStore(Rc<RefCell<MemoryStore>>);

impl SharedStore {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(MemoryStore::new())))
    }
}

impl ProofStore for SharedStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.borrow_mut().load()
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        self.0.borrow_mut().save(blob)
    }
}

fn wallet_with(mock: &MockMint, store: SharedStore) -> PersistentWallet<MockMint, SharedStore> {
    PersistentWallet::new("https://mint.example", mock.clone(), store, SEED).unwrap()
}

/// A fresh shared medium pre-seeded with raw bytes (corruption cases).
fn medium_with(blob: &[u8]) -> SharedStore {
    let medium = SharedStore::new();
    medium.0.borrow_mut().save(blob).unwrap();
    medium
}

#[test]
fn mint_deterministic_persists_across_restart() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();
    let medium = SharedStore::new();

    let mut w = wallet_with(&mock, medium.clone());
    let minted = w
        .mint_deterministic("quote-1", 63, KEYSET_ID, &keyset)
        .unwrap();
    assert_eq!(minted, 63);
    assert_eq!(w.balance(), 63);
    assert_eq!(w.proof_count(), 6, "63 splits into 6 power-of-two proofs");

    let restarted = wallet_with(&mock, medium);
    assert_eq!(
        restarted.balance(),
        63,
        "proofs must survive restart via the store"
    );
    assert_eq!(restarted.proof_count(), 6);
}

#[test]
fn different_seed_is_a_different_wallet() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();
    let medium = SharedStore::new();

    let mut w = wallet_with(&mock, medium.clone());
    w.mint_deterministic("quote-1", 10, KEYSET_ID, &keyset)
        .unwrap();

    let other = PersistentWallet::new(
        "https://mint.example",
        mock.clone(),
        medium.clone(),
        [0x99; 32],
    )
    .unwrap();
    assert_eq!(
        other.balance(),
        0,
        "stored proofs belong to the seed that minted them (seed fingerprint mismatch)"
    );

    let same_seed = wallet_with(&mock, medium);
    assert_eq!(
        same_seed.balance(),
        10,
        "the original seed still loads its proofs"
    );
}

#[test]
fn corrupt_blob_starts_fresh_not_panicking() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();

    let medium = SharedStore::new();
    let mut w = wallet_with(&mock, medium.clone());
    w.mint_deterministic("quote-1", 21, KEYSET_ID, &keyset)
        .unwrap();
    let blob = medium.0.borrow_mut().load().unwrap().expect("blob stored");
    assert!(blob.len() > 16);

    let mut corrupted = blob.clone();
    corrupted[10] ^= 0xff;
    let w2 =
        PersistentWallet::new("https://mint.example", mock, medium_with(&corrupted), SEED).unwrap();
    assert_eq!(w2.balance(), 0, "CRC mismatch must read as a fresh wallet");

    let w3 = PersistentWallet::new(
        "https://mint.example",
        MockMint::new(),
        medium_with(&blob[..4]),
        SEED,
    )
    .unwrap();
    assert_eq!(
        w3.balance(),
        0,
        "truncated blob must read as a fresh wallet"
    );
}

#[test]
fn spend_removes_and_persists_undo_restores() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();

    let medium = SharedStore::new();
    let mut w = wallet_with(&mock, medium.clone());
    w.mint_deterministic("quote-1", 63, KEYSET_ID, &keyset)
        .unwrap();

    let selected = w.spend(40).unwrap();
    let selected_sum: u64 = selected.iter().map(|p| p.amount).sum();
    assert!(selected_sum >= 40, "selection must cover the amount");
    assert_eq!(selected_sum, 48, "largest-first picks 32+16 for 40");
    assert_eq!(w.balance(), 15, "spent proofs leave the wallet");

    let restarted = wallet_with(&mock, medium.clone());
    assert_eq!(restarted.balance(), 15, "spend must persist");

    w.undo_spend(selected).unwrap();
    assert_eq!(w.balance(), 63, "undo returns the proofs");
}

#[test]
fn spend_insufficient_funds_errors_without_state_change() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();

    let mut w = wallet_with(&mock, SharedStore::new());
    w.mint_deterministic("quote-1", 10, KEYSET_ID, &keyset)
        .unwrap();
    assert_eq!(w.spend(11), Err(CashuError::InsufficientInputs));
    assert_eq!(w.balance(), 10);
}

#[test]
fn restore_recovers_proofs_after_store_loss() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();

    // The original wallet mints with this seed; the mint remembers every B'.
    let mut original = wallet_with(&mock, SharedStore::new());
    original
        .mint_deterministic("quote-1", 63, KEYSET_ID, &keyset)
        .unwrap();

    // Its store is then lost entirely (power cycle + wiped blob).
    let mut lost = wallet_with(&mock, SharedStore::new());
    assert_eq!(lost.balance(), 0);

    // Same seed re-derives the outputs; restore re-fetches the signatures.
    let restored_count = lost.restore(KEYSET_ID, &keyset).unwrap();
    assert_eq!(
        restored_count, 6,
        "the 63-sat mint used 6 deterministic outputs"
    );
    assert_eq!(lost.balance(), 63);
}

#[test]
fn restore_is_idempotent() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();

    let mut original = wallet_with(&mock, SharedStore::new());
    original
        .mint_deterministic("quote-1", 63, KEYSET_ID, &keyset)
        .unwrap();

    let mut w = wallet_with(&mock, SharedStore::new());
    w.restore(KEYSET_ID, &keyset).unwrap();
    let again = w.restore(KEYSET_ID, &keyset).unwrap();
    assert_eq!(again, 0, "already-known secrets must not double-count");
    assert_eq!(w.balance(), 63);
}

#[test]
fn melt_consumes_inputs_and_reclaims_nut08_change() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();
    let medium = SharedStore::new();

    let mut w = wallet_with(&mock, medium.clone());
    w.mint_deterministic("q1", 63, KEYSET_ID, &keyset).unwrap();

    // Invoice 18 sats + 2 fee reserve: selection covers 20 with the 32.
    let outcome = w
        .melt_deterministic("melt-1", 18, 2, KEYSET_ID, &keyset)
        .unwrap();
    assert!(outcome.paid);
    assert_eq!(
        outcome.change_sats, 1,
        "2 reserve - 1 fee_used = 1 sat back"
    );
    assert!(outcome.preimage.is_some());
    // 63 - 32 (selected input) + 1 (change) = 32.
    assert_eq!(w.balance(), 32);
    assert!(w.proof_count() >= 1, "change proof is stored");

    let restarted = wallet_with(&mock, medium);
    assert_eq!(restarted.balance(), 32, "melt + change persist");
}

#[test]
fn pending_melt_preserves_funds() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();
    let medium = SharedStore::new();

    let mut w = wallet_with(&mock, medium.clone());
    w.mint_deterministic("q1", 32, KEYSET_ID, &keyset).unwrap();

    mock.melt_pending.set(true);
    let outcome = w
        .melt_deterministic("melt-1", 30, 2, KEYSET_ID, &keyset)
        .unwrap();
    assert!(!outcome.paid);
    assert_eq!(outcome.change_sats, 0);
    assert_eq!(w.balance(), 32, "pending melt must not consume inputs");
    assert_eq!(
        w.proof_count(),
        1,
        "32 is a single denomination — one proof, intact"
    );

    let restarted = wallet_with(&mock, medium);
    assert_eq!(restarted.balance(), 32);
}

#[test]
fn melt_insufficient_inputs_errors() {
    let mock = MockMint::new();
    let keyset = mock.keys.clone();
    let mut w = wallet_with(&mock, SharedStore::new());
    w.mint_deterministic("q1", 8, KEYSET_ID, &keyset).unwrap();
    assert_eq!(
        w.melt_deterministic("melt-1", 100, 2, KEYSET_ID, &keyset),
        Err(CashuError::InsufficientInputs)
    );
    assert_eq!(w.balance(), 8);
}
