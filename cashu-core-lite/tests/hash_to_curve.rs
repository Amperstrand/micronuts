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
    let secret = [0u8; 32];
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "024c84933755027799375b5e661a0b084b92fb34402fc7bb15609c0a8c45017ab1",
    );
}

#[test]
fn test_hash_to_curve_cdk_vector_secret_one() {
    let mut secret = [0u8; 32];
    secret[31] = 1;
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "02773f15d3489cad416a08d23fffa6ea03883e9a108be6cd353e0171afbc7ea881",
    );
}

#[test]
fn test_hash_to_curve_cdk_vector_secret_two() {
    let mut secret = [0u8; 32];
    secret[31] = 2;
    let point = hash_to_curve(&secret).expect("should find valid point");
    assert_compressed_hex(
        &point,
        "02517a405ddf5a86949e2a20df024767cf4e7c102fa7b68767ba1e50c6d0941035",
    );
}
