//! CLI offline-gate verifier for hardware-swap tokens.
//!
//! `--selftest` mints a demo-keyset proof in-process and verifies it
//! through the gate (proves the wiring without hardware). With `--token`,
//! verifies an exported device token against the pinned demo keyset
//! (all denominations map to the `SHA256("demo://micronuts")` mint key —
//! host-mint-tool's DemoMint signs every amount with that one key).
//!
//! Exit 0 iff the gate opens at the expected total.

use cashu_core_lite::crypto::{blind_message, sign_message, unblind_signature};
use cashu_core_lite::keypair::SecretKey;
use cashu_core_lite::nuts::nut01::{KeyPair, KeySet};
use cashu_core_lite::nuts::nut12::{prove_dleq, ProofDleq};
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{Proof, TokenV4, TokenV4Token};
use sha2::{Digest, Sha256};
use walletport::{encode_token_wire, GateDecision, OfflineGateValidator};

const DEMO_MINT: &str = "demo://micronuts";
const DEMO_KEYSET: &str = "00";

fn demo_mint_secret() -> SecretKey {
    SecretKey::from_slice(&Sha256::digest(b"demo://micronuts")).expect("valid demo key")
}

/// host-mint-tool's DemoMint: one key for every denomination.
fn demo_keyset() -> KeySet {
    let pk = demo_mint_secret().public_key();
    let keys = (0..17u32)
        .map(|e| KeyPair {
            amount: 1u64 << e,
            pubkey: pk,
        })
        .collect();
    KeySet {
        id: DEMO_KEYSET.to_string(),
        unit: "sat".to_string(),
        keys,
    }
}

fn validator() -> OfflineGateValidator<MemoryStore> {
    OfflineGateValidator::new(
        vec![DEMO_MINT.to_string()],
        vec![demo_keyset()],
        MemoryStore::new(),
    )
    .expect("validator boots")
}

fn sk(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).expect("valid scalar")
}

fn selftest_token(amount: u64) -> String {
    let a = demo_mint_secret();
    let a_pk = a.public_key();
    let secret_str = "676174652d7665726966792d73656c6674657374";
    let bm = blind_message(secret_str.as_bytes(), Some(sk(11))).expect("blind");
    let c_prime = sign_message(&a, &bm.blinded);

    let nonce = sk(13);
    let dleq = prove_dleq(&bm.blinded, &a, Some(nonce)).expect("dleq");

    let c = unblind_signature(&c_prime, &bm.blinder, &a_pk).expect("unblind");
    let proof = Proof {
        amount,
        keyset_id: DEMO_KEYSET.to_string(),
        secret: secret_str.to_string(),
        c: c.to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(dleq.e, dleq.s, bm.blinder)),
    };
    encode_token_wire(&TokenV4 {
        mint: DEMO_MINT.to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: DEMO_KEYSET.to_string(),
            proofs: vec![proof],
        }],
    })
    .expect("encode")
}

fn usage() -> ! {
    eprintln!("usage: gate_verify --selftest | --token <cashuB...> --expect <sats>");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut token: Option<String> = None;
    let mut expect: Option<u64> = None;
    let mut selftest = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--selftest" => selftest = true,
            "--token" => {
                i += 1;
                token = Some(args.get(i).expect("--token requires a value").clone());
            }
            "--expect" => {
                i += 1;
                expect = Some(
                    args.get(i)
                        .expect("--expect requires a value")
                        .parse()
                        .expect("integer sats"),
                );
            }
            _ => usage(),
        }
        i += 1;
    }

    let (wire, price) = if selftest {
        println!("selftest: minting an in-process demo-keyset proof");
        (selftest_token(8), 8)
    } else {
        match (token, expect) {
            (Some(w), Some(p)) => (w, p),
            _ => usage(),
        }
    };

    let mut v = validator();
    match v.verify_token(&wire, price) {
        Ok(GateDecision::Open { total_sats }) if total_sats == price => {
            println!("OPEN: {total_sats} sats verified against the pinned demo keyset");
            println!("spent ring now holds the proof (replay rejected)");
            match v.verify_token(&wire, price) {
                Err(_) => {
                    println!("replay correctly rejected");
                    std::process::exit(0);
                }
                Ok(_) => {
                    eprintln!("FAIL: replay was accepted");
                    std::process::exit(1);
                }
            }
        }
        Ok(GateDecision::Open { total_sats }) => {
            eprintln!("FAIL: opened at {total_sats} sats, expected {price}");
            std::process::exit(1);
        }
        Ok(other) => {
            eprintln!("FAIL: gate did not open: {other:?}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("FAIL: token rejected: {e:?}");
            std::process::exit(1);
        }
    }
}
