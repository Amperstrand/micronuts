//! Differential tests: the mint's NUT-10/11 spending-condition enforcement
//! vs the upstream `cashu` crate 0.18 (house oracle pattern).
//!
//! Locks and witnesses are BUILT with the upstream crate's NUT-10/11 types
//! (so wire shapes come from its serde), the aggregated SIG_ALL messages
//! come from its `sig_all_msg_to_sign`/`sign_sig_all` implementations, and
//! each scenario additionally asserts the upstream verifier agrees with our
//! mint's accept/reject decision.
//!
//! L2 scope: locktime pathways are enforced. Differential scenarios use
//! locktimes far from the boundary (long-expired or far-future) so the
//! upstream oracle's wall clock and our injectable clock always agree;
//! the exact-boundary behavior (locktime == now) is pinned by frozen-clock
//! tests against OUR mint only — upstream's verifier reads the real wall
//! clock, so sub-second boundary agreement cannot be differentially
//! asserted. L3 HTLC scenarios live in htlc_differential.rs.

use std::str::FromStr;

use cashu::dhke::{blind_message, unblind_message};
use cashu::nuts::nut00::{
    BlindedMessage as CashuBlindedMessage, Proof as CashuProof, Witness as CashuWitness,
};
use cashu::nuts::nut01::{PublicKey as CashuPublicKey, SecretKey as CashuSecretKey};
use cashu::nuts::nut03::SwapRequest as CashuSwapRequest;
use cashu::nuts::nut05::MeltRequest as CashuMeltRequest;
use cashu::nuts::nut10::{
    Conditions, Kind, Secret as CashuNut10Secret, SecretData, SpendingConditionVerification,
    SpendingConditions,
};
use cashu::nuts::nut11::P2PKWitness;
use cashu::secret::Secret as CashuSecret;
use cashu::{Amount, Id};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut03, nut04, nut05};
use micronuts_mint::ln::{FakeWallet, MintClock, MockClock};
use micronuts_mint::type_conversion::{cashu_pk_to_lite, lite_pk_to_cashu};
use micronuts_mint::DemoMint;

// ---- lock/witness builders (upstream crate = wire-shape oracle) ----

fn signing_key(seed: u8) -> CashuSecretKey {
    CashuSecretKey::from_slice(&[seed; 32]).expect("valid scalar")
}

/// Serialize a P2PK spending condition to the well-known secret string via
/// the upstream crate's authoring path (validates as it constructs).
fn upstream_p2pk_secret(pubkey: &CashuPublicKey, conditions: Option<Conditions>) -> String {
    let secret: CashuSecret = SpendingConditions::new_p2pk(*pubkey, conditions)
        .try_into()
        .expect("upstream builds the lock");
    secret.to_string()
}

/// Serialize a P2PK secret from raw parts, skipping upstream authoring
/// validation (for conditions upstream's constructor refuses to build).
fn raw_p2pk_secret(data: &str, tags: Option<Vec<Vec<String>>>) -> String {
    let secret = CashuNut10Secret::new(Kind::P2PK, SecretData::new(data.to_string(), tags));
    let plain: CashuSecret = secret.try_into().expect("serializes");
    plain.to_string()
}

/// Sign a message with an upstream key, returning the hex signature.
fn sign_hex(key: &CashuSecretKey, message: &str) -> String {
    key.sign(message.as_bytes()).expect("sign").to_string()
}

/// Stringified-JSON witness serialized by the upstream P2PKWitness type.
fn witness_json(signatures: &[String]) -> String {
    serde_json::to_string(&P2PKWitness {
        signatures: signatures.to_vec(),
    })
    .expect("witness JSON")
}

// ---- mint helpers ----

/// A fresh demo mint with proofs minted over caller-chosen secrets (the
/// spending condition travels inside the secret, blind-signed by the mint).
fn mint_with_secrets(secret_amounts: &[(String, u64)]) -> (DemoMint, Vec<nut00::Proof>) {
    mint_with_secrets_at(Box::new(micronuts_mint::ln::SystemClock), secret_amounts)
}

