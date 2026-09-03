//! OfflineGateValidator tests: a local "mint" generates NUT-12 proofs
//! (same construction the ccv cross-verification harness used against
//! gonuts), and the validator's decision matrix is exercised end to end
//! over the real `cashuB…` wire form.

use cashu_core_lite::crypto::{blind_message, sign_message, unblind_signature};
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::nuts::nut12::{hash_e, ProofDleq};
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{TokenV4, TokenV4Token};
use k256::ProjectivePoint;
use walletport::{encode_token_wire, GateDecision, OfflineGateValidator, WalletPortError};

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://gate-mint.example";

fn sk(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).unwrap()
}

/// One (amount → mint key) pinned keyset for amounts 1..=64.
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

/// Mint a single DLEQ-carrying proof the way a real mint + wallet would.
fn mint_proof(amount: u64, secret_hex: &str) -> cashu_core_lite::token::Proof {
    let a = sk(amount as u8); // mint key for this denomination
    let k_pub = a.public_key();

    let bm = blind_message(secret_hex.as_bytes(), Some(sk(11))).unwrap();
    let b_prime = bm.blinded;
    let r = bm.blinder;

    let c_prime = sign_message(&a, &b_prime);

    // Fiat–Shamir over (R1, R2, A, C') with nonce k = 13·G scalar.
    let k = sk(13);
    let k_scalar = k.to_scalar();
    let r1 = PublicKey::from_affine((ProjectivePoint::GENERATOR * k_scalar).into()).unwrap();
    let bp: ProjectivePoint = (&b_prime).into();
    let r2 = PublicKey::from_affine((bp * k_scalar).into()).unwrap();
    let e_bytes = hash_e(&r1, &r2, &k_pub, &c_prime);
    let e = SecretKey::from_slice(&e_bytes).unwrap();
    let s = SecretKey::from_slice(&(k_scalar + e.to_scalar() * a.to_scalar()).to_bytes()).unwrap();

    let c = unblind_signature(&c_prime, &r, &k_pub).unwrap();

    cashu_core_lite::token::Proof {
        amount,
        keyset_id: KEYSET_ID.to_string(),
        secret: secret_hex.to_string(),
        c: c.to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(e, s, r)),
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

fn validator() -> OfflineGateValidator<MemoryStore> {
    OfflineGateValidator::new(
        vec![MINT.to_string()],
        vec![pinned_keyset()],
        MemoryStore::new(),
    )
    .unwrap()
}

#[test]
fn valid_dleq_token_opens_gate() {
    let mut v = validator();
    let wire = token_wire(vec![
        mint_proof(8, "a1b2c3d4e5f60718"),
        mint_proof(4, "deadbeefcafe0011"),
    ]);
    assert_eq!(
        v.verify_token(&wire, 12).unwrap(),
        GateDecision::Open { total_sats: 12 }
    );
}

#[test]
fn below_price_is_underpaid_and_not_spent() {
    let mut v = validator();
    let wire = token_wire(vec![mint_proof(4, "0123456789abcdef")]);
    assert_eq!(
        v.verify_token(&wire, 8).unwrap(),
        GateDecision::Underpaid { total_sats: 4 }
    );
    // Not marked spent: a top-up token for the rest must still verify.
    let more = token_wire(vec![mint_proof(4, "fedcba9876543210")]);
    assert_eq!(
        v.verify_token(&more, 4).unwrap(),
        GateDecision::Open { total_sats: 4 }
    );
}

#[test]
fn replayed_token_is_rejected() {
    let mut v = validator();
    let wire = token_wire(vec![mint_proof(8, "abcdefabcdef0011")]);
    assert_eq!(
        v.verify_token(&wire, 8).unwrap(),
        GateDecision::Open { total_sats: 8 }
    );
    assert_eq!(v.verify_token(&wire, 8), Err(WalletPortError::Replay));
}

#[test]
fn tampered_signature_is_rejected() {
    let mut v = validator();
    let mut proof = mint_proof(8, "0011223344556677");
    proof.c[10] ^= 0x01; // flip one byte of C
    let wire = token_wire(vec![proof]);
    assert!(matches!(
        v.verify_token(&wire, 8),
        Err(WalletPortError::InvalidProof(_))
    ));
}

#[test]
fn tampered_dleq_response_is_rejected() {
    let mut v = validator();
    let mut proof = mint_proof(8, "9988776655443322");
    if let Some(d) = proof.dleq.as_mut() {
        d.s = sk(42); // wrong s
    }
    let wire = token_wire(vec![proof]);
    assert!(matches!(
        v.verify_token(&wire, 8),
        Err(WalletPortError::InvalidProof(_))
    ));
}

#[test]
fn missing_dleq_is_rejected() {
    let mut v = validator();
    let mut proof = mint_proof(8, "5566778899aabbcc");
    proof.dleq = None;
    let wire = token_wire(vec![proof]);
    assert!(matches!(
        v.verify_token(&wire, 8),
        Err(WalletPortError::InvalidProof(_))
    ));
}

#[test]
fn untrusted_mint_is_rejected() {
    let mut v = OfflineGateValidator::new(
        vec!["https://other.example".to_string()],
        vec![pinned_keyset()],
        MemoryStore::new(),
    )
    .unwrap();
    let wire = token_wire(vec![mint_proof(8, "ddeeff0011223344")]);
    assert!(matches!(
        v.verify_token(&wire, 8),
        Err(WalletPortError::UntrustedMint(_))
    ));
}

#[test]
fn unpinned_keyset_is_rejected() {
    // #56 provisioning model: a fully valid DLEQ chain minted under a
    // keyset the gate never pinned must be rejected — validity of the
    // cryptography is not authorization of the keyset.
    let mut v = validator();
    let foreign_id = "eeff0011aa22bb33cc44dd55ee66ff7700112233445566778899aabbccddeeff";
    let amount = 8u64;
    let a = sk(amount as u8 + 100);
    let k_pub = a.public_key();
    let bm = blind_message(b"0f0e0d0c0b0a".as_ref(), Some(sk(11))).unwrap();
    let c_prime = sign_message(&a, &bm.blinded);
    let k_scalar = sk(13).to_scalar();
    let r1 = PublicKey::from_affine((ProjectivePoint::GENERATOR * k_scalar).into()).unwrap();
    let bp: ProjectivePoint = (&bm.blinded).into();
    let r2 = PublicKey::from_affine((bp * k_scalar).into()).unwrap();
    let e = SecretKey::from_slice(&hash_e(&r1, &r2, &k_pub, &c_prime)).unwrap();
    let s = SecretKey::from_slice(&(k_scalar + e.to_scalar() * a.to_scalar()).to_bytes()).unwrap();
    let c = unblind_signature(&c_prime, &bm.blinder, &k_pub).unwrap();
    let proof = cashu_core_lite::token::Proof {
        amount,
        keyset_id: foreign_id.to_string(),
        secret: String::from("0f0e0d0c0b0a"),
        c: c.to_bytes().to_vec(),
        dleq: Some(ProofDleq::new(e, s, bm.blinder)),
    };
    let wire = encode_token_wire(&TokenV4 {
        mint: MINT.to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: foreign_id.to_string(),
            proofs: vec![proof],
        }],
    })
    .unwrap();
    assert!(matches!(
        v.verify_token(&wire, 8),
        Err(WalletPortError::InvalidProof(_))
    ));
}

