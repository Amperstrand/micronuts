//! Integration tests for NUT-12 DLEQ verification.
//!
//! Covers:
//! 1. The official `hash_e` vector from `nuts/tests/12-tests.md`.
//! 2. The official `BlindSignature` DLEQ vector.
//! 3. The official `Proof` DLEQ vector (reconstructs `B'` and `C'` from the
//!    blinding factor `r`, matching CDK's `Proof::verify_dleq`).
//! 4. The official deterministic-nonce derivation vector (mint secret = 2).
//! 5. A negative test: flipping one bit in `e` MUST make verification fail.
//! 6. A CDK round-trip: the `cashu` crate (CDK 0.17.3) constructs a
//!    `BlindSignature` with a DLEQ proof, we verify it here.

use cashu_core_lite::hash_to_curve;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut12::verify_dleq;
use k256::ProjectivePoint;

// ---------- helpers ----------

fn pk_from_hex(hex: &str) -> PublicKey {
    let bytes = hex::decode(hex).expect("valid hex");
    assert_eq!(
        bytes.len(),
        33,
        "expected compressed (33-byte) public key, got {} bytes",
        bytes.len()
    );
    let mut arr = [0u8; 33];
    arr.copy_from_slice(&bytes);
    PublicKey::from_bytes(&arr).expect("valid secp256k1 public key")
}

fn sk_from_hex(hex: &str) -> SecretKey {
    let bytes = hex::decode(hex).expect("valid hex");
    assert_eq!(bytes.len(), 32, "expected 32-byte secret key");
    SecretKey::from_slice(&bytes).expect("valid secp256k1 secret key")
}

// ---------- 1. Official hash_e vector ----------
//
// `hash_e` is exercised end-to-end by the lib unit test
// `nuts::nut12::tests::hash_e_matches_nut12_spec_vector`, which asserts the
// exact spec hash `a4dc034b...6401e`. The tests below cover the higher-level
// `verify_dleq` invariants, which transitively exercise `hash_e` against all
// three official proof vectors.

// ---------- 2. Official BlindSignature DLEQ vector ----------

#[test]
fn verify_dleq_accepts_official_blind_signature_vector() {
    // From nuts/tests/12-tests.md "DLEQ verification on BlindSignature".
    let a = pk_from_hex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798");
    let b_prime = pk_from_hex("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2");
    // C_' in the spec vector equals B_' (because the mint key is `1`, so
    // C' = 1 * B' = B').
    let c_prime = pk_from_hex("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2");
    let e = sk_from_hex("9818e061ee51d5c8edc3342369a554998ff7b4381c8652d724cdf46429be73d9");
    let s = sk_from_hex("9818e061ee51d5c8edc3342369a554998ff7b4381c8652d724cdf46429be73da");

    let valid = verify_dleq(&b_prime, &c_prime, &e, &s, &a).expect("verify should not error");
    assert!(valid, "official BlindSignature DLEQ vector must verify");
}

// ---------- 3. Official Proof DLEQ vector ----------

#[test]
fn verify_dleq_accepts_official_proof_vector() {
    // From nuts/tests/12-tests.md "DLEQ verification on Proof".
    //
    // The Proof vector carries the *unblinded* signature C and blinding
    // factor r. Carol reconstructs the blind pair (B_', C_') via:
    //     Y  = hash_to_curve(secret)
    //     C' = C + r*A
    //     B' = Y + r*G
    // and then runs the standard Alice-side verification.
    let a = pk_from_hex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798");
    let c_unblinded =
        pk_from_hex("024369d2d22a80ecf78f3937da9d5f30c1b9f74f0c32684d583cca0fa6a61cdcfc");
    let e = sk_from_hex("b31e58ac6527f34975ffab13e70a48b6d2b0d35abc4b03f0151f09ee1a9763d4");
    let s = sk_from_hex("8fbae004c59e754d71df67e392b6ae4e29293113ddc2ec86592a0431d16306d8");
    let r = sk_from_hex("a6d13fcd7a18442e6076f5e1e7c887ad5de40a019824bdfa9fe740d302e8d861");

    // IMPORTANT: CDK's `Secret::as_bytes()` returns the UTF-8 bytes of the
    // *hex string* (64 ASCII bytes), NOT the 32 decoded bytes. We must
    // match that to reproduce the same `Y` the mint used.
    let secret_str = b"daf4dd00a2b68a0858a80450f52c8a7d2ccf87d375e43e216e0c571f089f63e9";
    let y = hash_to_curve(secret_str).expect("hash_to_curve");

    let y_proj: ProjectivePoint = y.into();
    let a_proj: ProjectivePoint = a.into();
    let c_proj: ProjectivePoint = c_unblinded.into();
    let r_scalar = r.to_scalar();

    // B' = Y + r*G
    let r_g = ProjectivePoint::GENERATOR * r_scalar;
    let b_prime_proj = y_proj + r_g;
    let b_prime = PublicKey::from_affine(b_prime_proj.into()).expect("B' must not be the identity");

    // C' = C + r*A
    let r_a = a_proj * r_scalar;
    let c_prime_proj = c_proj + r_a;
    let c_prime = PublicKey::from_affine(c_prime_proj.into()).expect("C' must not be the identity");

    let valid = verify_dleq(&b_prime, &c_prime, &e, &s, &a).expect("verify should not error");
    assert!(
        valid,
        "official Proof DLEQ vector must verify after reconstruction"
    );
}

// ---------- 4. Official deterministic-nonce vector ----------