/// [`mint_with_secrets`] with an injected clock (frozen-time scenarios).
fn mint_with_secrets_at(
    clock: Box<dyn MintClock>,
    secret_amounts: &[(String, u64)],
) -> (DemoMint, Vec<nut00::Proof>) {
    let mut mint = DemoMint::with_backend(Box::new(FakeWallet), clock, 0);
    let keyset = mint.public_keyset();
    let total: u64 = secret_amounts.iter().map(|(_, a)| a).sum();

    let quote = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: total,
            unit: "sat".to_string(),
        })
        .expect("mint quote");
    mint.get_mint_quote(&quote.quote)
        .expect("FakeWallet settles");

    let mut outputs = Vec::new();
    let mut blinders = Vec::new();
    for (secret, amount) in secret_amounts {
        let (b_, r) = blind_message(secret.as_bytes(), None).expect("blind");
        outputs.push(nut00::BlindedMessage {
            amount: *amount,
            id: keyset.id.clone(),
            b: cashu_pk_to_lite(&b_),
        });
        blinders.push((secret.clone(), r, *amount));
    }
    let response = mint
        .post_mint(nut04::MintRequest {
            quote: quote.quote,
            outputs,
        })
        .expect("mint outputs");

    let proofs = response
        .signatures
        .into_iter()
        .zip(blinders)
        .map(|(sig, (secret, r, amount))| {
            let mint_key = keyset
                .keys
                .iter()
                .find(|kp| kp.amount == amount)
                .expect("denomination key");
            let k = lite_pk_to_cashu(&mint_key.pubkey);
            let c = unblind_message(&lite_pk_to_cashu(&sig.c), &r, &k).expect("unblind");
            nut00::Proof {
                amount,
                id: keyset.id.clone(),
                secret,
                c: cashu_pk_to_lite(&c),
                dleq: None,
                witness: None,
            }
        })
        .collect();
    (mint, proofs)
}

/// Fresh blinded outputs decomposing `total`, as swap targets.
fn swap_outputs(total: u64) -> Vec<nut00::BlindedMessage> {
    let mut outputs = Vec::new();
    for amount in nut00::decompose_amount(total) {
        let secret = format!("swap-target-{amount}-{}", outputs.len());
        let (b_, _) = blind_message(secret.as_bytes(), None).expect("blind");
        outputs.push(nut00::BlindedMessage {
            amount,
            id: "009a1f293253e41e".to_string(),
            b: cashu_pk_to_lite(&b_),
        });
    }
    outputs
}

fn try_swap(
    mint: &mut DemoMint,
    inputs: Vec<nut00::Proof>,
) -> Result<nut03::SwapResponse, CashuError> {
    let total: u64 = inputs.iter().map(|p| p.amount).sum();
    mint.post_swap(nut03::SwapRequest {
        inputs,
        outputs: swap_outputs(total),
    })
}

fn melt_quote(mint: &mut DemoMint, amount: u64) -> String {
    mint.post_melt_quote(nut05::MeltQuoteRequest {
        request: format!("lnbcdemo{amount}sat1micronuts"),
        unit: "sat".to_string(),
    })
    .expect("melt quote")
    .quote
}

// ---- upstream-oracle mirrors ----

fn to_cashu_proof(proof: &nut00::Proof) -> CashuProof {
    CashuProof {
        amount: Amount::from(proof.amount),
        keyset_id: Id::from_str(&proof.id).expect("keyset id"),
        secret: CashuSecret::from_str(&proof.secret).expect("secret"),
        c: lite_pk_to_cashu(&proof.c),
        witness: proof
            .witness
            .as_deref()
            .and_then(|raw| serde_json::from_str::<P2PKWitness>(raw).ok())
            .map(CashuWitness::from),
        dleq: None,
        p2pk_e: None,
    }
}

