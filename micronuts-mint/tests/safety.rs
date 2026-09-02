//! Payment-safety tests for the backend-driven mint core.
//!
//! Covers the Amperstrand cashu-payment-safety checklist items that the
//! demo path does not exercise: atomic double-spend rejection, keyset
//! binding, NUT-08 fee accounting, melt rollback on backend failure,
//! terminal melt states, and NUT-09 restore fidelity.

use cashu_core_lite::crypto::{blind_message, unblind_signature};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::{nut00, nut01, nut03, nut04, nut05, nut09};
use micronuts_mint::ln::{FakeWallet, LightningBackend, MintClock};
use micronuts_mint::DemoMint;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};

/// Backend whose payments always fail (rollback path under test).
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

/// Frozen clock so quote expiry never interferes with these tests.
struct FrozenClock;

impl MintClock for FrozenClock {
    fn now_secs(&self) -> u64 {
        1_000_000
    }
}

struct PendingOutput {
    amount: u64,
    blinder: SecretKey,
    secret: String,
}

/// Mint `sats` directly against the mint (quote → settle → blind → mint →
/// unblind), mirroring the wallet flow but keeping the mint handle for
/// direct assertions.
fn mint_proofs_direct(
    mint: &mut DemoMint,
    sats: u64,
    keyset: &nut01::KeySet,
    rng: &mut StdRng,
) -> Result<Vec<nut00::Proof>, CashuError> {
    let quote = mint.post_mint_quote(nut04::MintQuoteRequest {
        amount: sats,
        unit: "sat".to_string(),
    })?;
    mint.get_mint_quote(&quote.quote)?; // FakeWallet settles on first poll

    let denominations = nut00::decompose_amount(sats);
    let mut messages = Vec::with_capacity(denominations.len());
    let mut pending = Vec::with_capacity(denominations.len());

    for amount in denominations {
        let mut secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut secret_bytes);
        let secret_hex = hex::encode(secret_bytes);

        let mut blinder_bytes = [0u8; 32];
        rng.fill_bytes(&mut blinder_bytes);
        let blinder = SecretKey::from_slice(&blinder_bytes)
            .map_err(|_| CashuError::Crypto("bad blinder scalar".into()))?;

        let blinded = blind_message(secret_hex.as_bytes(), Some(blinder.clone()))
            .map_err(|_| CashuError::Crypto("blind_message failed".into()))?;

        messages.push(nut00::BlindedMessage {
            amount,
            id: mint.keyset_id().to_string(),
            b: blinded.blinded,
        });
        pending.push(PendingOutput {
            amount,
            blinder,
            secret: secret_hex,
        });
    }

    let response = mint.post_mint(nut04::MintRequest {
        quote: quote.quote,
        outputs: messages,
    })?;

    let mut proofs = Vec::with_capacity(pending.len());
    for (p, sig) in pending.iter().zip(response.signatures.iter()) {
        let mint_pubkey = keyset
            .keys
            .iter()
            .find(|kp| kp.amount == p.amount)
            .map(|kp| &kp.pubkey)
            .ok_or(CashuError::KeysetNotFound)?;
        let c = unblind_signature(&sig.c, &p.blinder, mint_pubkey)
            .map_err(|_| CashuError::Crypto("unblind failed".into()))?;
        proofs.push(nut00::Proof {
            amount: p.amount,
            id: sig.id.clone(),
            secret: p.secret.clone(),
            c,
            // Proof-level DLEQ (with blinder) is wallet metadata; the mint
            // verifies via the privkey path, so tests omit it.
            dleq: None,
        });
    }
    Ok(proofs)
}

/// Blind `amounts` for a swap/melt change output, returning the messages
/// plus the pending data needed to unblind the response.
fn blind_outputs(
    amounts: &[u64],
    keyset_id: &str,
    rng: &mut StdRng,
) -> Result<(Vec<nut00::BlindedMessage>, Vec<PendingOutput>), CashuError> {
    let mut messages = Vec::with_capacity(amounts.len());
    let mut pending = Vec::with_capacity(amounts.len());
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
        messages.push(nut00::BlindedMessage {
            amount,
            id: keyset_id.to_string(),
            b: blinded.blinded,
        });
        pending.push(PendingOutput {
            amount,
            blinder,
            secret: secret_hex,
        });
    }
    Ok((messages, pending))
}

fn zero_fee_mint() -> DemoMint {
    DemoMint::with_backend(Box::new(FakeWallet), Box::new(FrozenClock), 0)
}

fn fee_mint(ppk: u64) -> DemoMint {
    DemoMint::with_backend(Box::new(FakeWallet), Box::new(FrozenClock), ppk)
}

