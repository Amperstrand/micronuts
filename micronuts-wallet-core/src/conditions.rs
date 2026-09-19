//! Wallet-side NUT-10 spending conditions: the deterministic P2PK
//! identity key and witness construction for condition-locked proofs.
//!
//! The identity key is a BIP-340 signing scalar derived from the wallet
//! seed in a domain disjoint from NUT-13 token-secret derivation
//! (`hmac(seed, keyset_id‖counter)` there, `hmac(seed, IDENTITY_LABEL)`
//! here), so revealing the P2PK pubkey leaks nothing about future
//! deterministic secrets. Signatures are deterministic
//! (`sign_prehash_with_aux_rand` with a zero aux) — no RNG on the spend
//! path.
//!
//! Message convention (matches the mint's verification, which delegates
//! to the upstream `cashu` crate, and the Python audit builders): the
//! BIP-340 message is `sha256(secret_utf8_bytes)`.

use cashu_core_lite::nuts::nut10::{Secret, SecretKind};
use cashu_core_lite::nuts::nut11::{P2pkConditions, P2pkWitness, SigFlag, SpendingRequirements};
use cashu_core_lite::nuts::nut14::{HtlcConditions, HtlcWitness};
use k256::schnorr::{Signature, SigningKey};
use sha2::{Digest, Sha256};

/// The HMAC message that separates the identity scalar from every other
/// derivation domain over the same seed.
pub const IDENTITY_LABEL: &[u8] = b"micronuts/nut11/p2pk-identity/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionError {
    /// SIG_ALL aggregates every input and output into one message — the
    /// wallet does not build aggregated witnesses yet.
    SigAllUnsupported,
    /// The lock demands more distinct-key signatures than the single
    /// identity key can produce.
    MultisigUnsupported { required: u64 },
    /// The lock needs a hash-lock preimage and none was supplied.
    PreimageMissing,
    /// The supplied preimage does not hash to the lock.
    PreimageMismatch,
    /// No spending pathway of this lock involves the wallet's key.
    NotOurLock,
    /// The secret parses as a condition but violates NUT-11/14 rules;
    /// the mint will reject the proof as unspendable.
    MalformedSecret,
    /// Identity derivation hit a zero scalar (probability ~2⁻¹²⁸).
    DerivationFailed,
    /// Schnorr signing failed (zero nonce; probability ~2⁻²⁵⁶).
    SigningFailed,
}

impl core::fmt::Display for ConditionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SigAllUnsupported => write!(f, "SIG_ALL receive is not supported yet"),
            Self::MultisigUnsupported { required } => write!(
                f,
                "lock requires {required} signatures; the wallet holds one identity key"
            ),
            Self::PreimageMissing => write!(f, "HTLC lock needs a preimage to receive"),
            Self::PreimageMismatch => write!(f, "preimage does not hash to the HTLC lock"),
            Self::NotOurLock => write!(f, "locked to a key this wallet does not hold"),
            Self::MalformedSecret => write!(f, "malformed spending condition"),
            Self::DerivationFailed => write!(f, "identity derivation failed"),
            Self::SigningFailed => write!(f, "schnorr signing failed"),
        }
    }
}

/// The wallet's deterministic P2PK identity: `hmac_sha256(seed, label)`
/// reduced to a BIP-340 signing scalar.
pub fn derive_identity(seed: &[u8; 32]) -> Result<SigningKey, ConditionError> {
    let scalar = hmac_sha256(seed, IDENTITY_LABEL);
    SigningKey::from_bytes(&scalar).map_err(|_| ConditionError::DerivationFailed)
}

/// The lock-facing public key: 33-byte compressed hex, the format NUT-11
/// `data` and the `pubkeys`/`refund` tags use.
pub fn p2pk_pubkey_hex(identity: &SigningKey) -> String {
    let public: k256::PublicKey = identity.verifying_key().into();
    hex::encode(k256::EncodedPoint::from(&public).as_bytes())
}

