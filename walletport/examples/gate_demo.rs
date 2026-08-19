//! Offline gate demo: mint a DLEQ-carrying token locally (as a stand-in
//! mint would), hand it to the `OfflineGateValidator` as `cashuB…` wire
//! bytes, and watch the decision matrix — open, underpaid, replay.
//!
//! Run: `cargo run -p walletport --example gate_demo`

use cashu_core_lite::crypto::{blind_message, sign_message, unblind_signature};
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::nuts::nut12::{hash_e, ProofDleq};
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{TokenV4, TokenV4Token};
use k256::ProjectivePoint;
use walletport::{encode_token_wire, GateDecision, OfflineGateValidator};

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://gate-mint.example";
const PRICE: u64 = 12;

fn sk(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).unwrap()
}

/// The mint's per-denomination keys, provisioned onto the gate at
/// deployment (the pinned trust anchor — the gate never contacts the
/// mint to fetch them).
fn pinned_keyset() -> nut01::KeySet {
    let keys = (0..7u32)
        .map(|exp| {
            let amount = 1u64 << exp;
            nut01::KeyPair {
                amount,
                pubkey: sk(amount as u8).public_key(),
            }
        })
        .collect();
    nut01::KeySet {
        id: KEYSET_ID.to_string(),
        unit: "sat".to_string(),
        keys,
    }
}

/// A customer's proof, as a NUT-12-capable mint + wallet would produce:
/// blind → sign → DLEQ proof → unblind.
fn mint_proof(amount: u64, secret_hex: &str) -> cashu_core_lite::token::Proof {
    let a = sk(amount as u8);
    let k_pub = a.public_key();

    let bm = blind_message(secret_hex.as_bytes(), Some(sk(11))).unwrap();
    let c_prime = sign_message(&a, &bm.blinded);

    let k = sk(13);
    let k_scalar = k.to_scalar();
    let r1 = PublicKey::from_affine((ProjectivePoint::GENERATOR * k_scalar).into()).unwrap();
    let bp: ProjectivePoint = (&bm.blinded).into();
    let r2 = PublicKey::from_affine((bp * k_scalar).into()).unwrap();
    let e = SecretKey::from_slice(&hash_e(&r1, &r2, &k_pub, &c_prime)).unwrap();
    let s = SecretKey::from_slice(&(k_scalar + e.to_scalar() * a.to_scalar()).to_bytes()).unwrap();

    let c = unblind_signature(&c_prime, &bm.blinder, &k_pub).unwrap();

    cashu_core_lite::token::Proof {
        amount,
        keyset_id: KEYSET_ID.to_string(),
        secret: secret_hex.to_string(),
        c: c.to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(e, s, bm.blinder)),
    }
}

fn token_wire(proofs: Vec<cashu_core_lite::token::Proof>) -> String {
    encode_token_wire(&TokenV4 {
        mint: MINT.to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: KEYSET_ID.to_string(),
            proofs,
        }],
    })
    .unwrap()
}

fn main() {
    let keyset = pinned_keyset();
    let mut gate = OfflineGateValidator::new(vec![MINT.to_string()], vec![keyset], MemoryStore::new())
        .expect("validator boots");

    println!("== TollGate offline validator demo (price: {PRICE} sats) ==\n");

    // 1. Exact payment: 8 + 4 = 12 sats.
    let exact = token_wire(vec![mint_proof(8, "a1b2c3d4e5f60718"), mint_proof(4, "deadbeefcafe0011")]);
    println!("token 1 (12 sats, exact): {}", &exact[..40.min(exact.len())]);
    match gate.verify_token(&exact, PRICE).unwrap() {
        GateDecision::Open { total_sats } => println!("  -> OPEN (paid {total_sats} sats)\n"),
        d => println!("  -> unexpected: {d:?}"),
    }

    // 2. Same token again: replay.
    println!("token 1 again (replay):");
    match gate.verify_token(&exact, PRICE) {
        Err(e) => println!("  -> REJECTED: {e:?}\n"),
        d => println!("  -> unexpected: {d:?}"),
    }

    // 3. Underpayment: 4 sats against a 12-sat price.
    let short = token_wire(vec![mint_proof(4, "0123456789abcdef")]);
    println!("token 2 (4 sats, underpaid):");
    match gate.verify_token(&short, PRICE).unwrap() {
        GateDecision::Underpaid { total_sats } => {
            println!("  -> UNDERPAID ({total_sats}/{PRICE} sats) — not spent, top up and retry\n")
        }
        d => println!("  -> unexpected: {d:?}"),
    }

    // 4. Tampered signature: one byte of C flipped.
    let mut tampered = mint_proof(8, "7766554433221100");
    tampered.c[10] ^= 0x01;
    println!("token 3 (tampered C):");
    match gate.verify_token(&token_wire(vec![tampered]), PRICE) {
        Err(e) => println!("  -> REJECTED: {e:?}\n"),
        d => println!("  -> unexpected: {d:?}"),
    }

    println!("All decisions made offline — the mint was never contacted.");
}
