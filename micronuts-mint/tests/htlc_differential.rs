//! Differential tests: the mint's NUT-14 HTLC spending-condition
//! enforcement vs the upstream `cashu` crate 0.18 (house oracle pattern).
//!
//! Locks are BUILT with the upstream crate's NUT-10/14 types (wire shapes
//! from its serde), the aggregated SIG_ALL messages come from its
//! `sig_all_msg_to_sign`, and each scenario asserts the upstream verifier
//! agrees with our mint's accept/reject decision. Divergences (where the
//! local spec files win over upstream's untagged-witness parsing) are
//! asserted against OUR mint only, mirroring the
//! `duplicate_tag_rejected_by_spec` precedent in p2pk_differential.rs.

use std::str::FromStr;

use bitcoin::hashes::{sha256, Hash};
use cashu::dhke::{blind_message, unblind_message};
use cashu::nuts::nut00::{
    BlindedMessage as CashuBlindedMessage, Proof as CashuProof, Witness as CashuWitness,
};
use cashu::nuts::nut01::SecretKey as CashuSecretKey;
use cashu::nuts::nut03::SwapRequest as CashuSwapRequest;
use cashu::nuts::nut05::MeltRequest as CashuMeltRequest;
use cashu::nuts::nut10::{
    Conditions, Kind, Secret as CashuNut10Secret, SecretData, SpendingConditionVerification,
    SpendingConditions,
};
use cashu::nuts::nut14::HTLCWitness as CashuHtlcWitness;
use cashu::secret::Secret as CashuSecret;
use cashu::{Amount, Id};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut03, nut04, nut05};
use micronuts_mint::type_conversion::{cashu_pk_to_lite, lite_pk_to_cashu};
use micronuts_mint::DemoMint;

// ---- lock/witness builders (upstream crate = wire-shape oracle) ----

fn signing_key(seed: u8) -> CashuSecretKey {
    CashuSecretKey::from_slice(&[seed; 32]).expect("valid scalar")
}

/// Serialize an HTLC spending condition to the well-known secret string via
/// the upstream crate's authoring path (validates as it constructs).
fn upstream_htlc_secret(hash_hex: &str, conditions: Option<Conditions>) -> String {
    let secret: CashuSecret = SpendingConditions::new_htlc_hash(hash_hex, conditions)
        .expect("valid hash lock")
        .try_into()
        .expect("upstream builds the lock");
    secret.to_string()
}

/// Serialize an HTLC secret from raw parts, skipping upstream authoring
/// validation (for shapes its constructor refuses to build).
fn raw_htlc_secret(data: &str, tags: Option<Vec<Vec<String>>>) -> String {
    let secret = CashuNut10Secret::new(Kind::HTLC, SecretData::new(data.to_string(), tags));
    let plain: CashuSecret = secret.try_into().expect("serializes");
    plain.to_string()
}

fn sign_hex(key: &CashuSecretKey, message: &str) -> String {
    key.sign(message.as_bytes()).expect("sign").to_string()
}

/// Stringified-JSON HTLC witness serialized by the upstream type.
fn htlc_witness_json(preimage: Option<&str>, signatures: &[String]) -> String {
    serde_json::to_string(&CashuHtlcWitness {
        preimage: preimage
            .expect("upstream HTLCWitness requires a preimage")
            .to_string(),
        signatures: if signatures.is_empty() {
            None
        } else {
            Some(signatures.to_vec())
        },
    })
    .expect("witness JSON")
}

/// Signatures-only witness (the NUT-11 P2PK wire shape used by the sender
/// pathway); built as raw JSON because upstream's HTLCWitness type requires
/// a preimage field.
fn signatures_only_witness_json(signatures: &[String]) -> String {
    serde_json::json!({ "signatures": signatures }).to_string()
}

fn sha256_hex(preimage: &[u8; 32]) -> String {
    sha256::Hash::hash(preimage).to_string()
}

// ---- mint helpers (mirrors of p2pk_differential.rs) ----

