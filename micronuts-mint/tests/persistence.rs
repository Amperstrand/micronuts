//! Restart harness for durable mint state (docs/PERSISTENCE-DESIGN.md).
//!
//! Each test drives the mint through real lifecycle operations against a
//! state file, drops the mint (a crash — there is no shutdown hook, which
//! is the point: snapshot-per-mutation means any completed mutation is
//! already durable), reconstructs from the same file, and asserts the
//! invariants that make restarts safe:
//!   - spent proofs stay spent (no re-mint / double-spend window)
//!   - quote states and accounting survive (no free re-mint of ISSUED
//!     quotes, no lost PAID state)
//!   - the NUT-09 restore index survives
//!   - melt rollback (proof release + FAILED quote) survives
//!   - a corrupt state file refuses to boot

use cashu_core_lite::crypto::{blind_message, hash_to_curve, unblind_signature};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::{nut00, nut01, nut03, nut04, nut05, nut07, nut09};
use micronuts_mint::ln::{LightningBackend, MintClock};
use micronuts_mint::persist::{MintStateSnapshot, SnapshotFile, StateStore};
use micronuts_mint::DemoMint;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Backend whose payments always fail — drives the melt rollback path.
struct FailingWallet;

impl LightningBackend for FailingWallet {
    fn create_invoice(
        &mut self,
        amount_sat: u64,
        _description: &str,
    ) -> Result<String, CashuError> {
        Ok(format!("lnbcdemo{amount_sat}sat1micronuts"))
    }

    fn is_settled(&mut self, _invoice: &str) -> Result<bool, CashuError> {
        Ok(true)
    }

    fn lookup_amount(&mut self, invoice: &str) -> Result<u64, CashuError> {
        micronuts_mint::ln::parse_demo_invoice_amount(invoice)
            .ok_or_else(|| CashuError::Protocol("invalid demo invoice amount".to_string()))
    }

    fn pay_invoice(&mut self, _invoice: &str, _amount_sat: u64) -> Result<String, CashuError> {
        Err(CashuError::PaymentFailed)
    }
}

struct FrozenClock;

impl MintClock for FrozenClock {
    fn now_secs(&self) -> u64 {
        1_000_000
    }
}

struct PendingOutput {
    blinder: SecretKey,
    secret: String,
    b: PublicKey,
}

fn blind_amounts(
    amounts: &[u64],
    keyset_id: &str,
    rng: &mut StdRng,
) -> Result<Vec<(nut00::BlindedMessage, PendingOutput)>, CashuError> {
    let mut out = Vec::with_capacity(amounts.len());
    for &amount in amounts {
        let mut secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut secret_bytes);
        let secret_hex = hex::encode(secret_bytes);
        let mut blinder_bytes = [0u8; 32];
        rng.fill_bytes(&mut blinder_bytes);
        let blinder = SecretKey::from_slice(&blinder_bytes)
            .map_err(|_| CashuError::Crypto("bad blinder scalar".into()))?;
        let blinded = blind_message(secret_hex.as_bytes(), Some(blinder.clone()))
            .map_err(|_| CashuError::Crypto("blind_message failed".into()))?;
        out.push((
            nut00::BlindedMessage {
                amount,
                id: keyset_id.to_string(),
                b: blinded.blinded,
            },
            PendingOutput {
                blinder,
                secret: secret_hex,
                b: blinded.blinded,
            },
        ));
    }
    Ok(out)
}

fn unblind_proofs(
    pending: &[PendingOutput],
    signatures: &[nut00::BlindSignature],
    keyset: &nut01::KeySet,
) -> Result<Vec<nut00::Proof>, CashuError> {
    pending
        .iter()
        .zip(signatures.iter())
        .map(|(p, sig)| {
            let mint_pubkey = keyset
                .keys
                .iter()
                .find(|kp| kp.amount == sig.amount)
                .map(|kp| &kp.pubkey)
                .ok_or(CashuError::KeysetNotFound)?;
            let c = unblind_signature(&sig.c, &p.blinder, mint_pubkey)
                .map_err(|_| CashuError::Crypto("unblind failed".into()))?;
            Ok(nut00::Proof {
                amount: sig.amount,
                id: sig.id.clone(),
                secret: p.secret.clone(),
                c,
                dleq: None,
            })
        })
        .collect()
}