fn to_cashu_outputs(outputs: &[nut00::BlindedMessage]) -> Vec<CashuBlindedMessage> {
    outputs
        .iter()
        .map(|o| CashuBlindedMessage {
            amount: Amount::from(o.amount),
            keyset_id: Id::from_str(&o.id).expect("keyset id"),
            blinded_secret: lite_pk_to_cashu(&o.b),
            witness: None,
        })
        .collect()
}

/// The upstream verdict for the same swap (witness logic only — amounts and
/// DHKE are known-good in these scenarios).
fn upstream_swap_verdict(inputs: &[nut00::Proof], outputs: &[nut00::BlindedMessage]) -> bool {
    CashuSwapRequest::new(
        inputs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(outputs),
    )
    .verify_spending_conditions()
    .is_ok()
}

fn upstream_melt_verdict(
    inputs: &[nut00::Proof],
    outputs: Option<&[nut00::BlindedMessage]>,
    quote_id: &str,
) -> bool {
    CashuMeltRequest::<String>::new(
        quote_id.to_string(),
        inputs.iter().map(to_cashu_proof).collect(),
        outputs.map(to_cashu_outputs),
    )
    .verify_spending_conditions()
    .is_ok()
}

/// Run a swap against our mint AND the upstream verifier; both must agree.
fn assert_swap_differential(mint: &mut DemoMint, inputs: Vec<nut00::Proof>, should_pass: bool) {
    let total: u64 = inputs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let upstream = upstream_swap_verdict(&inputs, &outputs);
    assert_eq!(
        upstream, should_pass,
        "UPSTREAM disagrees: expected verdict {should_pass}"
    );
    let ours = mint.post_swap(nut03::SwapRequest { inputs, outputs });
    assert_eq!(ours.is_ok(), should_pass, "OUR MINT disagrees: {ours:?}");
}

// ---- SIG_INPUTS scenarios ----

fn sig_inputs_conditions() -> Conditions {
    Conditions {
        sig_flag: cashu::nuts::nut11::SigFlag::SigInputs,
        ..Default::default()
    }
}

fn sig_all_conditions() -> Conditions {
    Conditions {
        sig_flag: cashu::nuts::nut11::SigFlag::SigAll,
        ..Default::default()
    }
}

#[test]
fn valid_witness_passes_and_marks_spent() {
    let key = signing_key(1);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret.clone(), 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));

    assert_swap_differential(&mut mint, proofs.clone(), true);

    // Spent: a second attempt fails as already-spent, not conditions.
    let err = try_swap(&mut mint, proofs).unwrap_err();
    assert_eq!(err, CashuError::TokensAlreadySpent);
}