fn mint_with_secrets(secret_amounts: &[(String, u64)]) -> (DemoMint, Vec<nut00::Proof>) {
    let mut mint = DemoMint::new();
    let keyset = mint.public_keyset();
    let total: u64 = secret_amounts.iter().map(|(_, a)| a).sum();

    let quote = mint
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: total,
            unit: "sat".to_string(),
            pubkey: None,
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
            signature: None,
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

/// Parse a stringified witness JSON into the upstream untagged `Witness`
/// the way Proof-JSON deserialization would: HTLCWitness first (preimage
/// key present), then P2PKWitness (signatures-only).
fn to_cashu_witness(raw: &str) -> Option<CashuWitness> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("preimage").is_some() {
        let witness: CashuHtlcWitness = serde_json::from_value(value).ok()?;
        Some(CashuWitness::HTLCWitness(witness))
    } else {
        let witness: cashu::nuts::nut11::P2PKWitness = serde_json::from_value(value).ok()?;
        Some(CashuWitness::P2PKWitness(witness))
    }
}

fn to_cashu_proof(proof: &nut00::Proof) -> CashuProof {
    CashuProof {
        amount: Amount::from(proof.amount),
        keyset_id: Id::from_str(&proof.id).expect("keyset id"),
        secret: CashuSecret::from_str(&proof.secret).expect("secret"),
        c: lite_pk_to_cashu(&proof.c),
        witness: proof.witness.as_deref().and_then(to_cashu_witness),
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

fn upstream_swap_verdict(inputs: &[nut00::Proof], outputs: &[nut00::BlindedMessage]) -> bool {
    CashuSwapRequest::new(
        inputs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(outputs),
    )
    .verify_spending_conditions()
    .is_ok()
}

fn assert_swap_differential(mint: &mut DemoMint, inputs: Vec<nut00::Proof>, should_pass: bool) {
    let total: u64 = inputs.iter().map(|p| p.amount).sum();
    assert_swap_differential_with(mint, inputs, swap_outputs(total), should_pass);
}

/// [`assert_swap_differential`] against caller-supplied outputs — SIG_ALL
/// scenarios must sign and spend against the SAME outputs (fresh
/// `swap_outputs` draws new random blinders).
fn assert_swap_differential_with(
    mint: &mut DemoMint,
    inputs: Vec<nut00::Proof>,
    outputs: Vec<nut00::BlindedMessage>,
    should_pass: bool,
) {
    let upstream = upstream_swap_verdict(&inputs, &outputs);
    assert_eq!(
        upstream, should_pass,
        "UPSTREAM disagrees: expected verdict {should_pass}"
    );
    let ours = mint.post_swap(nut03::SwapRequest { inputs, outputs });
    assert_eq!(ours.is_ok(), should_pass, "OUR MINT disagrees: {ours:?}");
}

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

const EXPIRED_LOCKTIME: u64 = 1_600_000_000; // 2020-09-13
const FUTURE_LOCKTIME: u64 = 4_102_444_800; // 2100-01-01

// ---- SIG_INPUTS: hash lock ----

#[test]
fn preimage_only_spends_without_pubkeys() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, None);
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode(preimage)), &[]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn wrong_preimage_rejected_without_pubkeys() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, None);
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode([8u8; 32])), &[]));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn missing_witness_rejected() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, None);
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    assert_swap_differential(&mut mint, proofs, false);
}

// ---- SIG_INPUTS: receiver pathway (preimage + signatures) ----

#[test]
fn preimage_and_signature_spend() {
    let key = signing_key(60);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    // Signature without the preimage: rejected.
    proofs[0].witness = Some(signatures_only_witness_json(&[sign_hex(
        &key,
        &proofs[0].secret,
    )]));
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Preimage + signature: accepted.
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&key, &proofs[0].secret)],
    ));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn preimage_with_wrong_key_signature_rejected() {
    let key = signing_key(61);
    let wrong_key = signing_key(62);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&wrong_key, &proofs[0].secret)],
    ));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn preimage_multisig_2of3() {
    let k1 = signing_key(63);
    let k2 = signing_key(64);
    let k3 = signing_key(65);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![k1.public_key(), k2.public_key(), k3.public_key()]),
        num_sigs: Some(2),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let message = proofs[0].secret.clone();

    // One of two: fail.
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&k1, &message)],
    ));
    assert_swap_differential(&mut mint, proofs.clone(), false);

    // Two of three: pass.
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&k1, &message), sign_hex(&k3, &message)],
    ));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn receiver_path_still_available_after_locktime() {
    // The receiver (hash lock) pathway is ALWAYS available, even after the
    // locktime has expired and refund keys exist.
    let key = signing_key(66);
    let refund_key = signing_key(67);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&key, &proofs[0].secret)],
    ));
    assert_swap_differential(&mut mint, proofs, true);
}

