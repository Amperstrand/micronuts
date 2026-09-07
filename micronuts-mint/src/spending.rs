//! NUT-10/11/14 spending-condition enforcement for swap and melt inputs.
//!
//! Sequencing rule (roadmap #51): [`verify_spending_conditions`] runs on
//! the RAW inputs BEFORE [`crate::DemoMint`] calls `claim_proofs`, so a
//! failed witness leaves the proofs unspent and the whole request atomic.
//!
//! Semantics mirror the upstream `cashu` crate 0.18 (the differential
//! oracle; see tests/p2pk_differential.rs and tests/htlc_differential.rs):
//! - Non-condition secrets are skipped (anyone-can-spend, NUT-10 Caution).
//! - A proof carrying a witness whose secret is NOT a condition is
//!   rejected (upstream `IncorrectWitnessKind`).
//! - SIG_INPUTS (default): every locked input needs its own witness
//!   signing its own `secret` string.
//! - SIG_ALL: every input must carry the SAME condition (kind, data and
//!   tags); only the FIRST input's witness is read, signing the aggregated
//!   secret‖C‖amount‖B_‖(quote) message.
//! - P2PK locktime (L2): before expiry only the primary pathway can spend;
//!   after expiry the refund pathway opens (refund keys, `n_sigs_refund`
//!   with x-dedup) while the primary pathway REMAINS available — the
//!   refund path is additional (NUT-11 §Refund Multisig). Expired with no
//!   refund keys is anyone-can-spend.
//! - HTLC (L3): the receiver pathway (valid preimage + `pubkeys`
//!   signatures) is always available; after expiry the sender pathway
//!   (refund keys, no preimage needed) opens; expired with no refund keys
//!   is anyone-can-spend.
//!
//! Time comes in as `now` (unix seconds) from the mint's injectable
//! [`crate::ln::MintClock`] — tests freeze time with `MockClock`, never by
//! sleeping.

use std::collections::HashSet;
use std::str::FromStr;

use bitcoin::secp256k1::schnorr::Signature;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::PublicKey as LitePublicKey;
use cashu_core_lite::nuts::nut00::{BlindedMessage, Proof};
use cashu_core_lite::nuts::nut10::{self, SecretKind};
use cashu_core_lite::nuts::nut11::{P2pkConditions, P2pkWitness, SigFlag, SpendingRequirements};
use cashu_core_lite::nuts::nut14::{HtlcConditions, HtlcWitness};

use crate::type_conversion::lite_pk_to_cashu;

/// A swap/melt input whose secret parsed as a spending condition.
enum InputLock<'a> {
    P2pk {
        proof: &'a Proof,
        secret: nut10::Secret,
        conditions: P2pkConditions,
    },
    Htlc {
        proof: &'a Proof,
        secret: nut10::Secret,
        conditions: HtlcConditions,
    },
}

impl InputLock<'_> {
    fn proof(&self) -> &Proof {
        match self {
            InputLock::P2pk { proof, .. } | InputLock::Htlc { proof, .. } => proof,
        }
    }

    fn secret(&self) -> &nut10::Secret {
        match self {
            InputLock::P2pk { secret, .. } | InputLock::Htlc { secret, .. } => secret,
        }
    }

    fn sigflag(&self) -> SigFlag {
        match self {
            InputLock::P2pk { conditions, .. } => conditions.sigflag,
            InputLock::Htlc { conditions, .. } => conditions.sigflag,
        }
    }

    fn requirements_at(&self, now: u64) -> SpendingRequirements {
        match self {
            InputLock::P2pk { conditions, .. } => conditions.requirements_at(now),
            InputLock::Htlc { conditions, .. } => conditions.requirements_at(now),
        }
    }
}