#[test]
fn missing_witness_fails_without_marking_spent() {
    let key = signing_key(2);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Atomicity: the failed witness attempt did NOT claim the proof.
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn wrong_key_witness_fails() {
    let lock_key = signing_key(3);
    let wrong_key = signing_key(4);
    let secret = upstream_p2pk_secret(&lock_key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&wrong_key, &proofs[0].secret)]));

    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn invalid_signature_hex_fails() {
    let key = signing_key(5);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some("{\"signatures\":[\"deadbeef\"]}".to_string());

    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_inputs_requires_every_input_signed() {
    let key = signing_key(6);
    let conditions = Some(sig_inputs_conditions());
    let secrets: Vec<String> = (0..2)
        .map(|_| upstream_p2pk_secret(&key.public_key(), conditions.clone()))
        .collect();
    let (mut mint, mut proofs) =
        mint_with_secrets(&[(secrets[0].clone(), 8), (secrets[1].clone(), 8)]);

    // Only the first input carries a witness.
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Both signed: accepted.
    for proof in &mut proofs {
        proof.witness = Some(witness_json(&[sign_hex(&key, &proof.secret)]));
    }
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn multisig_2of3_enforced() {
    let k1 = signing_key(7);
    let k2 = signing_key(8);
    let k3 = signing_key(9);
    let conditions = Conditions {
        pubkeys: Some(vec![k2.public_key(), k3.public_key()]),
        num_sigs: Some(2),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&k1.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();

    // One signature of two required: fail.
    proofs[0].witness = Some(witness_json(&[sign_hex(&k1, &message)]));
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Two distinct keys: pass.
    proofs[0].witness = Some(witness_json(&[
        sign_hex(&k1, &message),
        sign_hex(&k3, &message),
    ]));
    assert_swap_differential(&mut mint, proofs.clone(), true);
}

#[test]
fn duplicate_signatures_from_one_key_fail() {
    let key = signing_key(10);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();

    // The same key signs twice (Schnorr is non-deterministic: two distinct
    // valid signatures) — upstream rejects this as DuplicateSignature.
    proofs[0].witness = Some(witness_json(&[
        sign_hex(&key, &message),
        sign_hex(&key, &message),
    ]));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn more_signatures_than_required_pass() {
    let k1 = signing_key(11);
    let k2 = signing_key(12);
    let conditions = Conditions {
        pubkeys: Some(vec![k2.public_key()]),
        num_sigs: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&k1.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();
    proofs[0].witness = Some(witness_json(&[
        sign_hex(&k1, &message),
        sign_hex(&k2, &message),
    ]));
    assert_swap_differential(&mut mint, proofs, true);
}

// ---- malformed conditions (both sides reject) ----

#[test]
fn malformed_sigflag_rejected() {
    let key = signing_key(13);
    let secret = raw_p2pk_secret(
        &key.public_key().to_hex(),
        Some(vec![vec![
            "sigflag".to_string(),
            "SIG_SOMETHING".to_string(),
        ]]),
    );
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));

    // Upstream rejects the unknown sigflag at condition-parse time.
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn duplicate_tag_rejected_by_spec() {
    // Upstream 0.18 keeps the FIRST occurrence (first-wins); the local spec
    // files mandate rejection and win — documented divergence, so this case
    // asserts OUR behavior only.
    let key = signing_key(14);
    let secret = raw_p2pk_secret(
        &key.public_key().to_hex(),
        Some(vec![
            vec!["sigflag".to_string(), "SIG_INPUTS".to_string()],
            vec!["sigflag".to_string(), "SIG_ALL".to_string()],
        ]),
    );
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let result = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(matches!(result, Err(CashuError::SpendConditionsNotMet)));
}

#[test]
fn x_coordinate_duplicate_keys_rejected() {
    let key = signing_key(15);
    let x_only = key.public_key().x_only_public_key();
    // Same x-coordinate with the opposite parity prefix: one key per spec.
    let flipped_parity = match key.public_key().to_bytes()[0] {
        0x02 => bitcoin::secp256k1::Parity::Odd,
        _ => bitcoin::secp256k1::Parity::Even,
    };
    let flipped = CashuPublicKey::from(bitcoin::secp256k1::PublicKey::from_x_only_public_key(
        x_only,
        flipped_parity,
    ));
    let secret = raw_p2pk_secret(
        &key.public_key().to_hex(),
        Some(vec![vec!["pubkeys".to_string(), flipped.to_hex()]]),
    );
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));

    // Both sides reject a duplicate key in the primary pathway.
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn invalid_data_pubkey_rejected() {
    let secret = raw_p2pk_secret("zzz-not-hex", None);
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);
    let err = try_swap(&mut mint, proofs).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

// ---- ordinary secrets and witness misuse ----

#[test]
fn plain_proofs_still_swap_with_or_without_locks_mixed() {
    let key = signing_key(16);
    let locked = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));
    let (mut mint, mut proofs) =
        mint_with_secrets(&[(locked, 8), ("plain-secret-1".to_string(), 8)]);

    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn witness_on_plain_proof_rejected() {
    let key = signing_key(17);
    let (mut mint, mut proofs) = mint_with_secrets(&[("plain-secret-2".to_string(), 8)]);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));

    // Upstream: IncorrectWitnessKind.
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn unparsable_condition_secret_is_anyone_can_spend() {
    // Structurally-broken "condition" (tags row with a number): upstream's
    // SecretData deserialization fails too, so both treat it as ordinary.
    let secret =
        "[\"P2PK\",{\"nonce\":\"n\",\"data\":\"zzz\",\"tags\":[[\"locktime\",1765300829]]}]";
    let (mut mint, proofs) = mint_with_secrets(&[(secret.to_string(), 8)]);
    assert!(proofs[0].witness.is_none());

    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn htlc_secret_rejected_until_l3() {
    let hash = "5c23fc3aec9d985bd5fc88ca8bceaccc52cf892715dd94b42b84f1b43350751e";
    let secret = format!("[\"HTLC\",{{\"nonce\":\"n\",\"data\":\"{hash}\"}}]");
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    let err = try_swap(&mut mint, proofs).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

// ---- SIG_ALL scenarios (message construction is fully upstream-driven) ----

#[test]
fn sig_all_accepts_upstream_transaction_signature() {
    let key = signing_key(19);
    let conditions = Some(sig_all_conditions());
    let secrets: Vec<String> = (0..2)
        .map(|_| upstream_p2pk_secret(&key.public_key(), conditions.clone()))
        .collect();
    let (mut mint, mut proofs) =
        mint_with_secrets(&[(secrets[0].clone(), 8), (secrets[1].clone(), 8)]);

    // Let the upstream crate build the witness over ITS aggregated message.
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let mut cashu_request = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    );
    cashu_request
        .sign_sig_all(key.clone())
        .expect("upstream signs");
    let witness = match &cashu_request.inputs()[0].witness {
        Some(CashuWitness::P2PKWitness(w)) => serde_json::to_string(w).unwrap(),
        other => panic!("expected P2PK witness, got {other:?}"),
    };
    assert!(cashu_request.verify_spending_conditions().is_ok());

    proofs[0].witness = Some(witness);
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(
        ours.is_ok(),
        "our mint rejected the upstream SIG_ALL swap: {ours:?}"
    );
}

#[test]
fn sig_all_unsigned_fails() {
    let key = signing_key(20);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_all_conditions()));
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_all_with_per_input_signatures_fails() {
    let key = signing_key(21);
    let conditions = Some(sig_all_conditions());
    let secrets: Vec<String> = (0..2)
        .map(|_| upstream_p2pk_secret(&key.public_key(), conditions.clone()))
        .collect();
    let (mut mint, mut proofs) =
        mint_with_secrets(&[(secrets[0].clone(), 8), (secrets[1].clone(), 8)]);

    // Signatures on each input's OWN secret: not the aggregated message.
    for proof in &mut proofs {
        proof.witness = Some(witness_json(&[sign_hex(&key, &proof.secret)]));
    }
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_all_mixed_conditions_rejected() {
    let k1 = signing_key(22);
    let k2 = signing_key(23);
    let secret_a = upstream_p2pk_secret(&k1.public_key(), Some(sig_all_conditions()));
    let secret_b = upstream_p2pk_secret(&k2.public_key(), Some(sig_all_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret_a, 8), (secret_b, 8)]);

    let message = {
        let total: u64 = proofs.iter().map(|p| p.amount).sum();
        let outputs = swap_outputs(total);
        CashuSwapRequest::new(
            proofs.iter().map(to_cashu_proof).collect(),
            to_cashu_outputs(&outputs),
        )
        .sig_all_msg_to_sign()
    };
    proofs[0].witness = Some(witness_json(&[sign_hex(&k1, &message)]));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_all_plain_input_mixed_in_rejected() {
    let key = signing_key(24);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_all_conditions()));
    let (mut mint, mut proofs) =
        mint_with_secrets(&[(secret.clone(), 8), ("plain-sigall-mix".to_string(), 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    )
    .sig_all_msg_to_sign();
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &message)]));

    let upstream = upstream_swap_verdict(&proofs, &outputs);
    assert!(!upstream);
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(matches!(ours, Err(CashuError::SpendConditionsNotMet)));
}