// ---- SIG_INPUTS: sender pathway (refund after locktime) ----

#[test]
fn refund_key_spends_after_expiry_without_valid_preimage() {
    let refund_key = signing_key(69);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    // Wrong preimage + refund signature: the sender pathway spends.
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode([9u8; 32])),
        &[sign_hex(&refund_key, &proofs[0].secret)],
    ));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn refund_signatures_only_witness_spends_after_expiry() {
    // Upstream divergence (spec wins): cashu 0.18's untagged witness enum
    // parses a signatures-only witness as P2PKWitness, so its `verify_htlc`
    // rejects the sender pathway spent this way; NUT-14 defines the sender
    // pathway as "providing signature(s) as per the NUT-11 rules", whose
    // witness IS the signatures array. Our mint accepts — asserted here
    // against OUR mint only (see spending.rs verify_htlc_pathways note).
    let refund_key = signing_key(71);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let message = proofs[0].secret.clone();
    proofs[0].witness = Some(signatures_only_witness_json(&[sign_hex(
        &refund_key,
        &message,
    )]));
    let ours = try_swap(&mut mint, proofs);
    assert!(
        ours.is_ok(),
        "our mint must accept the sender pathway: {ours:?}"
    );
}

#[test]
fn refund_signature_blocked_before_expiry() {
    let refund_key = signing_key(73);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(FUTURE_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode([9u8; 32])),
        &[sign_hex(&refund_key, &proofs[0].secret)],
    ));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn expired_without_refund_keys_is_anyone_can_spend() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        ..sig_inputs_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);

    // No witness, no preimage: expired + no refund keys → anyone-can-spend.
    assert_swap_differential(&mut mint, proofs, true);
}

// ---- malformed HTLC conditions ----

#[test]
fn invalid_data_hash_rejected() {
    // A 33-byte compressed pubkey is not a 32-byte hash lock.
    let key = signing_key(74);
    let secret = raw_htlc_secret(&key.public_key().to_hex(), None);
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);
    let err = try_swap(&mut mint, proofs).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

#[test]
fn n_sigs_without_pubkeys_rejected() {
    // Upstream split: its AUTHORING path rejects n_sigs > pubkeys
    // (validate(0)), but its verification path computes required_sigs = 0
    // for a pubkeys-less lock and lets the preimage alone spend. The spec
    // ("exceeds the total number of keys in its pathway ... MUST be
    // rejected as unspendable") wins: we reject; asserted against OUR
    // mint only.
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = raw_htlc_secret(
        &hash,
        Some(vec![vec!["n_sigs".to_string(), "1".to_string()]]),
    );
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode(preimage)), &[]));
    let err = try_swap(&mut mint, proofs).unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);
}

// ---- SIG_ALL ----

#[test]
fn sig_all_preimage_only_spends() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, Some(sig_all_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode(preimage)), &[]));
    assert_swap_differential(&mut mint, proofs, true);
}

#[test]
fn sig_all_wrong_preimage_rejected() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, Some(sig_all_conditions()));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode([9u8; 32])), &[]));
    assert_swap_differential(&mut mint, proofs, false);
}

#[test]
fn sig_all_requires_preimage_and_transaction_signature() {
    let key = signing_key(75);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    )
    .sig_all_msg_to_sign();

    // Transaction signature without the preimage: rejected.
    proofs[0].witness = Some(signatures_only_witness_json(&[sign_hex(&key, &message)]));
    let upstream = upstream_swap_verdict(&proofs, &outputs);
    assert!(!upstream);
    let ours = mint.post_swap(nut03::SwapRequest {
        inputs: proofs.clone(),
        outputs: outputs.clone(),
    });
    assert!(matches!(ours, Err(CashuError::SpendConditionsNotMet)));

    // Preimage + transaction signature: accepted.
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&key, &message)],
    ));
    assert_swap_differential_with(&mut mint, proofs, outputs, true);
}

