//! End-to-end wallet lifecycle against a mint whose every signature is
//! produced — and self-verified — by **upstream `cashu` (CDK) 0.17.3
//! primitives**. The wallet side is our `PersistentWallet`; the mint side
//! signs with `cashu::dhke::sign_message`, computes NUT-12 DLEQ with CDK's
//! own `hash_e` construction, and every proof the wallet ends up storing
//! is re-verified with `cashu::dhke::verify_message` before the test
//! asserts anything.
//!
//! This extends the interop guarantee from single operations (gate
//! verification in walletport) to the *full wallet pipeline*: NUT-13
//! secret derivation → blinding → upstream signing → unblinding →
//! persistence → melt with NUT-08 change → NUT-09 restore after total
//! store loss.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cashu::dhke::{hash_e, sign_message, verify_message};
use cashu::util::SECP256K1;
use cashu::{PublicKey as CdkPublicKey, SecretKey as CdkSecretKey};

use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::persistent::PersistentWallet;
use cashu_core_lite::store::{MemoryStore, ProofStore, StoreError};
use cashu_core_lite::transport::MintClient;

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://cdk-mint-e2e.example";
const SEED: [u8; 32] = [0x5a; 32];

// ---- conversions ----

fn to_cdk(p: &PublicKey) -> CdkPublicKey {
    CdkPublicKey::from_slice(&p.to_bytes()).expect("valid point")
}

fn to_ccl(p: &CdkPublicKey) -> PublicKey {
    PublicKey::from_sec1_bytes(&p.serialize()).expect("valid point")
}

fn ccl_sk(k: &CdkSecretKey) -> SecretKey {
    SecretKey::from_slice(&k.to_secret_bytes()).expect("valid scalar")
}

/// CDK's calculate_dleq from public primitives (mirrors the private fn in
/// cashu::nuts::nut12): nonce k, R1 = kG, R2 = kB', e = hash_e, s = k+e·a.
fn cdk_dleq(
    a: &CdkSecretKey,
    b_prime: &CdkPublicKey,
    c_prime: &CdkPublicKey,
) -> (CdkSecretKey, CdkSecretKey) {
    let k = CdkSecretKey::from_slice(&[13u8; 32]).unwrap();
    let r1 = k.public_key();
    let r2: CdkPublicKey = b_prime
        .mul_tweak(&SECP256K1, &k.as_scalar())
        .unwrap()
        .into();
    let e_bytes = hash_e([r1, r2, a.public_key(), *c_prime]);
    let e = CdkSecretKey::from_slice(&e_bytes).unwrap();
    let s1: CdkSecretKey = e.mul_tweak(&a.as_scalar()).unwrap().into();
    let s: CdkSecretKey = k.add_tweak(&s1.as_scalar()).unwrap().into();
    (e, s)
}

/// The mint: per-denomination upstream keys; remembers signed B' for
/// restore and spent Ys for NUT-07. Clone shares state (same mint).
#[derive(Clone)]
struct CdkMint {
    privkeys: BTreeMap<u64, CdkSecretKey>,
    signed: Rc<RefCell<BTreeMap<[u8; 33], nut00::BlindSignature>>>,
    spent_y: Rc<RefCell<BTreeSet<[u8; 33]>>>,
    melt_pending: Rc<Cell<bool>>,
    fee_used: u64,
}

impl CdkMint {
    fn new() -> Self {
        let privkeys = (0..7u32)
            .map(|exp| {
                let amount = 1u64 << exp;
                (
                    amount,
                    CdkSecretKey::from_slice(&[amount as u8; 32]).unwrap(),
                )
            })
            .collect();
        Self {
            privkeys,
            signed: Rc::new(RefCell::new(BTreeMap::new())),
            spent_y: Rc::new(RefCell::new(BTreeSet::new())),
            melt_pending: Rc::new(Cell::new(false)),
            fee_used: 1,
        }
    }

    fn keyset(&self) -> nut01::KeySet {
        let keys = self
            .privkeys
            .iter()
            .map(|(amount, sk)| nut01::KeyPair {
                amount: *amount,
                pubkey: to_ccl(&sk.public_key()),
            })
            .collect();
        nut01::KeySet {
            id: KEYSET_ID.to_string(),
            unit: "sat".to_string(),
            keys,
        }
    }

