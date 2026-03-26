use cashu_core_lite::{
    blind_message, hash_to_curve, sign_message, unblind_signature, verify_signature,
};
use k256::{ProjectivePoint, PublicKey, SecretKey};

fn make_secret_key(bytes: &[u8; 32]) -> SecretKey {
    SecretKey::from_slice(bytes).expect("valid secret key")
}

fn expected_unblinded(secret: &[u8], mint_key: &SecretKey) -> PublicKey {
    let y = hash_to_curve(secret).expect("should hash to curve");
    let y_proj: ProjectivePoint = y.into();
    let k_scalar = *mint_key.to_nonzero_scalar();
    let expected = y_proj * k_scalar;
    PublicKey::from_affine(expected.into()).expect("valid point")
}

#[test]
fn test_blind_message_returns_valid_point() {
    let blinder = make_secret_key(&[1u8; 32]);
    let result = blind_message(b"test secret", Some(blinder)).expect("should blind");
    let _ = result.blinded.as_affine();
}

#[test]
fn test_blind_message_uses_provided_blinder() {
    let blinder = make_secret_key(&[2u8; 32]);
    let result = blind_message(b"test secret", Some(blinder.clone())).expect("should blind");
    assert_eq!(result.blinder, blinder);
}

#[test]
fn test_blind_message_computed_correctly() {
    let blinder = make_secret_key(&[3u8; 32]);
    let result = blind_message(b"test secret", Some(blinder.clone())).expect("should blind");

    let y = hash_to_curve(b"test secret").expect("should hash");
    let y_proj: ProjectivePoint = y.into();
    let r_proj: ProjectivePoint = blinder.public_key().into();
    let expected = y_proj + r_proj;

    assert_eq!(ProjectivePoint::from(result.blinded), expected);
}

#[test]
fn test_blind_unblind_roundtrip() {
    let blinder = make_secret_key(&[4u8; 32]);
    let mint_key = make_secret_key(&[5u8; 32]);
    let mint_pubkey = mint_key.public_key();

    let blinded = blind_message(b"roundtrip secret", Some(blinder.clone())).expect("should blind");
    let blinded_sig = sign_message(&mint_key, &blinded.blinded);

    let unblinded =
        unblind_signature(&blinded_sig, &blinder, &mint_pubkey).expect("should unblind");

    let expected = expected_unblinded(b"roundtrip secret", &mint_key);
    assert_eq!(unblinded, expected);
}

#[test]
fn test_unblind_wrong_blinder_produces_wrong_result() {
    let blinder = make_secret_key(&[6u8; 32]);
    let wrong_blinder = make_secret_key(&[7u8; 32]);
    let mint_key = make_secret_key(&[8u8; 32]);
    let mint_pubkey = mint_key.public_key();

    let blinded = blind_message(b"secret", Some(blinder.clone())).expect("should blind");
    let blinded_sig = sign_message(&mint_key, &blinded.blinded);

    let unblinded =
        unblind_signature(&blinded_sig, &wrong_blinder, &mint_pubkey).expect("should unblind");

    let expected = expected_unblinded(b"secret", &mint_key);
    assert_ne!(unblinded, expected);
}

#[test]
fn test_unblind_wrong_mint_pubkey_produces_wrong_result() {
    let blinder = make_secret_key(&[9u8; 32]);
    let mint_key = make_secret_key(&[10u8; 32]);
    let wrong_mint_key = make_secret_key(&[11u8; 32]);
    let wrong_mint_pubkey = wrong_mint_key.public_key();

    let blinded = blind_message(b"secret", Some(blinder.clone())).expect("should blind");
    let blinded_sig = sign_message(&mint_key, &blinded.blinded);

    let unblinded =
        unblind_signature(&blinded_sig, &blinder, &wrong_mint_pubkey).expect("should unblind");

    let expected = expected_unblinded(b"secret", &mint_key);
    assert_ne!(unblinded, expected);
}