#[test]
fn sig_all_output_order_matters() {
    let key = signing_key(25);
    let conditions = Some(sig_all_conditions());
    let secret = upstream_p2pk_secret(&key.public_key(), conditions.clone());
    let secret2 = upstream_p2pk_secret(&key.public_key(), conditions);
    // Two locked inputs (8 + 4 sats) decompose into two swap outputs
    // ([8, 4]) so reordering actually changes the aggregated message.
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8), (secret2, 4)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let mut swapped_outputs = outputs.clone();
    swapped_outputs.reverse();
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&swapped_outputs),
    )
    .sig_all_msg_to_sign();
    // Signed for the REVERSED outputs, spent against the original order.
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &message)]));

    let upstream = upstream_swap_verdict(&proofs, &outputs);
    assert!(
        !upstream,
        "upstream must reject reordered-outputs signature"
    );
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(matches!(ours, Err(CashuError::SpendConditionsNotMet)));
}

// ---- melt pathway ----

#[test]
fn melt_p2pk_signed_succeeds_unsigned_fails() {
    let key = signing_key(26);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_inputs_conditions()));

    // Unsigned melt: rejected, proofs released (atomic).
    let (mut mint, proofs) = mint_with_secrets(&[(secret.clone(), 8)]);
    let quote = melt_quote(&mut mint, 8);
    let upstream = upstream_melt_verdict(&proofs, None, &quote);
    assert!(!upstream);
    let err = mint
        .post_melt(nut05::MeltRequest {
            quote: quote.clone(),
            inputs: proofs,
            outputs: None,
        })
        .unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);

    // Signed melt on a fresh mint: succeeds.
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let quote = melt_quote(&mut mint, 8);
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &proofs[0].secret)]));
    let response = mint
        .post_melt(nut05::MeltRequest {
            quote,
            inputs: proofs,
            outputs: None,
        })
        .expect("signed melt");
    assert!(response.paid);
}

