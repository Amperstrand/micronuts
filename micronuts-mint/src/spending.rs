//! NUT-10/11 spending-condition enforcement for swap and melt inputs.
//!
//! Sequencing rule (roadmap #51): [`verify_spending_conditions`] runs on
//! the RAW inputs BEFORE [`crate::DemoMint`] calls `claim_proofs`, so a
//! failed witness leaves the proofs unspent and the whole request atomic.
//!
//! Semantics mirror the upstream `cashu` crate 0.18 (the differential
//! oracle; see tests/p2pk_differential.rs):
//! - Non-condition secrets are skipped (anyone-can-spend, NUT-10 Caution).
//! - A proof carrying a witness whose secret is NOT a condition is
//!   rejected (upstream `IncorrectWitnessKind`).
//! - HTLC-kind secrets are rejected as unsupported until L3.
//! - SIG_INPUTS (default): every locked input needs its own witness
//!   signing its own `secret` string.
//! - SIG_ALL: every input must carry the SAME P2PK SIG_ALL condition; only
//!   the FIRST input's witness is read, signing the aggregated
//!   secret‖C‖amount‖B_‖(quote) message.
//!
//! L1 scope: the locktime/refund pathway is parsed but not enforced — the
//! lock is treated as permanent, so refund keys can never spend here (L2).

use std::collections::HashSet;
use std::str::FromStr;

use bitcoin::secp256k1::schnorr::Signature;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::PublicKey as LitePublicKey;
use cashu_core_lite::nuts::nut00::{BlindedMessage, Proof};
use cashu_core_lite::nuts::nut10::{self, SecretKind};
use cashu_core_lite::nuts::nut11::{P2pkConditions, P2pkWitness, SigFlag};

use crate::type_conversion::lite_pk_to_cashu;

/// A swap/melt input whose secret parsed as a P2PK spending condition.
struct InputLock<'a> {
    proof: &'a Proof,
    secret: nut10::Secret,
    conditions: P2pkConditions,
}

// NUT #11: Spending conditions are defined for each individual `Proof` and not on a transaction level that can consist of multiple `Proofs`. Similarly, spending conditions must be satisfied by providing signatures or additional witness data for each `Proof` separately. For a transaction to be valid, all `Proofs` in that transaction must be unlocked successfully.
//
/// Verify every spending condition on a transaction's inputs. `outputs` is
/// the request's blinded messages (as sent) and `quote_id` the melt quote —
/// both only feed the SIG_ALL aggregated message.
pub(crate) fn verify_spending_conditions(
    inputs: &[Proof],
    outputs: Option<&[BlindedMessage]>,
    quote_id: Option<&str>,
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
                SecretKind::Htlc => {
                    // HTLC enforcement is L3 (roadmap #51): reject as
                    // unspendable rather than treating it anyone-can-spend,
                    // which is the closest safe approximation of upstream
                    // (whose HTLC verification also fails without a
                    // preimage witness).
                    return Err(CashuError::SpendConditionsNotMet);
                }
                SecretKind::P2PK => {
                    let conditions = P2pkConditions::from_secret(&secret)
                        .map_err(|_| CashuError::SpendConditionsNotMet)?;
                    saw_sig_all |= conditions.sigflag == SigFlag::SigAll;
                    locks.push(InputLock {
                        proof,
                        secret,
                        conditions,
                    });
                }
            },
        }
    }

    if saw_sig_all {
        verify_sig_all(inputs, &locks, outputs, quote_id)
    } else {
        verify_inputs_individually(&locks)
    }
}

/// SIG_INPUTS (and mixed/unlocked inputs): each locked proof is verified
/// independently against its own secret string.
// NUT #11: `SIG_INPUTS` requires valid signatures on all inputs independently. It is the default signature flag and will be applied if the `sigflag` tag is absent.
// NUT #11: `SIG_INPUTS` means that each `Proof` (input) requires its own signature. The signature is provided in the `Proof.witness` field of each input separately.
fn verify_inputs_individually(locks: &[InputLock<'_>]) -> Result<(), CashuError> {
    for lock in locks {
        // NUT #11: The `secret` field is **signed as a string**.
        let witness = lock
            .proof
            .witness
            .as_deref()
            .and_then(P2pkWitness::from_json)
            .ok_or(CashuError::SpendConditionsNotMet)?;
        verify_witness_signatures(
            lock.proof.secret.as_bytes(),
            &lock.conditions.signing_pubkeys(),
            &witness.signatures,
            lock.conditions.required_sigs(),
        )?;
    }
    Ok(())
}

/// SIG_ALL: all inputs must share one P2PK SIG_ALL condition, and only the
/// first input's witness is checked against the aggregated message.
// NUT #11: `SIG_ALL` requires valid signatures on all inputs and on all outputs of a transaction.
// NUT #11: If one input has the signature flag `SIG_ALL`, all other inputs MUST have the same `Secret.data` and `Secret.tags`, and by extension, also be `SIG_ALL`.
// NUT #11: If this condition is met, the `SIG_ALL` flag is enforced and only **the first input of a transaction requires a witness** that covers all other inputs and outputs of the transaction. All signatures by the signing public keys MUST be provided in the `Proof.witness` of the first input of the transaction.
fn verify_sig_all(
    inputs: &[Proof],
    locks: &[InputLock<'_>],
    outputs: Option<&[BlindedMessage]>,
    quote_id: Option<&str>,
) -> Result<(), CashuError> {
    // Every input must be locked (upstream requires every input to parse as
    // the same condition; a plain input mixed into a SIG_ALL transaction
    // fails there too), and the first input must itself be SIG_ALL.
    if locks.len() != inputs.len() || locks[0].conditions.sigflag != SigFlag::SigAll {
        return Err(CashuError::SpendConditionsNotMet);
    }
    let first = &locks[0];
    for lock in &locks[1..] {
        if lock.secret.data != first.secret.data || lock.secret.tags != first.secret.tags {
            return Err(CashuError::SpendConditionsNotMet);
        }
    }

    let message = sig_all_message(inputs, outputs, quote_id);
    let witness = inputs[0]
        .witness
        .as_deref()
        .and_then(P2pkWitness::from_json)
        .ok_or(CashuError::SpendConditionsNotMet)?;
    verify_witness_signatures(
        message.as_bytes(),
        &first.conditions.signing_pubkeys(),
        &witness.signatures,
        first.conditions.required_sigs(),
    )
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

/// Count distinct locked keys (by x-coordinate) with a valid signature and
/// require `required` of them. Any unparseable signature fails the check,
/// and a key presenting TWO valid signatures is rejected (upstream
/// `DuplicateSignature`).
// NUT #11: Because Schnorr signatures are non-deterministic (due to auxiliary random data), we expect a minimum number of unique public keys with valid signatures instead of expecting a minimum number of signatures.
fn verify_witness_signatures(
    message: &[u8],
    locked_pubkeys: &[LitePublicKey],
    signature_hexes: &[String],
    required: u64,
) -> Result<(), CashuError> {
    let signatures: Vec<Signature> = signature_hexes
        .iter()
        .map(|hex_str| Signature::from_str(hex_str))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CashuError::SpendConditionsNotMet)?;

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
                    return Err(CashuError::SpendConditionsNotMet);
                }
            }
        }
    }

    if verified_x_coordinates.len() as u64 >= required {
        Ok(())
    } else {
        Err(CashuError::SpendConditionsNotMet)
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