/// Everything a witness may depend on when unlocking one proof.
pub struct UnlockContext<'a> {
    pub identity: &'a SigningKey,
    pub preimage: Option<&'a str>,
    /// Unix seconds; refund pathways open strictly after `locktime`.
    pub now: u64,
}

/// Build the `Proof.witness` stringified JSON for one input secret:
/// `Ok(None)` when the input spends without a witness (an ordinary
/// secret, or an expired lock with no refund keys — anyone-can-spend).
pub fn witness_for_secret(
    secret: &str,
    ctx: &UnlockContext<'_>,
) -> Result<Option<String>, ConditionError> {
    let Some(parsed) = Secret::parse(secret) else {
        return Ok(None);
    };
    match parsed.kind {
        SecretKind::P2PK => p2pk_witness(secret, &parsed, ctx),
        SecretKind::Htlc => htlc_witness(secret, &parsed, ctx),
    }
}

fn p2pk_witness(
    raw: &str,
    secret: &Secret,
    ctx: &UnlockContext<'_>,
) -> Result<Option<String>, ConditionError> {
    let conditions =
        P2pkConditions::from_secret(secret).map_err(|_| ConditionError::MalformedSecret)?;
    if conditions.sigflag == SigFlag::SigAll {
        return Err(ConditionError::SigAllUnsupported);
    }
    let requirements = conditions.requirements_at(ctx.now);
    if is_anyone_can_spend(&requirements) {
        return Ok(None);
    }
    if path_holds_identity(&requirements.pubkeys, ctx.identity) {
        return match requirements.required_sigs {
            0 => Ok(None),
            1 => Ok(Some(
                P2pkWitness {
                    signatures: vec![sign_identity(ctx.identity, raw)?],
                }
                .to_json(),
            )),
            required => Err(ConditionError::MultisigUnsupported { required }),
        };
    }
    if let Some(refund) = &requirements.refund_path {
        if refund.required_sigs == 1 && path_holds_identity(&refund.pubkeys, ctx.identity) {
            return Ok(Some(
                P2pkWitness {
                    signatures: vec![sign_identity(ctx.identity, raw)?],
                }
                .to_json(),
            ));
        }
        if refund.required_sigs > 1 && path_holds_identity(&refund.pubkeys, ctx.identity) {
            return Err(ConditionError::MultisigUnsupported {
                required: refund.required_sigs,
            });
        }
    }
    Err(ConditionError::NotOurLock)
}

fn htlc_witness(
    raw: &str,
    secret: &Secret,
    ctx: &UnlockContext<'_>,
) -> Result<Option<String>, ConditionError> {
    let conditions =
        HtlcConditions::from_secret(secret).map_err(|_| ConditionError::MalformedSecret)?;
    if conditions.sigflag == SigFlag::SigAll {
        return Err(ConditionError::SigAllUnsupported);
    }
    let requirements = conditions.requirements_at(ctx.now);
    let Some(preimage) = ctx.preimage else {
        if is_anyone_can_spend(&requirements) {
            return Ok(None);
        }
        return Err(ConditionError::PreimageMissing);
    };
    if !conditions.matches_preimage(preimage) {
        return Err(ConditionError::PreimageMismatch);
    }
    let signatures = match requirements.required_sigs {
        0 => None,
        1 if path_holds_identity(&requirements.pubkeys, ctx.identity) => {
            Some(vec![sign_identity(ctx.identity, raw)?])
        }
        1 => return Err(ConditionError::NotOurLock),
        required => return Err(ConditionError::MultisigUnsupported { required }),
    };
    Ok(Some(
        HtlcWitness {
            preimage: Some(preimage.to_string()),
            signatures,
        }
        .to_json(),
    ))
}

fn sign_identity(identity: &SigningKey, secret: &str) -> Result<String, ConditionError> {
    let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
    let signature: Signature = identity
        .sign_prehash_with_aux_rand(&digest, &[0u8; 32])
        .map_err(|_| ConditionError::SigningFailed)?;
    Ok(hex::encode(signature.to_bytes()))
}

