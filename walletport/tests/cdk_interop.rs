//! Upstream-CDK interop: DLEQ proofs generated with cashu (CDK)
//! primitives, *verified by CDK's own `Proof::verify_dleq`*, then fed to
//! our `OfflineGateValidator` through our wire format.
//!
//! This is the NUT-13-class divergence check applied to the offline gate:
//! our self-generated test fixtures could share a blind spot with our
//! implementation; artifacts blessed by upstream CDK cannot. The exact
//! construction mirrors CDK's private `calculate_dleq` (nonce k,
//! R1 = kG, R2 = kB', e = hash_e(R1,R2,A,C'), s = k + e·a), rebuilt here
//! from CDK's public primitives.

use cashu::amount::Amount;
use cashu::dhke::{blind_message, hash_e, hash_to_curve, sign_message, unblind_message};
use cashu::nuts::nut00::Proof as CdkProof;
use cashu::nuts::nut02::Id;
use cashu::nuts::nut12::ProofDleq as CdkProofDleq;
use cashu::secret::Secret;
use cashu::util::SECP256K1;
use cashu::{PublicKey as CdkPublicKey, SecretKey as CdkSecretKey};
use std::str::FromStr;

use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::nuts::nut12::ProofDleq;
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{TokenV4, TokenV4Token};
use walletport::{encode_token_wire, GateDecision, OfflineGateValidator, WalletPortError};

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://cdk-interop.example";

fn ccl_sk(k: &CdkSecretKey) -> SecretKey {
    SecretKey::from_slice(&k.to_secret_bytes()).unwrap()
}

fn ccl_pk(p: &CdkPublicKey) -> PublicKey {
    PublicKey::from_sec1_bytes(&p.serialize()).unwrap()
}

/// One proof minted the CDK way; returns (our token::Proof, cdk Proof,
/// mint pubkey) so both verifiers can be run on identical artifacts.
fn cdk_minted_proof(
    amount: u64,
    secret_hex: &str,
    tamper_s: bool,
) -> (cashu_core_lite::token::Proof, CdkProof, CdkPublicKey) {
    let a = CdkSecretKey::from_slice(&[amount as u8; 32]).unwrap();
    let a_pub = a.public_key();

    let (b_prime, r) = blind_message(secret_hex.as_bytes(), None).unwrap();
    let c_prime = sign_message(&a, &b_prime).unwrap();

    // CDK calculate_dleq, from public primitives:
    let k = CdkSecretKey::from_slice(&[13u8; 32]).unwrap();
    let r1 = k.public_key();
    let r2: CdkPublicKey = b_prime
        .mul_tweak(&SECP256K1, &k.as_scalar())
        .unwrap()
        .into();
    let e_bytes = hash_e([r1, r2, a_pub, c_prime]);
    let e = CdkSecretKey::from_slice(&e_bytes).unwrap();
    let s1: CdkSecretKey = e.mul_tweak(&a.as_scalar()).unwrap().into();
    let s: CdkSecretKey = if tamper_s {
        // Wrong-but-well-formed response: shape checks pass, the
        // Schnorr identity fails.
        CdkSecretKey::from_slice(&[42u8; 32]).unwrap()
    } else {
        k.add_tweak(&s1.as_scalar()).unwrap().into()
    };

    let c = unblind_message(&c_prime, &r, &a_pub).unwrap();

    let cdk_proof = CdkProof {
        amount: Amount::from(amount),
        keyset_id: Id::from_str(KEYSET_ID).unwrap(),
        secret: Secret::from_str(secret_hex).unwrap(),
        c,
        witness: None,
        dleq: Some(CdkProofDleq::new(e.clone(), s.clone(), r.clone())),
        p2pk_e: None,
    };

    let our_proof = cashu_core_lite::token::Proof {
        amount,
        keyset_id: KEYSET_ID.to_string(),
        secret: secret_hex.to_string(),
        c: ccl_pk(&c).to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(ccl_sk(&e), ccl_sk(&s), ccl_sk(&r))),
    };

    (our_proof, cdk_proof, a_pub)
}

fn pinned(a_pub: &CdkPublicKey) -> Vec<nut01::KeySet> {
    vec![nut01::KeySet {
        id: KEYSET_ID.to_string(),
        unit: "sat".to_string(),
        keys: vec![
            nut01::KeyPair {
                amount: 4,
                pubkey: ccl_pk(&CdkSecretKey::from_slice(&[4u8; 32]).unwrap().public_key()),
            },
            nut01::KeyPair {
                amount: 8,
                pubkey: ccl_pk(a_pub),
            },
        ],
    }]
}

fn wire(proof: cashu_core_lite::token::Proof) -> String {
    encode_token_wire(&TokenV4 {
        mint: MINT.to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: KEYSET_ID.to_string(),
            proofs: vec![proof],
        }],
    })
    .unwrap()
}

fn validator(keysets: Vec<nut01::KeySet>) -> OfflineGateValidator<MemoryStore> {
    OfflineGateValidator::new(vec![MINT.to_string()], keysets, MemoryStore::new()).unwrap()
}

#[test]
fn cdk_verified_proof_opens_our_gate() {
    let (our_proof, cdk_proof, a_pub) = cdk_minted_proof(8, "c0ffee0011223344", false);

    // Upstream CDK accepts its own artifacts…
    cdk_proof
        .verify_dleq(a_pub)
        .expect("CDK must verify its own proof");

    // …and our offline validator accepts the very same values.
    let mut v = validator(pinned(&a_pub));
    assert_eq!(
        v.verify_token(&wire(our_proof), 8).unwrap(),
        GateDecision::Open { total_sats: 8 }
    );
}

#[test]
fn cdk_rejected_proof_is_rejected_by_our_gate() {
    let (our_proof, cdk_proof, a_pub) = cdk_minted_proof(8, "c0ffee0011223344", true);

    // Upstream CDK rejects the tampered response…
    assert!(cdk_proof.verify_dleq(a_pub).is_err());

    // …and so do we, on identical artifacts.
    let mut v = validator(pinned(&a_pub));
    assert!(matches!(
        v.verify_token(&wire(our_proof), 8),
        Err(WalletPortError::InvalidProof(_))
    ));
}

#[test]
fn cdk_proofs_across_amounts_verify() {
    for (amount, secret) in [(4u64, "aaaa4a4a4a4a4a4a"), (8, "bbbb8b8b8b8b8b8b")] {
        let (our_proof, cdk_proof, a_pub) = cdk_minted_proof(amount, secret, false);
        cdk_proof.verify_dleq(a_pub).unwrap();
        let mut v = validator(pinned(&a_pub));
        assert_eq!(
            v.verify_token(&wire(our_proof), amount).unwrap(),
            GateDecision::Open { total_sats: amount },
            "amount {amount}"
        );
    }
}

#[test]
fn hash_to_curve_agrees_with_cdk() {
    // Sanity anchor for the B' reconstruction path: same secret → same Y.
    for secret in [
        "cdk-interop-secret",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    ] {
        let theirs = hash_to_curve(secret.as_bytes()).unwrap();
        let ours = cashu_core_lite::hash_to_curve(secret.as_bytes()).unwrap();
        assert_eq!(ccl_pk(&theirs).to_bytes(), ours.to_bytes());
    }
}
