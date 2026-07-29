use cashu::amount::FeeAndAmounts;
use cashu::dhke::{
    blind_message as cashu_blind_message, hash_to_curve as cashu_hash_to_curve,
    sign_message as cashu_sign_message, unblind_message as cashu_unblind_message,
    verify_message as cashu_verify_message,
};
use cashu::nuts::nut23::QuoteState;
use cashu::Amount;
use cashu_core_lite::{
    blind_message, hash_to_curve, sign_message, unblind_signature, verify_signature, PublicKey,
    SecretKey,
};
use cashu_core_lite::nuts::nut00::decompose_amount;

fn upstream_fee_table() -> FeeAndAmounts {
    (0, vec![1, 2, 4, 8, 16, 32, 64, 128]).into()
}

#[test]
fn amount_split_matches_upstream_cashu() {
    for amount in [0u64, 1, 2, 3, 7, 13, 42, 100, 127, 128, 255] {
        let ours = decompose_amount(amount);
        let theirs: Vec<u64> = if amount == 0 {
            Vec::new()
        } else {
            Amount::from(amount)
                .split(&upstream_fee_table())
                .expect("upstream split should succeed")
                .into_iter()
                .map(u64::from)
                .collect()
        };
        assert_eq!(ours, theirs, "split mismatch for amount {amount}");
    }
}

#[test]
fn hash_to_curve_matches_upstream_cashu() {
    let zero = [0u8; 32];
    let one = {
        let mut value = [0u8; 32];
        value[31] = 1;
        value
    };
    let two = {
        let mut value = [0u8; 32];
        value[31] = 2;
        value
    };
    let phrase = b"micronuts-cashu-interop-secret!!!";
    let inputs: [&[u8]; 4] = [&zero, &one, &two, phrase];

    for input in inputs {
        let ours = hash_to_curve(input).expect("our hash_to_curve should succeed");
        let theirs = cashu_hash_to_curve(input).expect("upstream hash_to_curve should succeed");
        assert_eq!(ours.to_bytes(), theirs.to_bytes());
    }
}

#[test]
fn blind_sign_unblind_matches_upstream_cashu() {
    let secret = [0x42u8; 32];
    let blinder_bytes = [0x11u8; 32];
    let mint_key_bytes = [0x22u8; 32];

    let our_blinder = SecretKey::from_slice(&blinder_bytes).expect("valid blinder");
    let our_mint_key = SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");

    let cashu_blinder = cashu::SecretKey::from_slice(&blinder_bytes).expect("valid blinder");
    let cashu_mint_key = cashu::SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");

    let our_blinded =
        blind_message(&secret, Some(our_blinder.clone())).expect("our blind_message succeeds");
    let (cashu_blinded, returned_blinder) =
        cashu_blind_message(&secret, Some(cashu_blinder)).expect("cashu blind_message succeeds");

    assert_eq!(our_blinded.blinded.to_bytes(), cashu_blinded.to_bytes());
    assert_eq!(our_blinded.blinder.to_secret_bytes(), returned_blinder.to_secret_bytes());

    let our_signed = sign_message(&our_mint_key, &our_blinded.blinded);
    let cashu_blinded_point =
        cashu::PublicKey::from_slice(&our_blinded.blinded.to_bytes()).expect("valid point");
    let cashu_signed =
        cashu_sign_message(&cashu_mint_key, &cashu_blinded_point).expect("cashu sign succeeds");
    assert_eq!(our_signed.to_bytes(), cashu_signed.to_bytes());

    let our_unblinded = unblind_signature(
        &our_signed,
        &our_blinded.blinder,
        &our_mint_key.public_key(),
    )
    .expect("our unblind succeeds");
    let cashu_unblinded = cashu_unblind_message(
        &cashu_signed,
        &cashu::SecretKey::from_slice(&our_blinded.blinder.to_secret_bytes()).unwrap(),
        &cashu_mint_key.public_key(),
    )
    .expect("cashu unblind succeeds");
    assert_eq!(our_unblinded.to_bytes(), cashu_unblinded.to_bytes());

    assert!(
        verify_signature(&secret, &our_unblinded, &our_mint_key).expect("our verify succeeds")
    );
    cashu_verify_message(&cashu_mint_key, cashu_unblinded, &secret)
        .expect("cashu verify should succeed");
}

#[test]
fn quote_state_strings_match_upstream_cashu() {
    assert_eq!(QuoteState::Unpaid.to_string(), "UNPAID");
    assert_eq!(QuoteState::Paid.to_string(), "PAID");
    assert_eq!(QuoteState::Issued.to_string(), "ISSUED");
}

#[test]
fn public_key_bytes_roundtrip() {
    let secret = SecretKey::from_slice(&[0x33u8; 32]).expect("valid secret");
    let pubkey = secret.public_key();
    let roundtrip = PublicKey::from_bytes(&pubkey.to_bytes()).expect("roundtrip succeeds");
    assert_eq!(pubkey, roundtrip);
}

// Randomized differential tests vs `cashu` 0.17.3. Each test feeds both
// implementations the same fresh `OsRng` inputs and asserts byte-for-byte
// equality. 128 iters/primitive -> >= 640 total assertions (> required 400).

use rand_core::{OsRng, RngCore};

/// 128 > 100 to leave margin above the required floor.
const DIFF_ITERATIONS: usize = 128;

fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf
}

/// `loop` looks unbounded; in practice it returns immediately because the
/// secp256k1 group order is ~2^256, so random bytes are almost always valid.
fn random_valid_secret_key() -> SecretKey {
    loop {
        let bytes = random_bytes(32);
        if let Ok(sk) = SecretKey::from_slice(&bytes) {
            return sk;
        }
    }
}