#[test]
fn test_verify_signature_valid_blind_sig() {
    let blinder = make_secret_key(&[12u8; 32]);
    let mint_key = make_secret_key(&[13u8; 32]);
    let mint_pubkey = mint_key.public_key();

    let blinded = blind_message(b"verify secret", Some(blinder.clone())).expect("should blind");
    let blinded_sig = sign_message(&mint_key, &blinded.blinded);

    let unblinded =
        unblind_signature(&blinded_sig, &blinder, &mint_pubkey).expect("should unblind");

    let valid = verify_signature(b"verify secret", &unblinded, &mint_key).expect("should verify");
    assert!(valid);
}

#[test]
fn test_verify_signature_rejects_wrong_point() {
    let random_key = make_secret_key(&[99u8; 32]);
    let random_pubkey = random_key.public_key();

    let valid =
        verify_signature(b"some secret", &random_pubkey, &random_key).expect("should verify");
    assert!(!valid);
}

#[test]
fn test_verify_signature_rejects_wrong_secret() {
    let mint_key = make_secret_key(&[42u8; 32]);
    let y = hash_to_curve(b"actual secret").expect("should hash");
    let valid = verify_signature(b"wrong secret", &y, &mint_key).expect("should verify");
    assert!(!valid);
}

#[test]
fn test_verify_signature_rejects_wrong_mint_key() {
    let blinder = make_secret_key(&[14u8; 32]);
    let mint_key = make_secret_key(&[15u8; 32]);
    let wrong_mint_key = make_secret_key(&[16u8; 32]);
    let mint_pubkey = mint_key.public_key();

    let blinded = blind_message(b"secret", Some(blinder.clone())).expect("should blind");
    let blinded_sig = sign_message(&mint_key, &blinded.blinded);

    let unblinded =
        unblind_signature(&blinded_sig, &blinder, &mint_pubkey).expect("should unblind");

    let valid = verify_signature(b"secret", &unblinded, &wrong_mint_key).expect("should verify");
    assert!(!valid);
}

#[test]
fn test_different_secrets_produce_different_blinded_outputs() {
    let blinder = make_secret_key(&[18u8; 32]);

    let b1 = blind_message(b"secret alpha", Some(blinder.clone())).expect("should blind");
    let b2 = blind_message(b"secret beta", Some(blinder.clone())).expect("should blind");

    assert_ne!(b1.blinded, b2.blinded);
}

#[test]
fn test_same_secret_different_blinders_produce_different_outputs() {
    let blinder1 = make_secret_key(&[19u8; 32]);
    let blinder2 = make_secret_key(&[20u8; 32]);

    let b1 = blind_message(b"shared secret", Some(blinder1)).expect("should blind");
    let b2 = blind_message(b"shared secret", Some(blinder2)).expect("should blind");

    assert_ne!(b1.blinded, b2.blinded);
}

#[test]
fn test_full_swap_flow_multiple_proofs() {
    let mint_key = make_secret_key(&[21u8; 32]);
    let mint_pubkey = mint_key.public_key();

    let secrets: Vec<&[u8]> = vec![b"proof1", b"proof2", b"proof3"];
    let mut blinded_msgs = Vec::new();
    let mut blinder_keys = Vec::new();

    for (i, secret) in secrets.iter().enumerate() {
        let mut b = [0u8; 32];
        b[0] = (i + 30) as u8;
        let blinder = make_secret_key(&b);
        let bm = blind_message(secret, Some(blinder.clone())).expect("should blind");
        blinder_keys.push(bm.blinder.clone());
        blinded_msgs.push(bm.blinded);
    }

    let mut new_proofs = Vec::new();
    for (i, blinded) in blinded_msgs.iter().enumerate() {
        let sig = sign_message(&mint_key, blinded);
        let unblinded =
            unblind_signature(&sig, &blinder_keys[i], &mint_pubkey).expect("should unblind");
        let valid = verify_signature(secrets[i], &unblinded, &mint_key).expect("should verify");
        assert!(valid, "proof {} failed verification", i);
        new_proofs.push(unblinded);
    }

    assert_eq!(new_proofs.len(), 3);
}
