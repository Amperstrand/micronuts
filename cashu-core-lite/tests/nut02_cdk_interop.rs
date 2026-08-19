//! NUT-02 keyset-ID derivation vs upstream `cashu` (CDK) — the oracle is
//! CDK's own `Id::v1_from_keys` / `Id::v2_from_data`. A wallet deriving a
//! different ID from a mint's keys would target the wrong keyset for
//! verification and restore (the same silent-divergence class the NUT-13
//! fix closed).

use cashu::amount::Amount;
use cashu::nuts::nut01::Keys;
use cashu::nuts::nut02::Id;
use cashu::CurrencyUnit;

use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut02::{
    derive_keyset_id_v1, derive_keyset_id_v2, keyset_id_version, verify_keyset_id, AmountKey,
    KeysetIdVersion,
};

fn cdk_key(seed: u8) -> cashu::PublicKey {
    cashu::SecretKey::from_slice(&[seed; 32])
        .unwrap()
        .public_key()
}

fn ccl_key(seed: u8) -> PublicKey {
    SecretKey::from_slice(&[seed; 32]).unwrap().public_key()
}

/// Same denominations/keys on both sides: (amount, ccl pubkey, cdk pubkey).
fn fixture(shuffle: bool) -> Vec<(u64, PublicKey, cashu::PublicKey)> {
    let seeds = [(1u64, 21u8), (2, 22), (4, 23), (8, 24), (64, 25)];
    let mut v: Vec<_> = seeds
        .iter()
        .map(|&(amt, seed)| (amt, ccl_key(seed), cdk_key(seed)))
        .collect();
    if shuffle {
        v.reverse();
    }
    v
}

fn cdk_keys(fx: &[(u64, PublicKey, cashu::PublicKey)]) -> Keys {
    let mut map = std::collections::BTreeMap::new();
    for (amt, _, cdk) in fx {
        map.insert(Amount::from(*amt), *cdk);
    }
    Keys::new(map)
}

fn ccl_amount_keys<'a>(fx: &'a [(u64, PublicKey, cashu::PublicKey)]) -> Vec<AmountKey<'a>> {
    fx.iter()
        .map(|(amt, pk, _)| AmountKey {
            amount: *amt,
            pubkey: pk,
        })
        .collect()
}

#[test]
fn v1_id_matches_cdk() {
    for shuffle in [false, true] {
        let fx = fixture(shuffle);
        let theirs = Id::v1_from_keys(&cdk_keys(&fx)).to_string();
        let ours = derive_keyset_id_v1(&ccl_amount_keys(&fx));
        assert_eq!(
            ours, theirs,
            "v1 keyset ID diverged from CDK (shuffle={shuffle})"
        );
        assert!(ours.starts_with("00") && ours.len() == 16);
        assert_eq!(keyset_id_version(&ours), Some(KeysetIdVersion::V0));
    }
}

#[test]
fn v2_id_matches_cdk() {
    for (fee, expiry) in [
        (0u64, None),
        (10, None),
        (0, Some(1_800_000_000u64)),
        (7, Some(42)),
    ] {
        let fx = fixture(true); // unsorted input on purpose: both sides must sort
        let theirs = Id::v2_from_data(&cdk_keys(&fx), &CurrencyUnit::Sat, fee, expiry).to_string();
        let ours = derive_keyset_id_v2(&ccl_amount_keys(&fx), "sat", fee, expiry);
        assert_eq!(
            ours, theirs,
            "v2 keyset ID diverged (fee={fee}, expiry={expiry:?})"
        );
        assert!(ours.starts_with("01") && ours.len() == 66);
        assert_eq!(keyset_id_version(&ours), Some(KeysetIdVersion::V1));
    }
}

#[test]
fn verify_binding_roundtrip() {
    let fx = fixture(false);
    let ak = ccl_amount_keys(&fx);
    let v2 = derive_keyset_id_v2(&ak, "sat", 0, None);
    assert_eq!(verify_keyset_id(&v2, &ak, "sat", 0, None), Some(true));
    // Wrong fee → different ID → definitely not these keys at this fee.
    assert_eq!(verify_keyset_id(&v2, &ak, "sat", 999, None), Some(false));
    // Rotated key material (key of amount i moved to amount i+1) → mismatch.
    let rotated_pks: Vec<&PublicKey> = fx.iter().map(|(_, pk, _)| pk).collect();
    let tampered: Vec<AmountKey> = fx
        .iter()
        .enumerate()
        .map(|(i, (amt, _, _))| AmountKey {
            amount: *amt,
            pubkey: rotated_pks[(i + 1) % rotated_pks.len()],
        })
        .collect();
    assert_eq!(
        verify_keyset_id(&v2, &tampered, "sat", 0, None),
        Some(false)
    );
    // Unknown version byte → indeterminate.
    assert_eq!(verify_keyset_id("02abcd", &ak, "sat", 0, None), None);
}

#[test]
fn v2_id_in_test_constants_is_wellformed() {
    // The keyset ID used across the ccl/walletport suites is v2-shaped.
    let id = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
    assert_eq!(keyset_id_version(id), Some(KeysetIdVersion::V1));
    assert_eq!(id.len(), 66);
}