#[test]
fn sig_all_multisig_2of3_with_preimage() {
    let k1 = signing_key(76);
    let k2 = signing_key(77);
    let k3 = signing_key(78);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![k1.public_key(), k2.public_key(), k3.public_key()]),
        num_sigs: Some(2),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    )
    .sig_all_msg_to_sign();

    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&k1, &message), sign_hex(&k2, &message)],
    ));
    assert_swap_differential_with(&mut mint, proofs, outputs, true);
}

#[test]
fn sig_all_refund_key_spends_after_expiry() {
    // The sender pathway under SIG_ALL with a signatures-only witness —
    // upstream's verify_sig_all_htlc accepts ANY witness shape for this
    // (it extracts signatures without requiring the preimage key), so this
    // is a full differential scenario (and the wire shape the conformance
    // matrix sends).
    let refund_key = signing_key(80);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    )
    .sig_all_msg_to_sign();

    proofs[0].witness = Some(signatures_only_witness_json(&[sign_hex(
        &refund_key,
        &message,
    )]));
    assert_swap_differential_with(&mut mint, proofs, outputs, true);
}

#[test]
fn sig_all_receiver_path_after_locktime() {
    let key = signing_key(81);
    let refund_key = signing_key(82);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        locktime: Some(EXPIRED_LOCKTIME),
        refund_keys: Some(vec![refund_key.public_key()]),
        num_sigs_refund: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);

    let total: u64 = proofs.iter().map(|p| p.amount).sum();
    let outputs = swap_outputs(total);
    let message = CashuSwapRequest::new(
        proofs.iter().map(to_cashu_proof).collect(),
        to_cashu_outputs(&outputs),
    )
    .sig_all_msg_to_sign();

    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&key, &message)],
    ));
    assert_swap_differential_with(&mut mint, proofs, outputs, true);
}

#[test]
fn sig_all_expired_no_refund_anyone_can_spend() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        locktime: Some(EXPIRED_LOCKTIME),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, proofs) = mint_with_secrets(&[(secret, 8)]);
    assert_swap_differential(&mut mint, proofs, true);
}

// ---- melt pathway ----

#[test]
fn melt_htlc_unsigned_fails_then_preimage_spends() {
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let secret = upstream_htlc_secret(&hash, None);

    // Unsigned melt: rejected, proofs stay unspent.
    let (mut mint, proofs) = mint_with_secrets(&[(secret.clone(), 8)]);
    let quote = melt_quote(&mut mint, 8);
    let err = mint
        .post_melt(nut05::MeltRequest {
            quote: quote.clone(),
            inputs: proofs,
            outputs: None,
        })
        .unwrap_err();
    assert_eq!(err, CashuError::SpendConditionsNotMet);

    // Preimage melt on a fresh mint: succeeds.
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let quote = melt_quote(&mut mint, 8);
    proofs[0].witness = Some(htlc_witness_json(Some(&hex::encode(preimage)), &[]));
    let response = mint
        .post_melt(nut05::MeltRequest {
            quote,
            inputs: proofs,
            outputs: None,
        })
        .expect("preimage melt");
    assert!(response.paid);
}

#[test]
fn melt_htlc_sig_all_preimage_and_transaction_signature() {
    let key = signing_key(83);
    let preimage = [7u8; 32];
    let hash = sha256_hex(&preimage);
    let conditions = Conditions {
        pubkeys: Some(vec![key.public_key()]),
        num_sigs: Some(1),
        ..sig_all_conditions()
    };
    let secret = upstream_htlc_secret(&hash, Some(conditions));
    let (mut mint, mut proofs) = mint_with_secrets(&[(secret, 8)]);
    let quote = melt_quote(&mut mint, 8);

    let message = CashuMeltRequest::<String>::new(
        quote.clone(),
        proofs.iter().map(to_cashu_proof).collect(),
        None,
    )
    .sig_all_msg_to_sign();
    proofs[0].witness = Some(htlc_witness_json(
        Some(&hex::encode(preimage)),
        &[sign_hex(&key, &message)],
    ));
    let cashu_request = CashuMeltRequest::<String>::new(
        quote.clone(),
        proofs.iter().map(to_cashu_proof).collect(),
        None,
    );
    assert!(cashu_request.verify_spending_conditions().is_ok());

    let response = mint
        .post_melt(nut05::MeltRequest {
            quote,
            inputs: proofs,
            outputs: None,
        })
        .expect("our mint accepts the upstream SIG_ALL HTLC melt");
    assert!(response.paid);
}