// NUT #11: Spending conditions are defined for each individual `Proof` and not on a transaction level that can consist of multiple `Proofs`. Similarly, spending conditions must be satisfied by providing signatures or additional witness data for each `Proof` separately. For a transaction to be valid, all `Proofs` in that transaction must be unlocked successfully.
//
/// Verify every spending condition on a transaction's inputs. `outputs` is
/// the request's blinded messages (as sent) and `quote_id` the melt quote —
/// both only feed the SIG_ALL aggregated message. `now` is the mint's clock
/// (unix seconds) and gates every locktime pathway.
pub(crate) fn verify_spending_conditions(
    inputs: &[Proof],
    outputs: Option<&[BlindedMessage]>,
    quote_id: Option<&str>,
    now: u64,
) -> Result<(), CashuError> {
    let mut locks: Vec<InputLock<'_>> = Vec::new();
    let mut saw_sig_all = false;

    for proof in inputs {
        match nut10::Secret::parse(&proof.secret) {
            None => {
                // NUT #10: Caution: If the mint does not support spending conditions or a specific `kind` of spending condition, proofs may be treated as a regular anyone-can-spend tokens.
                // A witness on a non-condition proof is nonsense and rejected
                // (upstream `IncorrectWitnessKind`).
                if proof.witness.is_some() {
                    return Err(CashuError::SpendConditionsNotMet);
                }
            }
            Some(secret) => match secret.kind {
                SecretKind::P2PK => {
                    let conditions = P2pkConditions::from_secret(&secret)
                        .map_err(|_| CashuError::SpendConditionsNotMet)?;
                    saw_sig_all |= conditions.sigflag == SigFlag::SigAll;
                    locks.push(InputLock::P2pk {
                        proof,
                        secret,
                        conditions,
                    });
                }
                SecretKind::Htlc => {
                    let conditions = HtlcConditions::from_secret(&secret)
                        .map_err(|_| CashuError::SpendConditionsNotMet)?;
                    saw_sig_all |= conditions.sigflag == SigFlag::SigAll;
                    locks.push(InputLock::Htlc {
                        proof,
                        secret,
                        conditions,
                    });
                }
            },
        }
    }

    if saw_sig_all {
        verify_sig_all(inputs, &locks, outputs, quote_id, now)
    } else {
        verify_inputs_individually(&locks, now)
    }
}

