use cashu_core_lite::{blind_message, hash_to_curve, unblind_signature};
use k256::NonZeroScalar;
use rand::rngs::OsRng;

#[test]
fn test_blind_message_produces_different_output() {
    let msg_point = hash_to_curve(b"test message").expect("valid point");
    let (blinded1, _) = blind_message(&msg_point, &mut OsRng);
    let (blinded2, _) = blind_message(&msg_point, &mut OsRng);
    assert_ne!(blinded1, blinded2);
}

#[test]
fn test_blind_message_produces_valid_point() {
    let msg_point = hash_to_curve(b"test message").expect("valid point");
    let (blinded, _) = blind_message(&msg_point, &mut OsRng);
    let _ = blinded.as_affine();
}

#[test]
fn test_unblind_signature_round_trip() {
    let msg_point = hash_to_curve(b"test message").expect("valid point");
    let (blinded_msg, blinding_factor) = blind_message(&msg_point, &mut OsRng);

    let mint_secret = NonZeroScalar::random(&mut OsRng);
    let mint_pubkey = k256::PublicKey::from_affine(
        (k256::ProjectivePoint::GENERATOR * mint_secret.as_ref()).to_affine(),
    )
    .expect("valid point");

    let blinded_sig = k256::PublicKey::from_affine(
        (k256::ProjectivePoint::from(blinded_msg.as_affine()) * mint_secret.as_ref()).to_affine(),
    )
    .expect("valid point");

    let unblinded = unblind_signature(&blinded_sig, &blinding_factor, &mint_pubkey);

    let expected_sig = k256::PublicKey::from_affine(
        (k256::ProjectivePoint::from(msg_point.as_affine()) * mint_secret.as_ref()).to_affine(),
    )
    .expect("valid point");

    assert_eq!(unblinded, expected_sig);
}