    fn sign_output(&self, output: &nut00::BlindedMessage) -> Option<nut00::BlindSignature> {
        let a = self.privkeys.get(&output.amount)?;
        let b_prime = to_cdk(&output.b);
        let c_prime = sign_message(a, &b_prime).expect("upstream sign");
        let (e, s) = cdk_dleq(a, &b_prime, &c_prime);
        let sig = nut00::BlindSignature {
            amount: output.amount,
            id: output.id.clone(),
            c: to_ccl(&c_prime),
            dleq: Some(cashu_core_lite::nuts::nut12::BlindSignatureDleq {
                e: ccl_sk(&e),
                s: ccl_sk(&s),
            }),
        };
        self.signed
            .borrow_mut()
            .insert(output.b.to_bytes(), sig.clone());
        Some(sig)
    }

    fn mint_key(&self, amount: u64) -> &CdkSecretKey {
        self.privkeys.get(&amount).expect("denomination key")
    }
}

fn unused<T>(_: T) -> CashuError {
    CashuError::Protocol(String::from("endpoint not used by these tests"))
}

impl MintClient for CdkMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        Err(unused(()))
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        Ok(nut01::KeysResponse {
            keysets: vec![self.keyset()],
        })
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        Err(unused(()))
    }

    fn post_mint_quote(
        &mut self,
        _: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn get_mint_quote(&mut self, _: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        let signatures = request
            .outputs
            .iter()
            .map(|o| self.sign_output(o).ok_or(CashuError::InvalidAmount))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nut04::MintResponse { signatures })
    }

    fn post_melt_quote(
        &mut self,
        _: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(unused(()))
    }

    fn get_melt_quote(&mut self, _: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
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
                quote: request.quote.clone(),
                amount: 0,
                fee_reserve: 0,
                unit: "sat".to_string(),
                expiry: 0,
                request: "lnbcdemo10sat1micronuts".to_string(),
                method: "bolt11".to_string(),
            });
        }
        for proof in &request.inputs {
            // Y = hash_to_curve(secret) via upstream — mint marks inputs spent.
            let y = cashu::dhke::hash_to_curve(proof.secret.as_bytes()).expect("hash_to_curve");
            self.spent_y.borrow_mut().insert(y.serialize());
        }
        let fee_used = self.fee_used;
        let change = request.outputs.as_deref().map(|outs| {
            let mut remaining: u64 = outs
                .iter()
                .map(|o| o.amount)
                .sum::<u64>()
                .saturating_sub(fee_used);
            let mut change = Vec::new();
            for out in outs {
                if remaining == 0 {
                    break;
                }
                let give = out.amount.min(remaining);
                if give == 0 {
                    continue;
                }
                let mut m = out.clone();
                m.amount = give;
                if let Some(sig) = self.sign_output(&m) {
                    change.push(sig);
                    remaining -= give;
                }
            }
            change
        });
        Ok(nut05::MeltResponse {
            paid: true,
            state: String::from("PAID"),
            payment_preimage: Some(String::from("e2epreimage0001")),
            change,
            quote: request.quote.clone(),
            amount: 0,
            fee_reserve: 0,
            unit: "sat".to_string(),
            expiry: 0,
            request: "lnbcdemo10sat1micronuts".to_string(),
            method: "bolt11".to_string(),
        })
    }

    fn post_swap(&mut self, _: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError> {
        Err(unused(()))
    }

    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        let spent = self.spent_y.borrow();
        Ok(nut07::CheckStateResponse {
            states: request
                .ys
                .iter()
                .map(|y| nut07::ProofState {
                    y: *y,
                    state: if spent.contains(&y.to_bytes()) {
                        String::from("SPENT")
                    } else {
                        String::from("UNSPENT")
                    },
                    witness: None,
                })
                .collect(),
        })
    }

    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        let signed = self.signed.borrow();
        Ok(nut09::RestoreResponse {
            outputs: request
                .outputs
                .iter()
                .filter_map(|b| {
                    signed.get(&b.to_bytes()).map(|sig| nut09::RestoreOutput {
                        y: *b,
                        signature: sig.clone(),
                    })
                })
                .collect(),
        })
    }
}

// Shared reopenable medium (same pattern as persistent_wallet.rs).
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

