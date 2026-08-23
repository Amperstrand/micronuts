#![no_main]
//! Fuzz the full offline gate decision path with a fixed pinned keyset:
//! token extraction -> base64url -> CBOR decode -> trust check ->
//! NUT-10/11 rejection -> hex/hash_to_curve on arbitrary secrets ->
//! point math on arbitrary C bytes -> DLEQ verify with arbitrary
//! scalars -> value check -> spent-ring persist. The only invariant:
//! no panics; every outcome is a clean Ok/Err.

use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::store::MemoryStore;
use libfuzzer_sys::fuzz_target;
use walletport::OfflineGateValidator;

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://fuzz.mint.example";

fn pinned() -> Vec<nut01::KeySet> {
    let keys = (0..7u32)
        .map(|exp| {
            let amount = 1u64 << exp;
            nut01::KeyPair {
                amount,
                pubkey: SecretKey::from_slice(&[amount as u8; 32])
                    .expect("valid scalar")
                    .public_key(),
            }
        })
        .collect();
    vec![nut01::KeySet {
        id: KEYSET_ID.to_string(),
        unit: "sat".to_string(),
        keys,
    }]
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(mut v) = OfflineGateValidator::new(vec![MINT.to_string()], pinned(), MemoryStore::new())
    else {
        return;
    };
    let price = (data.len() as u64) % 100;
    let _ = v.verify_token(s, price);
});
