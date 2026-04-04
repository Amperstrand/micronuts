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
