//! Bridge helpers between the upstream `cashu` crate's key types and
//! `cashu-core-lite`'s key types.
//!
//! Both crates wrap secp256k1 public/secret keys with thin newtypes, but the
//! underlying scalar bytes are identical. The conversions below use the
//! canonical 33-byte compressed SEC1 encoding (public keys) and the canonical
//! 32-byte secret scalar encoding (secret keys) as the wire format. This is
//! the exact pattern proven at `cashu-core-lite/tests/cashu_compat.rs:82`.
//!
//! These helpers exist so `cashu-core-lite` stays free of any `cashu`-crate
//! dependency in production (`no_std` firmware builds). All bridging happens
//! inside `micronuts-mint`, which is a host-only demo crate.

use cashu_core_lite::keypair::{PublicKey as LitePublicKey, SecretKey as LiteSecretKey};

/// Convert an upstream `cashu::PublicKey` to a `cashu_core_lite::PublicKey`.
///
/// Uses the 33-byte compressed SEC1 encoding as the wire format.
pub fn cashu_pk_to_lite(pk: &cashu::nuts::nut01::PublicKey) -> LitePublicKey {
    let bytes = pk.to_bytes();
    LitePublicKey::from_bytes(&bytes)
        .expect("cashu::PublicKey always yields a valid k256 compressed point")
}

/// Convert a `cashu_core_lite::PublicKey` to an upstream `cashu::PublicKey`.
pub fn lite_pk_to_cashu(pk: &LitePublicKey) -> cashu::nuts::nut01::PublicKey {
    let bytes = pk.to_bytes();
    cashu::nuts::nut01::PublicKey::from_slice(&bytes)
        .expect("cashu-core-lite PublicKey always yields a valid secp256k1 point")
}

/// Convert an upstream `cashu::SecretKey` to a `cashu_core_lite::SecretKey`.
///
/// Uses the canonical 32-byte secret scalar encoding as the wire format.
pub fn cashu_sk_to_lite(sk: &cashu::nuts::nut01::SecretKey) -> LiteSecretKey {
    let bytes = sk.to_secret_bytes();
    LiteSecretKey::from_slice(&bytes).expect("cashu::SecretKey always yields a valid k256 scalar")
}

/// Convert a `cashu_core_lite::SecretKey` to an upstream `cashu::SecretKey`.
pub fn lite_sk_to_cashu(sk: &LiteSecretKey) -> cashu::nuts::nut01::SecretKey {
    let bytes = sk.to_secret_bytes();
    cashu::nuts::nut01::SecretKey::from_slice(&bytes)
        .expect("cashu-core-lite SecretKey always yields a valid secp256k1 scalar")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_roundtrip_preserves_bytes() {
        let original_bytes = [0x42u8; 32];
        let lite_sk = LiteSecretKey::from_slice(&original_bytes).unwrap();
        let lite_pk = lite_sk.public_key();

        let cashu_pk = lite_pk_to_cashu(&lite_pk);
        let back = cashu_pk_to_lite(&cashu_pk);

        assert_eq!(lite_pk.to_bytes(), back.to_bytes());
        assert_eq!(cashu_pk.to_bytes(), lite_pk.to_bytes());
    }

    #[test]
    fn secret_key_roundtrip_preserves_bytes() {
        let original_bytes = [0x33u8; 32];
        let lite_sk = LiteSecretKey::from_slice(&original_bytes).unwrap();

        let cashu_sk = lite_sk_to_cashu(&lite_sk);
        let back = cashu_sk_to_lite(&cashu_sk);

        assert_eq!(lite_sk.to_secret_bytes(), back.to_secret_bytes());
        assert_eq!(cashu_sk.to_secret_bytes(), lite_sk.to_secret_bytes());
    }

    #[test]
    fn cross_crate_public_keys_match_for_same_scalar() {
        let sk_bytes = [0x55u8; 32];
        let lite_sk = LiteSecretKey::from_slice(&sk_bytes).unwrap();
        let cashu_sk = cashu::nuts::nut01::SecretKey::from_slice(&sk_bytes).unwrap();

        assert_eq!(
            lite_sk.public_key().to_bytes(),
            cashu_sk.public_key().to_bytes()
        );
    }
}