fn wallet(mint: &CdkMint, store: SharedStore) -> PersistentWallet<CdkMint, SharedStore> {
    PersistentWallet::new(MINT, mint.clone(), store, SEED).unwrap()
}

#[test]
fn mint_lifecycle_upstream_verified() {
    let mint = CdkMint::new();
    let keyset = mint.keyset();
    let mut w = wallet(&mint, SharedStore::new());

    let minted = w.mint_deterministic("q1", 63, KEYSET_ID, &keyset).unwrap();
    assert_eq!(minted, 63);
    assert_eq!(w.balance(), 63);

    // Extract all proofs via spend, then check each with upstream CDK.
    let proofs = w.spend(63).unwrap();
    assert_eq!(proofs.len(), 6);
    for proof in &proofs {
        assert!(proof.dleq.is_some(), "mint attaches NUT-12 to every proof");
        let c_cdk = CdkPublicKey::from_slice(&proof.c.to_bytes()).unwrap();
        verify_message(mint.mint_key(proof.amount), c_cdk, proof.secret.as_bytes())
            .expect("upstream CDK must verify every wallet-stored proof");
    }
    assert_eq!(w.balance(), 0, "spend removed all");
}

#[test]
fn melt_change_is_upstream_verified() {
    let mint = CdkMint::new();
    let keyset = mint.keyset();
    let medium = SharedStore::new();

    let mut w = wallet(&mint, medium.clone());
    w.mint_deterministic("q1", 32, KEYSET_ID, &keyset).unwrap();

    let outcome = w
        .melt_deterministic("m1", 30, 2, KEYSET_ID, &keyset)
        .unwrap();
    assert!(outcome.paid);
    assert_eq!(outcome.change_sats, 1, "2 sat reserve - 1 sat fee");

    // The 1-sat change proof must itself verify upstream.
    let change = w.spend(1).unwrap();
    assert_eq!(change.len(), 1);
    let c_cdk = CdkPublicKey::from_slice(&change[0].c.to_bytes()).unwrap();
    verify_message(mint.mint_key(1), c_cdk, change[0].secret.as_bytes())
        .expect("NUT-08 change proof must be upstream-verifiable");

    assert_eq!(wallet(&mint, medium).balance(), 0);
}

#[test]
fn restore_after_total_store_loss_upstream_verified() {
    let mint = CdkMint::new();
    let keyset = mint.keyset();

    // Wallet A mints; its store is then lost entirely.
    let mut a = wallet(&mint, SharedStore::new());
    a.mint_deterministic("q1", 63, KEYSET_ID, &keyset).unwrap();

    // Wallet B: same seed, blank medium, restore via NUT-09.
    let mut b = wallet(&mint, SharedStore::new());
    let restored = b.restore(KEYSET_ID, &keyset).unwrap();
    assert_eq!(restored, 6);
    assert_eq!(b.balance(), 63);

    let proofs = b.spend(63).unwrap();
    for proof in &proofs {
        let c_cdk = CdkPublicKey::from_slice(&proof.c.to_bytes()).unwrap();
        verify_message(mint.mint_key(proof.amount), c_cdk, proof.secret.as_bytes())
            .expect("restored proofs must be upstream-verifiable too");
    }
}

#[test]
fn check_state_reports_melted_inputs_spent() {
    let mint = CdkMint::new();
    let keyset = mint.keyset();
    let mut w = wallet(&mint, SharedStore::new());

    w.mint_deterministic("q1", 8, KEYSET_ID, &keyset).unwrap();
    // Capture a secret before melting it.
    let proofs = w.spend(8).unwrap();
    let secret_hex = proofs[0].secret.clone();
    w.undo_spend(proofs).unwrap();

    w.melt_deterministic("m1", 7, 1, KEYSET_ID, &keyset)
        .unwrap();

    // Y computed by OUR hash_to_curve must match the upstream-Y the mint
    // recorded when spending — cross-implementation agreement at NUT-07.
    let y = cashu_core_lite::hash_to_curve(secret_hex.as_bytes()).unwrap();
    let mut m = mint.clone();
    use cashu_core_lite::transport::MintClient as _;
    let resp = m
        .post_check_state(nut07::CheckStateRequest { ys: vec![y] })
        .unwrap();
    assert_eq!(
        resp.states[0].state, "SPENT",
        "melted input must read SPENT"
    );
}