#[test]
fn double_spend_within_one_request_rejected_atomically() {
    let mut mint = zero_fee_mint();
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(1);
    let proofs = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();
    assert_eq!(proofs.len(), 1);

    // Same proof twice in one swap input list.
    let (outputs, _) = blind_outputs(&[8], mint.keyset_id(), &mut rng).unwrap();
    let result = mint.post_swap(nut03::SwapRequest {
        inputs: vec![proofs[0].clone(), proofs[0].clone()],
        outputs,
    });
    assert!(matches!(result, Err(CashuError::TokensAlreadySpent)));

    // Atomicity: the rejected attempt must NOT have burned the proof.
    let (outputs, _) = blind_outputs(&[4], mint.keyset_id(), &mut rng).unwrap();
    let ok = mint.post_swap(nut03::SwapRequest {
        inputs: vec![proofs[0].clone()],
        outputs,
    });
    assert!(
        ok.is_ok(),
        "proof must survive a rejected duplicate-input swap"
    );
}

#[test]
fn double_spend_across_operations_rejected() {
    let mut mint = zero_fee_mint();
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(2);
    let proofs = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();

    let (outputs, _) = blind_outputs(&[4], mint.keyset_id(), &mut rng).unwrap();
    mint.post_swap(nut03::SwapRequest {
        inputs: vec![proofs[0].clone()],
        outputs,
    })
    .unwrap();

    // The spent proof cannot fund a melt now.
    let melt_quote = mint
        .post_melt_quote(nut05::MeltQuoteRequest {
            request: "lnbcdemo1sat1micronuts".to_string(),
            unit: "sat".to_string(),
        })
        .unwrap();
    let result = mint.post_melt(nut05::MeltRequest {
        quote: melt_quote.quote,
        inputs: vec![proofs[0].clone()],
        outputs: None,
    });
    assert!(matches!(result, Err(CashuError::TokensAlreadySpent)));
}

#[test]
fn foreign_keyset_proof_rejected() {
    let mut mint = zero_fee_mint();
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(3);
    let mut proofs = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();
    proofs[0].id = "00deadbeefdeadbe".to_string();

    let (outputs, _) = blind_outputs(&[4], mint.keyset_id(), &mut rng).unwrap();
    let result = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(matches!(result, Err(CashuError::KeysetNotFound)));
}

#[test]
fn swap_charges_nut08_input_fee_exactly() {
    // ppk=20, two inputs (128+64) → fee = ceil(2*20/1000) = 1 sat.
    let mut mint = fee_mint(20);
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(4);
    let proofs = mint_proofs_direct(&mut mint, 192, &keyset, &mut rng).unwrap();
    assert_eq!(proofs.len(), 2);

    // Full-value outputs: rejected (fee unpaid).
    let (outputs, _) = blind_outputs(&[128, 64], mint.keyset_id(), &mut rng).unwrap();
    let result = mint.post_swap(nut03::SwapRequest {
        inputs: proofs.clone(),
        outputs,
    });
    assert!(matches!(result, Err(CashuError::AmountMismatch)));

    // Fee-adjusted outputs: accepted (191 = 192 - 1 fee).
    let (outputs, _) =
        blind_outputs(&[128, 32, 16, 8, 4, 2, 1], mint.keyset_id(), &mut rng).unwrap();
    let ok = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(
        ok.is_ok(),
        "swap at output_sum = input_sum - ceil(n*ppk/1000) must pass"
    );
}

#[test]
fn melt_change_must_equal_overpay_minus_fees() {
    // ppk=20, one input (64) → fee = ceil(20/1000) = 1; melt 10 → change 53.
    let mut mint = fee_mint(20);
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(5);
    let proofs = mint_proofs_direct(&mut mint, 64, &keyset, &mut rng).unwrap();

    let melt_quote = mint
        .post_melt_quote(nut05::MeltQuoteRequest {
            request: "lnbcdemo10sat1micronuts".to_string(),
            unit: "sat".to_string(),
        })
        .unwrap();
    assert_eq!(melt_quote.amount, 10);

    // Wrong change (52 ≠ 53): rejected.
    let (outputs, _) = blind_outputs(&[52], mint.keyset_id(), &mut rng).unwrap();
    let result = mint.post_melt(nut05::MeltRequest {
        quote: melt_quote.quote.clone(),
        inputs: proofs.clone(),
        outputs: Some(outputs),
    });
    assert!(matches!(result, Err(CashuError::AmountMismatch)));

    // Exact change (53): accepted with signed change outputs.
    let (outputs, _) = blind_outputs(&[32, 16, 4, 1], mint.keyset_id(), &mut rng).unwrap();
    let response = mint
        .post_melt(nut05::MeltRequest {
            quote: melt_quote.quote,
            inputs: proofs,
            outputs: Some(outputs),
        })
        .unwrap();
    assert!(response.paid);
    let change_total: u64 = response.change.unwrap().iter().map(|s| s.amount).sum();
    assert_eq!(change_total, 53);
}