#[test]
fn sig_all_melt_accepts_upstream_transaction_signature() {
    let key = signing_key(27);
    let secret = upstream_p2pk_secret(&key.public_key(), Some(sig_all_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let quote = melt_quote(&mut mint, 8);
    let mut cashu_request = CashuMeltRequest::<String>::new(
        quote.clone(),
        proofs.iter().map(to_cashu_proof).collect(),
        None,
    );
    cashu_request
        .sign_sig_all(key.clone())
        .expect("upstream signs");
    let witness = match &cashu_request.inputs()[0].witness {
        Some(CashuWitness::P2PKWitness(w)) => serde_json::to_string(w).unwrap(),
        other => panic!("expected P2PK witness, got {other:?}"),
    };
    assert!(cashu_request.verify_spending_conditions().is_ok());

    proofs[0].witness = Some(witness);
    let response = mint
        .post_melt(nut05::MeltRequest {
            quote,
            inputs: proofs,
            outputs: None,
        })
        .expect("our mint accepts the upstream SIG_ALL melt");
    assert!(response.paid);
}

// ---- L1 locktime semantics (primary pathway only) ----

#[test]
fn future_locktime_primary_path_still_spends() {
    let key = signing_key(28);
    let refund_key = signing_key(29);
    let future = 4_102_444_800u64; // 2100-01-01
    let conditions = Conditions {
        locktime: Some(future),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    // Before expiry, only the primary pathway is available: the refund key
    // cannot spend (upstream agrees), the primary key can.
    let mut refund_attempt = proofs.clone();
    refund_attempt[0].witness = Some(witness_json(&[sign_hex(
        &refund_key,
        &refund_attempt[0].secret,
    )]));
    assert_swap_differential(&mut mint, refund_attempt, false);

    // The failed refund attempt did not spend the proof; the primary key
    // then spends it.
    let secret_str = proofs[0].secret.clone();
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &secret_str)]));
    assert_swap_differential(&mut mint, proofs, true);
}