#[test]
fn verify_dleq_accepts_deterministic_nonce_vector() {
    // From nuts/tests/12-tests.md "Deterministic nonce derivation".
    // Mint secret a = 2 (not 1), so A != G.
    let a = pk_from_hex("02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5");
    let b_prime = pk_from_hex("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2");
    let c_prime = pk_from_hex("0244eccfc7a348274458bb38044c7f3c389b3c2086c7ec18b5812d2877ab937787");
    let e = sk_from_hex("2a16ffee280aff3c429045607f9b8e0bf8b35910c44c1b20b9dfaf01b263d7b3");
    let s = sk_from_hex("9df27731238334718d120d4f74611a7c668233f988e687ac3fb188f0a34a2dab");

    let valid = verify_dleq(&b_prime, &c_prime, &e, &s, &a).expect("verify should not error");
    assert!(
        valid,
        "official deterministic-nonce DLEQ vector must verify (mint secret = 2)"
    );
}

// ---------- 5. Negative test: flip a bit in `e` ----------

#[test]
fn verify_dleq_rejects_tampered_e() {
    let a = pk_from_hex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798");
    let b_prime = pk_from_hex("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2");
    let c_prime = pk_from_hex("02a9acc1e48c25eeeb9289b5031cc57da9fe72f3fe2861d264bdc074209b107ba2");
    let e_bytes = hex::decode("9818e061ee51d5c8edc3342369a554998ff7b4381c8652d724cdf46429be73d9")
        .expect("valid hex");
    let s = sk_from_hex("9818e061ee51d5c8edc3342369a554998ff7b4381c8652d724cdf46429be73da");

    // Flip the lowest bit of the first byte. Use XOR with 0x01.
    let mut tampered = e_bytes.clone();
    tampered[0] ^= 0x01;
    let e_tampered = SecretKey::from_slice(&tampered).expect("still a valid scalar");

    let valid = verify_dleq(&b_prime, &c_prime, &e_tampered, &s, &a)
        .expect("verify should not error on a valid-shape scalar");
    assert!(
        !valid,
        "tampered `e` (one bit flipped) MUST fail verification"
    );

    // Sanity: the original (untampered) e verifies, so the negative result
    // above is due to the bit flip, not the surrounding inputs.
    let e = SecretKey::from_slice(&e_bytes).expect("valid scalar");
    let valid_original =
        verify_dleq(&b_prime, &c_prime, &e, &s, &a).expect("verify should not error");
    assert!(valid_original, "untampered vector must still verify");
}

// ---------- 6. CDK round-trip ----------

#[test]
fn verify_dleq_accepts_dleq_constructed_by_cdk() {
    // Have CDK (the `cashu` crate, 0.17.3) construct a real BlindSignature
    // with a DLEQ proof, then verify the proof with our k256 port.
    use std::str::FromStr;

    use cashu::dhke::{blind_message as cdk_blind, sign_message as cdk_sign};
    use cashu::nuts::nut02::Id;
    use cashu::Amount;

    // Fixed inputs for determinism.
    let mint_secret_bytes = [0x22u8; 32];
    let blinder_bytes = [0x55u8; 32];
    let secret_msg: &[u8] = b"micronuts-nut12-roundtrip-secret";

    let cdk_mint_sk =
        cashu::SecretKey::from_slice(&mint_secret_bytes).expect("cdk mint secret key");
    let cdk_blinder = cashu::SecretKey::from_slice(&blinder_bytes).expect("cdk blinder");

    // B_' and r, produced by CDK.
    let (cdk_b_prime, _r) =
        cdk_blind(secret_msg, Some(cdk_blinder.clone())).expect("cdk blind_message");
    // C_' = a * B_', produced by CDK.
    let cdk_c_prime = cdk_sign(&cdk_mint_sk, &cdk_b_prime).expect("cdk sign_message");

    // Construct a BlindSignature with DLEQ in CDK. This internally runs
    // `calculate_dleq` (HMAC-SHA256 deterministic nonce + the prover side
    // of the protocol). Note: 0.17.3 takes `mint_secretkey` by value.
    let cdk_id = Id::from_str("00882760bfa2eb41").expect("valid keyset id");
    let cdk_bs = cashu::BlindSignature::new(
        Amount::from(8),
        cdk_c_prime,
        cdk_id,
        &cdk_b_prime,
        cdk_mint_sk.clone(),
    )
    .expect("cdk BlindSignature::new with DLEQ");

    let dleq = cdk_bs.dleq.as_ref().expect("cdk must attach a DLEQ proof");

    // Cross-verify with CDK itself, as a sanity check that our inputs are
    // the ones CDK would also accept.
    cdk_bs
        .verify_dleq(cdk_mint_sk.public_key(), cdk_b_prime)
        .expect("cdk must verify its own DLEQ");

    // Now translate the same inputs into cashu-core-lite types and run
    // OUR k256-based verify_dleq.
    let our_b_prime = pk_from_bytes(&cdk_b_prime.to_bytes());
    let our_c_prime = pk_from_bytes(&cdk_c_prime.to_bytes());
    let our_a = pk_from_bytes(&cdk_mint_sk.public_key().to_bytes());
    let our_e = SecretKey::from_slice(&dleq.e.to_secret_bytes()).expect("our e scalar");
    let our_s = SecretKey::from_slice(&dleq.s.to_secret_bytes()).expect("our s scalar");

    let valid = verify_dleq(&our_b_prime, &our_c_prime, &our_e, &our_s, &our_a)
        .expect("our verify should not error");
    assert!(
        valid,
        "DLEQ constructed by CDK MUST verify under our k256 port"
    );
}

fn pk_from_bytes(compressed: &[u8; 33]) -> PublicKey {
    PublicKey::from_bytes(compressed).expect("valid compressed public key")
}