#[test]
fn hash_to_curve_random_matches_upstream_cashu() {
    for i in 0..DIFF_ITERATIONS {
        let input = random_bytes(32);
        let ours = hash_to_curve(&input).expect("our hash_to_curve should succeed");
        let theirs = cashu_hash_to_curve(&input).expect("upstream hash_to_curve should succeed");
        assert_eq!(
            ours.to_bytes(),
            theirs.to_bytes(),
            "hash_to_curve mismatch at iteration {i}\n  input:  {:02x?}\n  ours:   {}\n  theirs: {}",
            input,
            hex::encode(ours.to_bytes()),
            hex::encode(theirs.to_bytes()),
        );
    }
}

#[test]
fn blind_message_random_matches_upstream_cashu() {
    // One fixed blinder per run (fresh per execution), shared verbatim by
    // both impls; `secret` is the only varying input.
    let blinder_bytes = random_valid_secret_key().to_secret_bytes();
    let our_blinder = SecretKey::from_slice(&blinder_bytes).expect("valid blinder");

    for i in 0..DIFF_ITERATIONS {
        let secret = random_bytes(32);

        let ours = blind_message(&secret, Some(our_blinder.clone()))
            .expect("our blind_message succeeds");
        // Rebuild from bytes each iteration to avoid needing `cashu::SecretKey: Clone`.
        let cashu_blinder = cashu::SecretKey::from_slice(&blinder_bytes).expect("valid blinder");
        let (cashu_point, cashu_returned_blinder) =
            cashu_blind_message(&secret, Some(cashu_blinder))
                .expect("cashu blind_message succeeds");

        assert_eq!(
            ours.blinded.to_bytes(),
            cashu_point.to_bytes(),
            "blind_message blinded-point mismatch at iteration {i}\n  \
             secret: {:02x?}\n  ours:   {}\n  theirs: {}",
            secret,
            hex::encode(ours.blinded.to_bytes()),
            hex::encode(cashu_point.to_bytes()),
        );
        assert_eq!(
            ours.blinder.to_secret_bytes(),
            cashu_returned_blinder.to_secret_bytes(),
            "blind_message blinder mismatch at iteration {i}\n  \
             secret: {:02x?}\n  ours:   {}\n  theirs: {}",
            secret,
            hex::encode(ours.blinder.to_secret_bytes()),
            hex::encode(cashu_returned_blinder.to_secret_bytes()),
        );
    }
}

#[test]
fn sign_message_random_matches_upstream_cashu() {
    let mint_key_bytes = random_valid_secret_key().to_secret_bytes();
    let our_mint_key = SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");
    let cashu_mint_key = cashu::SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");

    for i in 0..DIFF_ITERATIONS {
        // `sign_message` is the differential under test: both impls see the
        // SAME blinded point (converted via 33-byte SEC1, per existing test).
        let secret = random_bytes(32);
        let our_blinder = random_valid_secret_key();
        let our_blinded =
            blind_message(&secret, Some(our_blinder)).expect("our blind_message succeeds");
        let cashu_blinded_point = cashu::PublicKey::from_slice(&our_blinded.blinded.to_bytes())
            .expect("valid blinded point for cashu conversion");

        let ours = sign_message(&our_mint_key, &our_blinded.blinded);
        let theirs = cashu_sign_message(&cashu_mint_key, &cashu_blinded_point)
            .expect("cashu sign_message succeeds");

        assert_eq!(
            ours.to_bytes(),
            theirs.to_bytes(),
            "sign_message mismatch at iteration {i}\n  \
             blinded: {}\n  ours:   {}\n  theirs: {}",
            hex::encode(our_blinded.blinded.to_bytes()),
            hex::encode(ours.to_bytes()),
            hex::encode(theirs.to_bytes()),
        );
    }
}

#[test]
fn unblind_signature_random_matches_upstream_cashu() {
    let mint_key_bytes = random_valid_secret_key().to_secret_bytes();
    let our_mint_key = SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");
    let cashu_mint_key = cashu::SecretKey::from_slice(&mint_key_bytes).expect("valid mint key");
    let our_mint_pubkey = our_mint_key.public_key();
    let cashu_mint_pubkey = cashu_mint_key.public_key();

    for i in 0..DIFF_ITERATIONS {
        // `unblind_signature` is the differential under test: both impls see
        // identical (signed, blinder, mint_pubkey) tuples for a fresh flow.
        let secret = random_bytes(32);
        let our_blinder = random_valid_secret_key();
        let cashu_blinder =
            cashu::SecretKey::from_slice(&our_blinder.to_secret_bytes()).expect("valid blinder");

        let our_blinded = blind_message(&secret, Some(our_blinder.clone()))
            .expect("our blind_message succeeds");
        let our_signed = sign_message(&our_mint_key, &our_blinded.blinded);
        let cashu_signed =
            cashu::PublicKey::from_slice(&our_signed.to_bytes()).expect("valid signed point");

        let ours = unblind_signature(&our_signed, &our_blinder, &our_mint_pubkey)
            .expect("our unblind_signature succeeds");
        let theirs = cashu_unblind_message(&cashu_signed, &cashu_blinder, &cashu_mint_pubkey)
            .expect("cashu unblind_message succeeds");

        assert_eq!(
            ours.to_bytes(),
            theirs.to_bytes(),
            "unblind_signature mismatch at iteration {i}\n  \
             secret:  {:02x?}\n  ours:   {}\n  theirs: {}",
            secret,
            hex::encode(ours.to_bytes()),
            hex::encode(theirs.to_bytes()),
        );
    }
}