fn is_anyone_can_spend(requirements: &SpendingRequirements) -> bool {
    requirements
        .refund_path
        .as_ref()
        .is_some_and(|refund| refund.is_anyone_can_spend())
}

fn path_holds_identity(path: &[cashu_core_lite::PublicKey], identity: &SigningKey) -> bool {
    let identity_x = identity.verifying_key().to_bytes();
    path.iter()
        .any(|key| key.to_bytes()[1..33] == identity_x[..])
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // Mirrors cashu-core-lite's nut13 helper (SHA-256-inner HMAC).
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let inner_pad: Vec<u8> = key_block.iter().map(|b| b ^ 0x36).collect();
    let outer_pad: Vec<u8> = key_block.iter().map(|b| b ^ 0x5c).collect();
    let inner = Sha256::new().chain_update(&inner_pad).chain_update(message);
    let outer = Sha256::new()
        .chain_update(&outer_pad)
        .chain_update(inner.finalize());
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::signature::hazmat::PrehashVerifier;
    use std::format;

    const NOW: u64 = 1_000_000_000;

    fn p2pk_secret(data: &str, tags: &str) -> String {
        format!(r#"["P2PK",{{"nonce":"t","data":"{data}","tags":{tags}}}]"#)
    }

    #[test]
    fn identity_is_deterministic_and_seed_bound() {
        let a = derive_identity(&[7u8; 32]).unwrap();
        let b = derive_identity(&[7u8; 32]).unwrap();
        let c = derive_identity(&[8u8; 32]).unwrap();
        assert_eq!(a.to_bytes(), b.to_bytes());
        assert_ne!(a.to_bytes(), c.to_bytes());
    }

    #[test]
    fn pubkey_hex_is_33_byte_compressed() {
        let identity = derive_identity(&[7u8; 32]).unwrap();
        let hex_str = p2pk_pubkey_hex(&identity);
        assert_eq!(hex_str.len(), 66);
        assert!(hex_str.starts_with("02") || hex_str.starts_with("03"));
        // Round-trips through the core pubkey parser locks actually use.
        let bytes = hex::decode(&hex_str).unwrap();
        let compressed: [u8; 33] = bytes.try_into().unwrap();
        assert!(cashu_core_lite::PublicKey::from_bytes(&compressed).is_some());
    }

    #[test]
    fn identity_domain_is_disjoint_from_nut13_secrets() {
        let seed = [7u8; 32];
        let identity_scalar = derive_identity(&seed).unwrap().to_bytes();
        let nut13_secret =
            cashu_core_lite::nuts::nut13::derive_secret(&seed, "0022e025867793d1", 0).unwrap();
        assert_ne!(identity_scalar[..], nut13_secret[..]);
    }

    #[test]
    fn bip340_cross_verification_with_libsecp_vector() {
        // Vector generated by coincurve (libsecp256k1, even-y key): proves
        // k256's BIP-340 verification accepts libsecp signatures over the
        // sha256(secret) convention the mint enforces.
        let pub_hex = "024d4b6cd1361032ca9bd2aeb9d900aa4d45d9ead80ac9423374c451a7254d0766";
        let digest: [u8; 32] =
            hex::decode("49ab82437fe8dfc7d2b5f88f7608975d22aac1c2046eb7b1f165d986f9fa6dc6")
                .unwrap()
                .try_into()
                .unwrap();
        let sig_bytes: [u8; 64] =
            hex::decode("418cc84562019cf7be6304fa344e29553ae5931ef9857d02af663be4cfdb7c24f0a1bab49a9e98157018f42ed77c2d05b1c2f74a0ce060eb8b1b5e462f33ca86")
                .unwrap()
                .try_into()
                .unwrap();
        let verifying: k256::PublicKey =
            k256::PublicKey::from_sec1_bytes(&hex::decode(pub_hex).unwrap()).unwrap();
        let schnorr_vk = k256::schnorr::VerifyingKey::try_from(verifying).unwrap();
        let sig = Signature::try_from(sig_bytes.as_slice()).unwrap();
        schnorr_vk
            .verify_prehash(&digest, &sig)
            .expect("k256 must accept the libsecp signature");
    }

    #[test]
    fn our_signature_verifies_against_the_convention() {
        let identity = derive_identity(&[7u8; 32]).unwrap();
        let secret = p2pk_secret(&p2pk_pubkey_hex(&identity), r#"[["sigflag","SIG_INPUTS"]]"#);
        let sig_hex = sign_identity(&identity, &secret).unwrap();
        let sig = Signature::try_from(hex::decode(&sig_hex).unwrap().as_slice()).unwrap();
        let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        identity
            .verifying_key()
            .verify_prehash(&digest, &sig)
            .expect("own signature must verify");
    }

    struct Case {
        identity: SigningKey,
    }

    fn case() -> Case {
        Case {
            identity: derive_identity(&[42u8; 32]).unwrap(),
        }
    }

    fn ctx<'a>(c: &'a Case, preimage: Option<&'a str>, now: u64) -> UnlockContext<'a> {
        UnlockContext {
            identity: &c.identity,
            preimage,
            now,
        }
    }

    #[test]
    fn ordinary_secret_needs_no_witness() {
        let c = case();
        assert_eq!(
            witness_for_secret("just a secret", &ctx(&c, None, NOW)).unwrap(),
            None
        );
    }

    #[test]
    fn p2pk_locked_to_us_signs() {
        let c = case();
        let secret = p2pk_secret(
            &p2pk_pubkey_hex(&c.identity),
            r#"[["sigflag","SIG_INPUTS"]]"#,
        );
        let witness = witness_for_secret(&secret, &ctx(&c, None, NOW))
            .unwrap()
            .unwrap();
        let parsed = P2pkWitness::from_json(&witness).expect("witness shape");
        assert_eq!(parsed.signatures.len(), 1);
        assert_eq!(parsed.signatures[0].len(), 128);
    }

    #[test]
    fn p2pk_foreign_lock_errors() {
        let c = case();
        let foreign = p2pk_pubkey_hex(&derive_identity(&[99u8; 32]).unwrap());
        let secret = p2pk_secret(&foreign, r#"[["sigflag","SIG_INPUTS"]]"#);
        assert_eq!(
            witness_for_secret(&secret, &ctx(&c, None, NOW)).unwrap_err(),
            ConditionError::NotOurLock
        );
    }

    #[test]
    fn p2pk_multisig_threshold_errors() {
        let c = case();
        let other = p2pk_pubkey_hex(&derive_identity(&[98u8; 32]).unwrap());
        let secret = p2pk_secret(
            &p2pk_pubkey_hex(&c.identity),
            &format!(r#"[["sigflag","SIG_INPUTS"],["pubkeys","{other}"],["n_sigs","2"]]"#),
        );
        assert_eq!(
            witness_for_secret(&secret, &ctx(&c, None, NOW)).unwrap_err(),
            ConditionError::MultisigUnsupported { required: 2 }
        );
    }

    #[test]
    fn p2pk_sig_all_errors() {
        let c = case();
        let secret = p2pk_secret(&p2pk_pubkey_hex(&c.identity), r#"[["sigflag","SIG_ALL"]]"#);
        assert_eq!(
            witness_for_secret(&secret, &ctx(&c, None, NOW)).unwrap_err(),
            ConditionError::SigAllUnsupported
        );
    }

    #[test]
    fn p2pk_expired_without_refund_is_anyone_can_spend() {
        let c = case();
        let foreign = p2pk_pubkey_hex(&derive_identity(&[97u8; 32]).unwrap());
        let secret = p2pk_secret(&foreign, r#"[["locktime","100"]]"#);
        assert_eq!(
            witness_for_secret(&secret, &ctx(&c, None, NOW)).unwrap(),
            None
        );
    }

    #[test]
    fn p2pk_expired_refund_key_ours_signs() {
        let c = case();
        let foreign = p2pk_pubkey_hex(&derive_identity(&[96u8; 32]).unwrap());
        let secret = p2pk_secret(
            &foreign,
            &format!(
                r#"[["locktime","100"],["refund","{}"]]"#,
                p2pk_pubkey_hex(&c.identity)
            ),
        );
        let witness = witness_for_secret(&secret, &ctx(&c, None, NOW))
            .unwrap()
            .unwrap();
        assert!(P2pkWitness::from_json(&witness).is_some());
    }

    fn htlc_secret(hash_hex: &str, tags: &str) -> String {
        format!(r#"["HTLC",{{"nonce":"t","data":"{hash_hex}","tags":{tags}}}]"#)
    }

    const PREIMAGE: &str = "0000000000000000000000000000000000000000000000000000000000000001";
    const LOCK_HASH: &str = "ec4916dd28fc4c10d78e287ca5d9cc51ee1ae73cbfde08c6b37324cbfaac8bc5";

    #[test]
    fn htlc_preimage_only_unlocks() {
        let c = case();
        let secret = htlc_secret(LOCK_HASH, r#"[["sigflag","SIG_INPUTS"]]"#);
        let witness = witness_for_secret(&secret, &ctx(&c, Some(PREIMAGE), NOW))
            .unwrap()
            .unwrap();
        let parsed = HtlcWitness::from_json(&witness).unwrap();
        assert_eq!(parsed.preimage.as_deref(), Some(PREIMAGE));
        assert!(parsed.signatures.is_none());
    }

    #[test]
    fn htlc_wrong_preimage_errors() {
        let c = case();
        let secret = htlc_secret(LOCK_HASH, r#"[["sigflag","SIG_INPUTS"]]"#);
        let wrong = format!("{}2", &PREIMAGE[..63]);
        assert_eq!(
            witness_for_secret(&secret, &ctx(&c, Some(&wrong), NOW)).unwrap_err(),
            ConditionError::PreimageMismatch
        );
    }

    #[test]
    fn htlc_missing_preimage_errors_unless_expired_open() {
        let c = case();
        let locked = htlc_secret(LOCK_HASH, r#"[["sigflag","SIG_INPUTS"]]"#);
        assert_eq!(
            witness_for_secret(&locked, &ctx(&c, None, NOW)).unwrap_err(),
            ConditionError::PreimageMissing
        );
        let expired = htlc_secret(LOCK_HASH, r#"[["locktime","100"]]"#);
        assert_eq!(
            witness_for_secret(&expired, &ctx(&c, None, NOW)).unwrap(),
            None
        );
    }

    #[test]
    fn htlc_pubkeys_pathway_adds_signature() {
        let c = case();
        let secret = htlc_secret(
            LOCK_HASH,
            &format!(
                r#"[["sigflag","SIG_INPUTS"],["pubkeys","{}"]]"#,
                p2pk_pubkey_hex(&c.identity)
            ),
        );
        let witness = witness_for_secret(&secret, &ctx(&c, Some(PREIMAGE), NOW))
            .unwrap()
            .unwrap();
        let parsed = HtlcWitness::from_json(&witness).unwrap();
        assert_eq!(parsed.preimage.as_deref(), Some(PREIMAGE));
        assert_eq!(parsed.signatures.as_deref().map(<[String]>::len), Some(1));
    }

    #[test]
    fn malformed_condition_errors() {
        let c = case();
        let secret = r#"["P2PK",{"nonce":"t","data":"zzz"}]"#;
        assert_eq!(
            witness_for_secret(secret, &ctx(&c, None, NOW)).unwrap_err(),
            ConditionError::MalformedSecret
        );
    }
}
