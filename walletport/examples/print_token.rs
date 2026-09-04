//! CI TOKEN-source for the cashu-ts conformance leg: mint a demo-keyset
//! token in-process (21 sat = 16/4/1, DLEQ on every proof, the device
//! export memo) and print the ccl wire token as the ONLY stdout line.
//! CI pipes it into scripts/e2e_cashuts_conformance.mjs (TOKEN=...), so
//! a real wallet — not this repo — judges the wire format.
//!
//! Run: `cargo run -p walletport --example print_token`

use cashu_core_lite::crypto::{blind_message, sign_message, unblind_signature};
use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::nut12::{prove_dleq, ProofDleq};
use cashu_core_lite::token::{Proof, TokenV4, TokenV4Token};
use sha2::{Digest, Sha256};
use walletport::encode_token_wire;

const DEMO_MINT: &str = "demo://micronuts";
const DEMO_KEYSET: &str = "00";
const MEMO: &str = "Swapped via Micronuts";

fn sk(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).expect("valid scalar")
}

fn mint_proof(a: &SecretKey, amount: u64, secret_hex: &str, nonce: u8) -> Proof {
    let bm = blind_message(secret_hex.as_bytes(), Some(sk(11))).expect("blind");
    let c_prime = sign_message(a, &bm.blinded);
    let dleq = prove_dleq(&bm.blinded, a, Some(sk(nonce))).expect("dleq");
    let c = unblind_signature(&c_prime, &bm.blinder, &a.public_key()).expect("unblind");
    Proof {
        amount,
        keyset_id: DEMO_KEYSET.to_string(),
        secret: secret_hex.to_string(),
        c: c.to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(dleq.e, dleq.s, bm.blinder)),
    }
}

fn main() {
    let a = SecretKey::from_slice(&Sha256::digest(b"demo://micronuts")).expect("demo key");
    let proofs = vec![
        mint_proof(&a, 16, "7072696e742d746f6b656e2d3136", 13),
        mint_proof(&a, 4, "7072696e742d746f6b656e2d3034", 14),
        mint_proof(&a, 1, "7072696e742d746f6b656e2d3031", 15),
    ];
    let token = encode_token_wire(&TokenV4 {
        mint: DEMO_MINT.to_string(),
        unit: "sat".to_string(),
        memo: Some(MEMO.to_string()),
        tokens: vec![TokenV4Token {
            keyset_id: DEMO_KEYSET.to_string(),
            proofs,
        }],
    })
    .expect("encode");
    println!("{token}");
}
