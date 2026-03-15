use cashu_core_lite::hash_to_curve;

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