#[test]
fn failing_payment_releases_proofs_and_fails_quote() {
    let mut mint = DemoMint::with_backend(Box::new(FailingWallet), Box::new(FrozenClock), 0);
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(6);
    let proofs = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();

    let melt_quote = mint
        .post_melt_quote(nut05::MeltQuoteRequest {
            request: "lnbcdemo2sat1micronuts".to_string(),
            unit: "sat".to_string(),
        })
        .unwrap();
    let result = mint.post_melt(nut05::MeltRequest {
        quote: melt_quote.quote.clone(),
        inputs: proofs.clone(),
        outputs: None,
    });
    assert!(matches!(result, Err(CashuError::PaymentFailed)));

    // Quote parked in FAILED (get returns the raw state string).
    let state = mint.get_melt_quote(&melt_quote.quote).unwrap().state;
    assert_eq!(state, "FAILED");

    // Rollback: the same proofs are spendable again (here: a swap succeeds).
    let (outputs, _) = blind_outputs(&[4], mint.keyset_id(), &mut rng).unwrap();
    let ok = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(
        ok.is_ok(),
        "claimed proofs must be released after a failed payment"
    );
}

#[test]
fn melt_blank_outputs_receive_overpay_decomposition() {
    // v4 wallets send amount-0 blank outputs; the mint imprints the overpay.
    let mut mint = zero_fee_mint();
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(9);
    let proofs = mint_proofs_direct(&mut mint, 64, &keyset, &mut rng).unwrap();

    let melt_quote = mint
        .post_melt_quote(nut05::MeltQuoteRequest {
            request: "lnbcdemo10sat1micronuts".to_string(),
            unit: "sat".to_string(),
        })
        .unwrap();

    // 6 blanks (ceil(log2(54)) = 6) for a 54-sat overpay.
    let (outputs, _) = blind_outputs(&[0, 0, 0, 0, 0, 0], mint.keyset_id(), &mut rng).unwrap();
    let response = mint
        .post_melt(nut05::MeltRequest {
            quote: melt_quote.quote,
            inputs: proofs,
            outputs: Some(outputs),
        })
        .unwrap();
    assert!(response.paid);

    let change = response.change.expect("blanks must be signed back");
    let total: u64 = change.iter().map(|s| s.amount).sum();
    assert_eq!(total, 54, "change must equal the overpay exactly");
    for sig in &change {
        assert!(sig.amount > 0, "no blank may be signed as zero");
        assert!(
            change.iter().all(|s| s.amount.count_ones() == 1),
            "imprinted amounts are powers of two"
        );
    }
}

#[test]
fn melt_quote_is_single_shot() {
    let mut mint = zero_fee_mint();
    let keyset = mint.public_keyset();
    let mut rng = StdRng::seed_from_u64(7);
    let proofs_a = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();
    let proofs_b = mint_proofs_direct(&mut mint, 4, &keyset, &mut rng).unwrap();

    let melt_quote = mint
        .post_melt_quote(nut05::MeltQuoteRequest {
            request: "lnbcdemo2sat1micronuts".to_string(),
            unit: "sat".to_string(),
        })
        .unwrap();

    let first = mint
        .post_melt(nut05::MeltRequest {
            quote: melt_quote.quote.clone(),
            inputs: proofs_a,
            outputs: None,
        })
        .unwrap();
    assert!(first.paid);

    let second = mint.post_melt(nut05::MeltRequest {
        quote: melt_quote.quote,
        inputs: proofs_b,
        outputs: None,
    });
    assert!(matches!(second, Err(CashuError::MeltAlreadyPaid)));
}

#[test]
fn restore_returns_previously_signed_signatures() {
    let mut mint = zero_fee_mint();
    let mut rng = StdRng::seed_from_u64(8);
    let quote = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: 3,
            unit: "sat".to_string(),
        })
        .unwrap();
    mint.get_mint_quote(&quote.quote).unwrap();

    let (outputs, _pending) = blind_outputs(&[1, 2], mint.keyset_id(), &mut rng).unwrap();
    let response = mint
        .post_mint(nut04::MintRequest {
            quote: quote.quote,
            outputs: outputs.clone(),
        })
        .unwrap();

    let restored = mint
        .post_restore(nut09::RestoreRequest {
            outputs: outputs.iter().map(|o| o.b).collect(),
        })
        .unwrap();
    assert_eq!(restored.outputs.len(), 2);
    for (sig, r) in response.signatures.iter().zip(restored.outputs.iter()) {
        assert_eq!(sig.id, r.signature.id);
        assert_eq!(sig.amount, r.signature.amount);
    }

    // Unknown B_ values are skipped, not errors.
    let (foreign, _) = blind_outputs(&[1], mint.keyset_id(), &mut rng).unwrap();
    let restored = mint
        .post_restore(nut09::RestoreRequest {
            outputs: vec![foreign[0].b],
        })
        .unwrap();
    assert_eq!(restored.outputs.len(), 0);
}
