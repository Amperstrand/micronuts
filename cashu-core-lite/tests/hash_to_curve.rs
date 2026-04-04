use cashu_core_lite::hash_to_curve;
use cashu_core_lite::PublicKey;

fn assert_compressed_hex(point: &PublicKey, expected_hex: &str) {
    let encoded = point.to_encoded_point(true);
    let actual_hex = hex::encode(encoded.as_bytes());
    assert_eq!(actual_hex, expected_hex);
}

#[test]
fn test_hash_to_curve_produces_valid_point() {
    let point = hash_to_curve(b"test message").expect("should find valid point");
    let _ = point.as_affine();
}

#[test]
fn test_hash_to_curve_is_deterministic() {
    let point1 = hash_to_curve(b"deterministic test").expect("should find valid point");
    let point2 = hash_to_curve(b"deterministic test").expect("should find valid point");
    assert_eq!(point1, point2);
}

#[test]
fn test_hash_to_curve_different_inputs_produce_different_outputs() {
    let point1 = hash_to_curve(b"message 1").expect("should find valid point");
    let point2 = hash_to_curve(b"message 2").expect("should find valid point");
    assert_ne!(point1, point2);
}

#[test]
fn test_hash_to_curve_cdk_vector_secret_zero() {
    // These vectors were updated when Micronuts aligned `hash_to_curve` with the
    // upstream Cashu/CDK reference, which hashes a 4-byte little-endian counter.
    let secret = [0u8; 32];
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "024cce997d3b518f739663b757deaec95bcd9473c30a14ac2fd04023a739d1a725",
    );
}

#[test]
fn test_hash_to_curve_cdk_vector_secret_one() {
    let mut secret = [0u8; 32];
    secret[31] = 1;
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "022e7158e11c9506f1aa4248bf531298daa7febd6194f003edcd9b93ade6253acf",
    );
}

#[test]
fn test_hash_to_curve_cdk_vector_secret_two() {
    let mut secret = [0u8; 32];
    secret[31] = 2;
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "026cdbe15362df59cd1dd3c9c11de8aedac2106eca69236ecd9fbe117af897be4f",
    );
}