#[test]
fn nut10_style_locked_secret_is_rejected() {
    let mut v = validator();
    // NUT-10 secrets are JSON-shaped; offline gates cannot evaluate locks.
    let locked = r#"{"kind":"P2PK","data":"02abcd"}"#;
    // It still needs to round-trip as a proof; the DLEQ math just needs
    // *some* secret bytes — the lock check fires before verification.
    let proof = mint_proof(8, "7777aaaa8888bbbb");
    let mut locked_proof = proof.clone();
    locked_proof.secret = locked.to_string();
    let wire = token_wire(vec![locked_proof]);
    assert_eq!(v.verify_token(&wire, 8), Err(WalletPortError::LockedSecret));
}

#[test]
fn spent_ring_survives_validator_restart() {
    // Shared medium: two validator sessions over one store.
    #[derive(Clone)]
    struct Shared(std::rc::Rc<std::cell::RefCell<MemoryStore>>);
    impl cashu_core_lite::store::ProofStore for Shared {
        fn load(&mut self) -> Result<Option<Vec<u8>>, cashu_core_lite::store::StoreError> {
            self.0.borrow_mut().load()
        }
        fn save(&mut self, blob: &[u8]) -> Result<(), cashu_core_lite::store::StoreError> {
            self.0.borrow_mut().save(blob)
        }
    }
    let medium = Shared(std::rc::Rc::new(
        std::cell::RefCell::new(MemoryStore::new()),
    ));

    let mut first = OfflineGateValidator::new(
        vec![MINT.to_string()],
        vec![pinned_keyset()],
        medium.clone(),
    )
    .unwrap();
    let wire = token_wire(vec![mint_proof(16, "beef0011beef0011")]);
    assert_eq!(
        first.verify_token(&wire, 16).unwrap(),
        GateDecision::Open { total_sats: 16 }
    );

    // Power cycle: a fresh validator over the same medium must replay-reject.
    let mut restarted =
        OfflineGateValidator::new(vec![MINT.to_string()], vec![pinned_keyset()], medium).unwrap();
    assert_eq!(
        restarted.verify_token(&wire, 16),
        Err(WalletPortError::Replay)
    );
}

#[test]
fn wire_roundtrip_is_stable() {
    // encode_token_wire → decode_token_or_err must round-trip proofs
    // including the dleq payload.
    let proof = mint_proof(2, "0102030405060708");
    let wire = token_wire(vec![proof.clone()]);
    assert!(wire.starts_with("cashuB"));
    let decoded = walletport::decode_token_or_err(&wire).unwrap();
    assert_eq!(decoded.total_amount(), 2);
    let dp = &decoded.tokens[0].proofs[0];
    assert_eq!(dp.secret, proof.secret);
    assert!(dp.dleq.is_some());
}
