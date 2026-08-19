//! GateController tests: scan payload → decision → GateIo effects, with
//! the hardware mocked so effect ordering is assertable.

use cashu_core_lite::crypto::{blind_message, sign_message, unblind_signature};
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::nuts::nut12::{hash_e, ProofDleq};
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{TokenV4, TokenV4Token};
use k256::ProjectivePoint;
use walletport::{encode_token_wire, GateAction, GateController, GateIo, OfflineGateValidator, RejectionReason};

const KEYSET_ID: &str = "015ba18a8adcd02e715a58358eb618da4a4b3791151a4bee5e968bb88406ccf76a";
const MINT: &str = "https://gate-mint.example";
const PRICE: u64 = 12;

/// Records Io calls in order — the assertions care about effects.
#[derive(Default)]
struct MockIo {
    log: Vec<&'static str>,
}

impl GateIo for MockIo {
    fn open(&mut self) {
        self.log.push("OPEN");
    }
    fn signal_ok(&mut self) {
        self.log.push("OK");
    }
    fn signal_err(&mut self, reason: RejectionReason) {
        self.log.push(match reason {
            RejectionReason::NotAToken => "ERR:NotAToken",
            RejectionReason::Undecodable => "ERR:Undecodable",
            RejectionReason::UntrustedMint => "ERR:UntrustedMint",
            RejectionReason::LockedSecret => "ERR:LockedSecret",
            RejectionReason::Replay => "ERR:Replay",
            RejectionReason::InvalidProof => "ERR:InvalidProof",
            RejectionReason::Other => "ERR:Other",
        });
    }
    fn status(&mut self, line: &str) {
        match line {
            "open" => self.log.push("STATUS:open"),
            "underpaid" => self.log.push("STATUS:underpaid"),
            _ => {}
        }
    }
}

fn sk(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).unwrap()
}

fn pinned() -> Vec<nut01::KeySet> {
    let keys = (0..7u32)
        .map(|exp| {
            let amount = 1u64 << exp;
            nut01::KeyPair {
                amount,
                pubkey: sk(amount as u8).public_key(),
            }
        })
        .collect();
    vec![nut01::KeySet {
        id: KEYSET_ID.to_string(),
        unit: "sat".to_string(),
        keys,
    }]
}

fn mint_proof(amount: u64, secret_hex: &str) -> cashu_core_lite::token::Proof {
    let a = sk(amount as u8);
    let bm = blind_message(secret_hex.as_bytes(), Some(sk(11))).unwrap();
    let c_prime = sign_message(&a, &bm.blinded);
    let k = sk(13);
    let r1 = PublicKey::from_affine((ProjectivePoint::GENERATOR * k.to_scalar()).into()).unwrap();
    let bp: ProjectivePoint = (&bm.blinded).into();
    let r2 = PublicKey::from_affine((bp * k.to_scalar()).into()).unwrap();
    let e = SecretKey::from_slice(&hash_e(&r1, &r2, &a.public_key(), &c_prime)).unwrap();
    let s = SecretKey::from_slice(&(k.to_scalar() + e.to_scalar() * a.to_scalar()).to_bytes()).unwrap();
    let c = unblind_signature(&c_prime, &bm.blinder, &a.public_key()).unwrap();
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

fn controller() -> (GateController<MemoryStore>, MockIo) {
    let v = OfflineGateValidator::new(vec![MINT.to_string()], pinned(), MemoryStore::new()).unwrap();
    (GateController::new(v, PRICE), MockIo::default())
}

#[test]
fn exact_payment_opens_and_signals() {
    let (mut c, mut io) = controller();
    let wire = token_wire(vec![mint_proof(8, "1111222233334444"), mint_proof(4, "5555666677778888")]);
    assert_eq!(
        c.handle_scan(&mut io, &wire),
        GateAction::Open { total_sats: 12 }
    );
    assert_eq!(io.log, vec!["OPEN", "OK", "STATUS:open"]);
}

#[test]
fn embedded_token_is_extracted() {
    // QR payloads sometimes carry labels around the token.
    let (mut c, mut io) = controller();
    let wire = token_wire(vec![mint_proof(8, "aaaabbbbccccdddd"), mint_proof(4, "eeeeffff00001111")]);
    let labeled = format!("TollGate topup: {wire} thanks!");
    assert_eq!(
        c.handle_scan(&mut io, &labeled),
        GateAction::Open { total_sats: 12 }
    );
}

#[test]
fn replay_is_rejected_with_signal() {
    let (mut c, mut io) = controller();
    let wire = token_wire(vec![mint_proof(8, "9999aaaa8888bbbb"), mint_proof(4, "1212343456567878")]);
    assert_eq!(c.handle_scan(&mut io, &wire), GateAction::Open { total_sats: 12 });
    assert_eq!(
        c.handle_scan(&mut io, &wire),
        GateAction::Rejected { reason: RejectionReason::Replay }
    );
    assert_eq!(io.log.last(), Some(&"ERR:Replay"));
}

#[test]
fn underpaid_reports_shortfall_without_opening() {
    let (mut c, mut io) = controller();
    let wire = token_wire(vec![mint_proof(4, "777788889999aaaa")]);
    assert_eq!(
        c.handle_scan(&mut io, &wire),
        GateAction::Underpaid { total_sats: 4, needed: 12 }
    );
    assert!(!io.log.contains(&"OPEN"), "underpaid must not open");
    assert_eq!(io.log.last(), Some(&"STATUS:underpaid"));
}

#[test]
fn garbage_payload_is_not_a_token() {
    let (mut c, mut io) = controller();
    assert_eq!(
        c.handle_scan(&mut io, "https://random-site.example/qr"),
        GateAction::Rejected { reason: RejectionReason::NotAToken }
    );
    // cashuA (V3) is out of scope for the V4 core by design.
    assert_eq!(
        c.handle_scan(&mut io, "cashuAeyJ0b2tlbiI6W119"),
        GateAction::Rejected { reason: RejectionReason::NotAToken }
    );
}

#[test]
fn tampered_proof_rejected_not_opened() {
    let (mut c, mut io) = controller();
    let mut p = mint_proof(12, "ccccdddd11112222");
    p.c[9] ^= 0x40;
    assert_eq!(
        c.handle_scan(&mut io, &token_wire(vec![p])),
        GateAction::Rejected { reason: RejectionReason::InvalidProof }
    );
    assert!(!io.log.contains(&"OPEN"));
}