/// SIG_INPUTS (and mixed/unlocked inputs): each locked proof is verified
/// independently against its own secret string.
// NUT #11: `SIG_INPUTS` requires valid signatures on all inputs independently. It is the default signature flag and will be applied if the `sigflag` tag is absent.
// NUT #11: `SIG_INPUTS` means that each `Proof` (input) requires its own signature. The signature is provided in the `Proof.witness` field of each input separately.
fn verify_inputs_individually(locks: &[InputLock<'_>], now: u64) -> Result<(), CashuError> {
    for lock in locks {
        // NUT #11: The `secret` field is **signed as a string**.
        let message = lock.proof().secret.as_bytes();
        let requirements = lock.requirements_at(now);
        match lock {
            InputLock::P2pk { proof, .. } => {
                let anyone_can_spend = requirements
                    .refund_path
                    .as_ref()
                    .is_some_and(|refund| refund.is_anyone_can_spend());
                if !anyone_can_spend {
                    let witness = proof
                        .witness
                        .as_deref()
                        .and_then(P2pkWitness::from_json)
                        .ok_or(CashuError::SpendConditionsNotMet)?;
                    verify_p2pk_pathways(message, &requirements, &witness.signatures)?;
                }
            }
            InputLock::Htlc {
                proof, conditions, ..
            } => {
                let witness = proof.witness.as_deref().and_then(HtlcWitness::from_json);
                let preimage_valid = witness
                    .as_ref()
                    .map(|w| {
                        w.preimage
                            .as_deref()
                            .is_some_and(|preimage| conditions.matches_preimage(preimage))
                    })
                    .unwrap_or(false);
                verify_htlc_pathways(message, &requirements, witness.as_ref(), preimage_valid)?;
            }
        }
    }
    Ok(())
}

/// P2PK: the primary pathway is tried first and, when the lock has expired,
/// the refund pathway additionally (count-based, mirroring upstream's
/// try-primary-then-refund — a failed primary must not block a valid
/// refund signature).
fn verify_p2pk_pathways(
    message: &[u8],
    requirements: &SpendingRequirements,
    signature_hexes: &[String],
) -> Result<(), CashuError> {
    if let Some(primary) = count_valid_signatures(message, &requirements.pubkeys, signature_hexes) {
        if primary >= requirements.required_sigs {
            return Ok(());
        }
    }
    if let Some(refund) = &requirements.refund_path {
        if let Some(refund_count) =
            count_valid_signatures(message, &refund.pubkeys, signature_hexes)
        {
            if refund_count >= refund.required_sigs {
                return Ok(());
            }
        }
    }
    Err(CashuError::SpendConditionsNotMet)
}

/// HTLC: receiver pathway (valid preimage + `pubkeys` signatures) is always
/// available; otherwise the sender/refund pathway after expiry. Signature
/// failures here are terminal (upstream propagates them with `?` instead of
/// falling through to the refund pathway).
// NUT #14: The receiver(s) listed in the `pubkeys` tag can spend the proof by providing **BOTH** of the following:
// NUT #14: The sender(s) listed in the `refund` tag can spend the proof once the `locktime` lock has "expired" by providing signature(s) as per the [NUT-11][11] rules for **Refund MultiSig**.
// Note: upstream divergence — cashu 0.18.0's untagged witness enum parses a
// Note: signatures-only witness as P2PKWitness, so its `verify_htlc`
// Note: rejects the sender pathway spent with `{"signatures":[…]}` (no
// Note: preimage key) unless the lock is anyone-can-spend; only a witness
// Note: carrying a (possibly wrong) preimage key reaches the refund check.
// Note: NUT-14 defines the sender pathway as "providing signature(s) as per
// Note: the NUT-11 rules", whose witness is the signatures array — the
// Note: local spec files win: a signatures-only witness may spend an
// Note: expired HTLC via the refund keys.
fn verify_htlc_pathways(
    message: &[u8],
    requirements: &SpendingRequirements,
    witness: Option<&HtlcWitness>,
    preimage_valid: bool,
) -> Result<(), CashuError> {
    if !preimage_valid {
        if let Some(refund) = &requirements.refund_path {
            if refund.is_anyone_can_spend() {
                // Preimage absent/invalid, lock expired, no refund keys.
                return Ok(());
            }
        }
    }
    if preimage_valid {
        // Receiver pathway: an absent pubkeys tag means the preimage alone
        // spends (required_sigs == 0 in that case).
        if requirements.required_sigs == 0 {
            return Ok(());
        }
        let signatures = witness
            .and_then(|w| w.signatures.as_deref())
            .ok_or(CashuError::SpendConditionsNotMet)?;
        verify_witness_signatures(
            message,
            &requirements.pubkeys,
            signatures,
            requirements.required_sigs,
        )
    } else if let Some(refund) = &requirements.refund_path {
        let signatures = witness
            .and_then(|w| w.signatures.as_deref())
            .ok_or(CashuError::SpendConditionsNotMet)?;
        verify_witness_signatures(message, &refund.pubkeys, signatures, refund.required_sigs)
    } else {
        Err(CashuError::SpendConditionsNotMet)
    }
}

/// SIG_ALL: all inputs must share one condition (any kind), and only the
/// first input's witness is checked against the aggregated message.
// NUT #11: `SIG_ALL` requires valid signatures on all inputs and on all outputs of a transaction.
// NUT #11: If one input has the signature flag `SIG_ALL`, all other inputs MUST have the same `Secret.data` and `Secret.tags`, and by extension, also be `SIG_ALL`.
// NUT #11: If this condition is met, the `SIG_ALL` flag is enforced and only **the first input of a transaction requires a witness** that covers all other inputs and outputs of the transaction. All signatures by the signing public keys MUST be provided in the `Proof.witness` of the first input of the transaction.
fn verify_sig_all(
    inputs: &[Proof],
    locks: &[InputLock<'_>],
    outputs: Option<&[BlindedMessage]>,
    quote_id: Option<&str>,
    now: u64,
) -> Result<(), CashuError> {
    // Every input must be locked (upstream requires every input to parse as
    // the same condition; a plain input mixed into a SIG_ALL transaction
    // fails there too), and the first input must itself be SIG_ALL.
    if locks.len() != inputs.len() || locks[0].sigflag() != SigFlag::SigAll {
        return Err(CashuError::SpendConditionsNotMet);
    }
    let first = &locks[0];
    for lock in &locks[1..] {
        if lock.secret().kind != first.secret().kind
            || lock.secret().data != first.secret().data
            || lock.secret().tags != first.secret().tags
        {
            return Err(CashuError::SpendConditionsNotMet);
        }
    }

    let message = sig_all_message(inputs, outputs, quote_id);
    let requirements = first.requirements_at(now);
    match first {
        InputLock::P2pk { .. } => {
            if let Some(refund) = &requirements.refund_path {
                if refund.is_anyone_can_spend() {
                    // Expired lock with no refund keys.
                    return Ok(());
                }
            }
            let witness = inputs[0]
                .witness
                .as_deref()
                .and_then(P2pkWitness::from_json)
                .ok_or(CashuError::SpendConditionsNotMet)?;
            verify_p2pk_pathways(message.as_bytes(), &requirements, &witness.signatures)
        }
        InputLock::Htlc { conditions, .. } => {
            let witness = inputs[0]
                .witness
                .as_deref()
                .and_then(HtlcWitness::from_json);
            let preimage_valid = witness
                .as_ref()
                .map(|w| {
                    w.preimage
                        .as_deref()
                        .is_some_and(|preimage| conditions.matches_preimage(preimage))
                })
                .unwrap_or(false);
            // Unlike the SIG_INPUTS path, upstream's `verify_sig_all_htlc`
            // accepts a signatures-only (P2PK-shape) witness for the sender
            // pathway, so the generic parse needs no divergence note here.
            verify_htlc_pathways(
                message.as_bytes(),
                &requirements,
                witness.as_ref(),
                preimage_valid,
            )
        }
    }
}

/// The SIG_ALL aggregated message.
// NUT #11: A swap contains `inputs` and `outputs` (see [NUT-03][03]). To provide a valid signature, the owner (or owners) of the signing public keys must concatenate the `secret` and `C` fields of all `Proofs` (inputs) with the `amount` and `B_` fields of all `BlindedMessages` (outputs, see [NUT-00][00]) to a single message string in the order they appear in the transaction. This concatenated string is then hashed and signed (see [Signature scheme](#signature-scheme)).
// NUT #11: If a swap transaction has `n` inputs and `m` outputs, the message to sign becomes:
// NUT #11: msg = secret_0 || C_0 || ... || secret_n || C_n || amount_0 ||  B_0 || ... || amount_m || B_m
// NUT #11: Here, `||` denotes string concatenation. The `C` of each input and `B_` of each output are **hex strings** and `amount` is a UTF-encoded string.
// NUT #11: For a melt transaction, the message to sign is composed of all the inputs, the quote ID being paid, and the [NUT-08][08] blank `outputs`.
// NUT #11: If a melt transaction has `n` inputs, `m` blank outputs, and a quote ID `quote_id`, the message to sign becomes:
// NUT #11: msg = secret_0 || C_0 || ... || secret_n || C_n || amount_0 || B_0 || ... || amount_m || B_m || quote_id
fn sig_all_message(
    inputs: &[Proof],
    outputs: Option<&[BlindedMessage]>,
    quote_id: Option<&str>,
) -> String {
    let mut message = String::new();
    for proof in inputs {
        message.push_str(&proof.secret);
        message.push_str(&hex::encode(proof.c.to_bytes()));
    }
    if let Some(outputs) = outputs {
        for output in outputs {
            message.push_str(&output.amount.to_string());
            message.push_str(&hex::encode(output.b.to_bytes()));
        }
    }
    if let Some(quote_id) = quote_id {
        message.push_str(quote_id);
    }
    message
}

/// Count distinct locked keys (by x-coordinate) with a valid signature.
/// `None` when any signature is unparseable or a key verifies twice
/// (upstream `InvalidSignature` / `DuplicateSignature` — both make the
/// PATHWAY invalid, not the whole request, so the caller may still try the
/// other pathway).
// NUT #11: Because Schnorr signatures are non-deterministic (due to auxiliary random data), we expect a minimum number of unique public keys with valid signatures instead of expecting a minimum number of signatures.
fn count_valid_signatures(
    message: &[u8],
    locked_pubkeys: &[LitePublicKey],
    signature_hexes: &[String],
) -> Option<u64> {
    let signatures: Vec<Signature> = signature_hexes
        .iter()
        .map(|hex_str| Signature::from_str(hex_str))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    let mut verified_x_coordinates: HashSet<[u8; 32]> = HashSet::new();
    for locked in locked_pubkeys {
        let cashu_pubkey = lite_pk_to_cashu(locked);
        for signature in &signatures {
            if cashu_pubkey.verify(message, signature).is_ok() {
                let compressed = locked.to_bytes();
                let mut x = [0u8; 32];
                x.copy_from_slice(&compressed[1..33]);
                if !verified_x_coordinates.insert(x) {
                    // Same key verified twice — duplicate signature.
                    return None;
                }
            }
        }
    }

    Some(verified_x_coordinates.len() as u64)
}

/// Require `required` distinct locked keys with a valid signature; any
/// unparseable signature or duplicate verification fails the check outright.
fn verify_witness_signatures(
    message: &[u8],
    locked_pubkeys: &[LitePublicKey],
    signature_hexes: &[String],
    required: u64,
) -> Result<(), CashuError> {
    match count_valid_signatures(message, locked_pubkeys, signature_hexes) {
        Some(count) if count >= required => Ok(()),
        _ => Err(CashuError::SpendConditionsNotMet),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(seed: u8) -> LitePublicKey {
        let sk = cashu::nuts::nut01::SecretKey::from_slice(&[seed; 32]).unwrap();
        crate::type_conversion::cashu_pk_to_lite(&sk.public_key())
    }

    /// The witness must cover the exact secret string bytes.
    #[test]
    fn sig_all_message_shape_matches_spec() {
        let proof = Proof {
            amount: 8,
            id: "00".to_string(),
            secret: "secret-0".to_string(),
            c: point(2),
            dleq: None,
            witness: None,
        };
        let output = BlindedMessage {
            amount: 4,
            id: "00".to_string(),
            b: point(3),
        };
        let swap_msg = sig_all_message(
            std::slice::from_ref(&proof),
            Some(std::slice::from_ref(&output)),
            None,
        );
        assert_eq!(
            swap_msg,
            format!(
                "secret-0{}4{}",
                hex::encode(point(2).to_bytes()),
                hex::encode(point(3).to_bytes())
            )
        );
        let melt_msg = sig_all_message(&[proof], Some(&[output]), Some("quote-1"));
        assert!(melt_msg.ends_with(&format!("4{}quote-1", hex::encode(point(3).to_bytes()))));
        // No outputs on a melt: inputs then quote id directly.
        let bare = sig_all_message(&[], None, Some("q"));
        assert_eq!(bare, "q");
    }
}