/// Quote → settle → mint; returns (quote id, proofs, pending outputs —
/// the B' points are needed for NUT-09 restore assertions).
fn mint_with_pending(
    mint: &mut DemoMint,
    sats: u64,
    keyset: &nut01::KeySet,
    rng: &mut StdRng,
) -> Result<(String, Vec<nut00::Proof>, Vec<PendingOutput>), CashuError> {
    let quote = mint.post_mint_quote(nut04::MintQuoteRequest {
        amount: sats,
        unit: "sat".to_string(),
    })?;
    let settled = mint.get_mint_quote(&quote.quote)?;
    assert_eq!(settled.state, "PAID", "FakeWallet settles on first poll");

    let pairs = blind_amounts(&nut00::decompose_amount(sats), mint.keyset_id(), rng)?;
    let (messages, pending): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let response = mint.post_mint(nut04::MintRequest {
        quote: quote.quote.clone(),
        outputs: messages,
    })?;
    let proofs = unblind_proofs(&pending, &response.signatures, keyset)?;
    Ok((quote.quote, proofs, pending))
}

fn y_of(secret: &str) -> PublicKey {
    hash_to_curve(secret.as_bytes()).expect("hash_to_curve of a mint secret")
}

fn temp_state_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "micronuts-restart-{label}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn restart_preserves_spent_set_and_rejects_respend() {
    let path = temp_state_path("spent");
    let mut rng = StdRng::seed_from_u64(0xA1);

    let saved_proofs;
    {
        let mut mint = DemoMint::new().with_state_file(&path);
        let keyset = mint.public_keyset();
        let (_, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        saved_proofs = proofs.clone();

        // Spend via swap (8 in → 8 out on the zero-fee keyset).
        let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
        let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        mint.post_swap(nut03::SwapRequest {
            inputs: proofs,
            outputs: messages,
        })
        .expect("first swap spends the proofs");
    } // drop = crash after the mutation

    let mut mint = DemoMint::new().with_state_file(&path);
    let ys: Vec<PublicKey> = saved_proofs.iter().map(|p| y_of(&p.secret)).collect();
    let states = mint
        .post_check_state(nut07::CheckStateRequest { ys })
        .unwrap();
    assert!(
        states.states.iter().all(|s| s.state == nut07::state::SPENT),
        "all restarted proofs report SPENT"
    );

    // Re-presenting the same proofs is rejected on the restarted mint.
    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let err = mint
        .post_swap(nut03::SwapRequest {
            inputs: saved_proofs,
            outputs: messages,
        })
        .expect_err("respend of spent proofs must fail");
    assert!(
        matches!(err, CashuError::TokensAlreadySpent),
        "expected TokensAlreadySpent, got {err:?}"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn restart_preserves_quote_state_and_accounting() {
    let path = temp_state_path("quote");
    let mut rng = StdRng::seed_from_u64(0xB2);
    let quote_id;

    {
        let mut mint = DemoMint::new().with_state_file(&path);
        let keyset = mint.public_keyset();
        let (minted_quote, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        quote_id = minted_quote;
        assert_eq!(
            proofs.iter().map(|p| p.amount).sum::<u64>(),
            8,
            "minted 8 sats before restart"
        );
    }

    let mut mint = DemoMint::new().with_state_file(&path);
    let after = mint.get_mint_quote(&quote_id).unwrap();
    assert_eq!(after.state, "ISSUED", "quote state survives restart");
    assert_eq!(after.amount_paid, 8);
    assert_eq!(after.amount_issued, 8);

    // Re-minting a fully issued quote is rejected after restart.
    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let err = mint
        .post_mint(nut04::MintRequest {
            quote: quote_id,
            outputs: messages,
        })
        .expect_err("re-mint of ISSUED quote must fail");
    assert!(matches!(err, CashuError::QuoteAlreadyIssued), "{err:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn restart_preserves_restore_index() {
    let path = temp_state_path("restore");
    let mut rng = StdRng::seed_from_u64(0xC3);
    let pending_out;

    {
        let mut mint = DemoMint::new().with_state_file(&path);
        let keyset = mint.public_keyset();
        let (_, _, pending) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        pending_out = pending;
    }

    let mint = DemoMint::new().with_state_file(&path);
    let bs: Vec<PublicKey> = pending_out.iter().map(|p| p.b).collect();
    let restored = mint
        .post_restore(nut09::RestoreRequest { outputs: bs })
        .unwrap();
    assert_eq!(
        restored.outputs.len(),
        pending_out.len(),
        "all issued signatures restorable after restart"
    );
    assert_eq!(
        restored
            .outputs
            .iter()
            .map(|o| o.signature.amount)
            .sum::<u64>(),
        8
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn melt_rollback_survives_restart() {
    let path = temp_state_path("rollback");
    let mut rng = StdRng::seed_from_u64(0xD4);
    let saved_proofs;
    let melt_quote_id;

    {
        let mut mint = DemoMint::with_backend(Box::new(FailingWallet), Box::new(FrozenClock), 0)
            .with_state_file(&path);
        let keyset = mint.public_keyset();
        let (_, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        saved_proofs = proofs.clone();

        let melt_quote = mint
            .post_melt_quote(nut05::MeltQuoteRequest {
                request: "lnbcdemo3sat1micronuts".to_string(),
                unit: "sat".to_string(),
            })
            .unwrap();
        melt_quote_id = melt_quote.quote.clone();

        let err = mint
            .post_melt(nut05::MeltRequest {
                quote: melt_quote.quote,
                inputs: proofs,
                outputs: None,
            })
            .expect_err("FailingWallet payment must fail");
        assert!(matches!(err, CashuError::PaymentFailed), "{err:?}");
    } // crash after rollback was persisted

    let mut mint = DemoMint::with_backend(Box::new(FailingWallet), Box::new(FrozenClock), 0)
        .with_state_file(&path);

    // Rollback survived: the proofs are spendable again.
    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    mint.post_swap(nut03::SwapRequest {
        inputs: saved_proofs,
        outputs: messages,
    })
    .expect("released proofs are spendable after restart");

    // And the failed melt quote stays terminal.
    let err = mint
        .post_melt(nut05::MeltRequest {
            quote: melt_quote_id,
            inputs: Vec::new(),
            outputs: None,
        })
        .expect_err("FAILED quote must stay terminal");
    assert!(matches!(err, CashuError::PaymentFailed), "{err:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
#[should_panic(expected = "refusing to start")]
fn corrupt_state_file_refuses_boot() {
    let path = temp_state_path("corrupt");
    std::fs::write(&path, b"{ this is not a snapshot").unwrap();
    let _ = DemoMint::new().with_state_file(&path);
    std::fs::remove_file(&path).ok();
}

#[test]
fn state_file_created_eagerly_and_valid_json() {
    let path = temp_state_path("eager");
    {
        let _mint = DemoMint::new().with_state_file(&path);
        assert!(path.exists(), "state file created at boot, not lazily");
    }
    let raw = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("snapshot is valid JSON");
    assert!(parsed.get("spent_ys").is_some(), "snapshot shape sanity");
    std::fs::remove_file(&path).ok();
}

// ---- StateStore seam: restart matrix + per-backend semantics ----

/// Shared cell backing for [`InMemoryStateStore`] — one durable "device"
/// that every store instance built from it sees.
type SharedCells = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Test-only in-memory [`StateStore`] modeling ESP32 NVS semantics: the
/// snapshot is serialized to one blob stored under a single key; `save`
/// is an atomic per-key replace (a reader sees the old or the new blob,
/// never a torn one), and an optional size bound rejects oversize
/// blobs instead of silently truncating.
struct InMemoryStateStore {
    cells: SharedCells,
    max_blob: Option<usize>,
}

impl InMemoryStateStore {
    const KEY: &'static str = "mint_state";

    fn with_shared(cells: SharedCells, max_blob: Option<usize>) -> Self {
        Self { cells, max_blob }
    }
}

impl StateStore for InMemoryStateStore {
    fn load(&self) -> Result<Option<MintStateSnapshot>, String> {
        match self.cells.lock().expect("cells mutex").get(Self::KEY) {
            None => Ok(None),
            Some(blob) => serde_json::from_slice(blob)
                .map(Some)
                .map_err(|e| format!("state {}: corrupt: {e}", Self::KEY)),
        }
    }

    fn save(&self, snap: &MintStateSnapshot) -> Result<(), String> {
        let blob = serde_json::to_vec(snap).map_err(|e| format!("serialize failed: {e}"))?;
        if let Some(max) = self.max_blob {
            if blob.len() > max {
                return Err(format!(
                    "state {}: {} bytes exceeds bound {max} bytes",
                    Self::KEY,
                    blob.len()
                ));
            }
        }
        self.cells
            .lock()
            .expect("cells mutex")
            .insert(Self::KEY.to_string(), blob);
        Ok(())
    }
}

/// Matrix leg — spent-stays-spent through `make_store`'s backend: after a
/// restart, spent proofs still report SPENT and re-spending them fails
/// with `TokensAlreadySpent`.
fn matrix_spent_stays_spent(make_store: &dyn Fn() -> Box<dyn StateStore>) {
    let mut rng = StdRng::seed_from_u64(0xA1);
    let saved_proofs;
    {
        let mut mint = DemoMint::new().with_state_store(make_store());
        let keyset = mint.public_keyset();
        let (_, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        saved_proofs = proofs.clone();

        let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
        let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        mint.post_swap(nut03::SwapRequest {
            inputs: proofs,
            outputs: messages,
        })
        .expect("first swap spends the proofs");
    } // drop = crash after the mutation

    let mut mint = DemoMint::new().with_state_store(make_store());
    let ys: Vec<PublicKey> = saved_proofs.iter().map(|p| y_of(&p.secret)).collect();
    let states = mint
        .post_check_state(nut07::CheckStateRequest { ys })
        .unwrap();
    assert!(
        states.states.iter().all(|s| s.state == nut07::state::SPENT),
        "all restarted proofs report SPENT"
    );

    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let err = mint
        .post_swap(nut03::SwapRequest {
            inputs: saved_proofs,
            outputs: messages,
        })
        .expect_err("respend of spent proofs must fail");
    assert!(
        matches!(err, CashuError::TokensAlreadySpent),
        "expected TokensAlreadySpent, got {err:?}"
    );
}

/// Matrix leg — ISSUED quote accounting survives a restart through
/// `make_store`'s backend (no free re-mint of issued quotes).
fn matrix_issued_quote_accounting_survives(make_store: &dyn Fn() -> Box<dyn StateStore>) {
    let mut rng = StdRng::seed_from_u64(0xB2);
    let quote_id;

    {
        let mut mint = DemoMint::new().with_state_store(make_store());
        let keyset = mint.public_keyset();
        let (minted_quote, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        quote_id = minted_quote;
        assert_eq!(
            proofs.iter().map(|p| p.amount).sum::<u64>(),
            8,
            "minted 8 sats before restart"
        );
    }

    let mut mint = DemoMint::new().with_state_store(make_store());
    let after = mint.get_mint_quote(&quote_id).unwrap();
    assert_eq!(after.state, "ISSUED", "quote state survives restart");
    assert_eq!(after.amount_paid, 8);
    assert_eq!(after.amount_issued, 8);

    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let err = mint
        .post_mint(nut04::MintRequest {
            quote: quote_id,
            outputs: messages,
        })
        .expect_err("re-mint of ISSUED quote must fail");
    assert!(matches!(err, CashuError::QuoteAlreadyIssued), "{err:?}");
}

/// Matrix leg — the NUT-09 restore index restores every issued signature
/// after a restart through `make_store`'s backend.
fn matrix_restore_index_restores(make_store: &dyn Fn() -> Box<dyn StateStore>) {
    let mut rng = StdRng::seed_from_u64(0xC3);
    let pending_out;

    {
        let mut mint = DemoMint::new().with_state_store(make_store());
        let keyset = mint.public_keyset();
        let (_, _, pending) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        pending_out = pending;
    }

    let mint = DemoMint::new().with_state_store(make_store());
    let bs: Vec<PublicKey> = pending_out.iter().map(|p| p.b).collect();
    let restored = mint
        .post_restore(nut09::RestoreRequest { outputs: bs })
        .unwrap();
    assert_eq!(
        restored.outputs.len(),
        pending_out.len(),
        "all issued signatures restorable after restart"
    );
    assert_eq!(
        restored
            .outputs
            .iter()
            .map(|o| o.signature.amount)
            .sum::<u64>(),
        8
    );
}

/// Matrix leg — melt rollback (proof release + FAILED quote) survives a
/// restart through `make_store`'s backend.
fn matrix_melt_rollback_survives(make_store: &dyn Fn() -> Box<dyn StateStore>) {
    let mut rng = StdRng::seed_from_u64(0xD4);
    let saved_proofs;
    let melt_quote_id;

    {
        let mut mint = DemoMint::with_backend(Box::new(FailingWallet), Box::new(FrozenClock), 0)
            .with_state_store(make_store());
        let keyset = mint.public_keyset();
        let (_, proofs, _) = mint_with_pending(&mut mint, 8, &keyset, &mut rng).unwrap();
        saved_proofs = proofs.clone();

        let melt_quote = mint
            .post_melt_quote(nut05::MeltQuoteRequest {
                request: "lnbcdemo3sat1micronuts".to_string(),
                unit: "sat".to_string(),
            })
            .unwrap();
        melt_quote_id = melt_quote.quote.clone();

        let err = mint
            .post_melt(nut05::MeltRequest {
                quote: melt_quote.quote,
                inputs: proofs,
                outputs: None,
            })
            .expect_err("FailingWallet payment must fail");
        assert!(matches!(err, CashuError::PaymentFailed), "{err:?}");
    } // crash after rollback was persisted

    let mut mint = DemoMint::with_backend(Box::new(FailingWallet), Box::new(FrozenClock), 0)
        .with_state_store(make_store());

    let pairs = blind_amounts(&[8], mint.keyset_id(), &mut rng).unwrap();
    let (messages, _): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    mint.post_swap(nut03::SwapRequest {
        inputs: saved_proofs,
        outputs: messages,
    })
    .expect("released proofs are spendable after restart");

    let err = mint
        .post_melt(nut05::MeltRequest {
            quote: melt_quote_id,
            inputs: Vec::new(),
            outputs: None,
        })
        .expect_err("FAILED quote must stay terminal");
    assert!(matches!(err, CashuError::PaymentFailed), "{err:?}");
}

/// Every [`StateStore`] backend must give the same restart invariants the
/// file store gives: each call to `make_store` returns a fresh handle on
/// the SAME durable backing (a "restart"), so the legs below replay
/// crash→reconstruct against it.
fn run_restart_matrix(make_store: impl Fn() -> Box<dyn StateStore> + Send) {
    matrix_spent_stays_spent(&make_store);
    matrix_issued_quote_accounting_survives(&make_store);
    matrix_restore_index_restores(&make_store);
    matrix_melt_rollback_survives(&make_store);
}

#[test]
fn restart_matrix_snapshot_file_store() {
    let path = temp_state_path("matrix-file");
    let state_path = path.clone();
    run_restart_matrix(move || {
        Box::new(SnapshotFile::<MintStateSnapshot>::new(state_path.clone()))
    });
    std::fs::remove_file(&path).ok();
}

#[test]
fn restart_matrix_in_memory_store() {
    let cells: SharedCells = Arc::new(Mutex::new(HashMap::new()));
    let shared = cells.clone();
    run_restart_matrix(move || Box::new(InMemoryStateStore::with_shared(shared.clone(), None)));
}

#[test]
fn snapshot_file_trait_semantics_absent_none_corrupt_err() {
    let path = temp_state_path("sem-file");
    let store = SnapshotFile::<MintStateSnapshot>::new(&path);
    let seam: &dyn StateStore = &store;

    assert!(
        seam.load().unwrap().is_none(),
        "absent data must load as Ok(None)"
    );

    std::fs::write(&path, b"{ not json").unwrap();
    let err = seam.load().unwrap_err();
    assert!(
        err.contains("corrupt"),
        "undecodable data must be an Err, got load result with: {err}"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn in_memory_trait_semantics_absent_none_corrupt_err() {
    let cells: SharedCells = Arc::new(Mutex::new(HashMap::new()));
    let store = InMemoryStateStore::with_shared(cells.clone(), None);
    let seam: &dyn StateStore = &store;

    assert!(
        seam.load().unwrap().is_none(),
        "absent data must load as Ok(None)"
    );

    // Present but undecodable → Err (a refuse-to-boot condition), NOT None.
    cells
        .lock()
        .unwrap()
        .insert(InMemoryStateStore::KEY.to_string(), b"{ not json".to_vec());
    let err = seam.load().unwrap_err();
    assert!(
        err.contains("corrupt"),
        "undecodable data must be an Err, got load result with: {err}"
    );
}

#[test]
fn in_memory_bounded_store_rejects_oversize_snapshot() {
    let cells: SharedCells = Arc::new(Mutex::new(HashMap::new()));
    // Even the empty snapshot serializes to well over 16 bytes.
    let store = InMemoryStateStore::with_shared(cells, Some(16));

    let err = store
        .save(&MintStateSnapshot::default())
        .expect_err("oversize snapshot must be rejected");
    assert!(
        err.contains("bound"),
        "rejection must mention the bound: {err}"
    );
    assert!(
        store.load().unwrap().is_none(),
        "rejected save must not write anything"
    );
}