// ---- L2 locktime semantics (refund pathways after expiry) ----

/// Long-expired locktime: the upstream oracle's wall clock and any sane
/// test clock agree the lock has expired.
const EXPIRED_LOCKTIME: u64 = 1_600_000_000; // 2020-09-13

#[test]
fn post_expiry_refund_key_spends() {
    let key = signing_key(30);
    let refund_key = signing_key(31);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    // No witness: still locked (refund path needs its signature).
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // The refund key alone spends after expiry.
    proofs[0].witness = Some(witness_json(&[sign_hex(&refund_key, &proofs[0].secret)]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn post_expiry_primary_pathway_still_spends() {
    // NUT-11 §Refund Multisig: the refund path is ADDITIONAL — the primary
    // pathway continues to apply after expiry (upstream mirrors this).
    let key = signing_key(32);
    let refund_key = signing_key(33);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let secret_str = proofs[0].secret.clone();
    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &secret_str)]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn post_expiry_no_refund_keys_anyone_can_spend() {
    let key = signing_key(34);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    // No witness at all: expired + no refund keys → anyone-can-spend.
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn post_expiry_refund_multisig_2of2() {
    let key = signing_key(35);
    let refund_a = signing_key(36);
    let refund_b = signing_key(37);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_a.public_key(), refund_b.public_key()]),
        num_sigs_refund: Some(2),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();

    // One refund signature of two required: fail.
    proofs[0].witness = Some(witness_json(&[sign_hex(&refund_a, &message)]));
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Both refund keys: pass.
    proofs[0].witness = Some(witness_json(&[
        sign_hex(&refund_a, &message),
        sign_hex(&refund_b, &message),
    ]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn post_expiry_primary_key_does_not_count_as_refund_signature() {
    // A 2-of-2 primary the single primary signature cannot satisfy, with a
    // 1-of-1 refund pathway the primary key is NOT part of: the primary
    // signature must not serve as the refund signature.
    let key = signing_key(38);
    let k2 = signing_key(40);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        pubkeys: Some(vec![k2.public_key()]),
        num_sigs: Some(2),
        refund_keys: Some(vec![signing_key(41).public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();

    proofs[0].witness = Some(witness_json(&[sign_hex(&key, &message)]));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_all_post_expiry_refund_key_spends_transaction() {
    let key = signing_key(42);
    let refund_key = signing_key(43);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let mut cashu_request = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    );
    cashu_request
        .sign_sig_all(refund_key.clone())
        .expect("upstream signs with the refund key");
    let witness = match &cashu_request.inputs()[0].witness {
        Some(CashuWitness::P2PKWitness(w)) => serde_json::to_string(w).unwrap(),
        other => panic!("expected P2PK witness, got {other:?}"),
    };
    assert!(cashu_request.verify_spending_conditions().is_ok());

    proofs[0].witness = Some(witness);
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(
        ours.is_ok(),
        "our mint rejected the refund SIG_ALL swap: {ours:?}"
    );
}

#[test]
fn sig_all_post_expiry_no_refund_anyone_can_spend() {
    let key = signing_key(44);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        ..sig_all_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    // No witness: expired + no refund keys → anyone-can-spend even under
    // SIG_ALL (upstream checks this before looking for a witness).
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn sig_all_pre_expiry_refund_signature_blocked() {
    let key = signing_key(45);
    let refund_key = signing_key(46);
    let future = 4_102_444_800u64; // 2100-01-01
    let conditions = Conditions {
        locktime: Some(future),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let mut cashu_request = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    );
    cashu_request
        .sign_sig_all(refund_key.clone())
        .expect("upstream signs");
    let witness = match &cashu_request.inputs()[0].witness {
        Some(CashuWitness::P2PKWitness(w)) => serde_json::to_string(w).unwrap(),
        other => panic!("expected P2PK witness, got {other:?}"),
    };
    assert!(!cashu_request.verify_spending_conditions().is_ok());

    proofs[0].witness = Some(witness);
    let upstream = upstream_swap_verdict(&proofs, &outputs);
    assert!(!upstream, "upstream must block pre-expiry refund SIG_ALL");
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs,
        outputs,
    });
    assert!(matches!(ours, Err(CashuError::SpendConditionsNotMet)));
}

// ---- frozen-clock boundary tests (OUR mint; the upstream oracle reads
// ---- the wall clock, so the exact boundary cannot be tested
// ---- differentially — see the module docs) ----

const FROZEN_NOW: u64 = 1_700_000_050;

fn frozen_mint_with_locktime(locktime: u64) -> (DemoMint, Vec<nut00::Proof>, CashuSecretKey) {
    let key = signing_key(47);
    let refund_key = signing_key(48);
    let conditions = Conditions {
        locktime: Some(locktime),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let (mint, proofs) = mint_with_secrets_at(Box::new(MockClock::new(FROZEN_NOW)), &[(secret, 8)]);
    (mint, proofs, refund_key)
}

fn refund_attempt(proofs: &[nut00::Proof], refund_key: &CashuSecretKey) -> Vec<nut00::Proof> {
    let mut attempt = proofs.to_vec();
    attempt[0].witness = Some(witness_json(&[sign_hex(refund_key, &attempt[0].secret)]));
    attempt
}

/// locktime == now: NOT expired (upstream computes `locktime < now`
/// strictly; the spec's "greater than locktime" wording agrees) — the
/// refund pathway stays closed for one more second.
#[test]
fn boundary_locktime_equals_now_still_active() {
    let (mut mint, proofs, refund_key) = frozen_mint_with_locktime(FROZEN_NOW);
    let err = try_swap(&mut mint, refund_attempt(&proofs, &refund_key)).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

/// locktime == now - 1: expired — the refund pathway is open.
#[test]
fn boundary_locktime_now_minus_one_expired() {
    let (mut mint, proofs, refund_key) = frozen_mint_with_locktime(FROZEN_NOW - 1);
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let inputs = refund_attempt(&proofs, &refund_key);
    let result = mint.post_swap(nut03::SwapRequest { inputs, outputs });
    assert!(
        result.is_ok(),
        "refund must spend at now = locktime + 1: {result:?}"
    );
}

/// locktime == now + 1: active — the refund pathway is closed.
#[test]
fn boundary_locktime_now_plus_one_active() {
    let (mut mint, proofs, refund_key) = frozen_mint_with_locktime(FROZEN_NOW + 1);
    let err = try_swap(&mut mint, refund_attempt(&proofs, &refund_key)).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

/// The same proofs transition from locked to refund-spendable purely by
/// the clock advancing past the locktime (no re-minting).
#[test]
fn boundary_clock_advance_opens_refund_pathway() {
    let key = signing_key(49);
    let refund_key = signing_key(50);
    let conditions = Conditions {
        locktime: Some(FROZEN_NOW + 10),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_p2pk_secret(&key.public_key(), Some(conditions));
    let clock = MockClock::new(FROZEN_NOW);
    let (mut mint, proofs) = mint_with_secrets_at(Box::new(clock.clone()), &[(secret, 8)]);

    // Before expiry: refund signature rejected, proofs stay unspent.
    let err = try_swap(&mut mint, refund_attempt(&proofs, &refund_key)).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);

    // Freeze-thaw: advance the SAME clock past the locktime.
    clock.advance(11);
    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let result = mint.post_swap(nut03::SwapRequest {
        inputs: refund_attempt(&proofs, &refund_key),
        outputs,
    });
    assert!(
        result.is_ok(),
        "refund must spend after the clock advances: {result:?}"
    );
}
