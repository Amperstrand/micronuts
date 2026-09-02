//! Real-mint end-to-end for the offline gate. #[ignore]d: needs network
//! and mints real (valueless) tokens from the testnut dummy mint.
//!
//! Run: cargo test -p walletport --test real_mint_gate_e2e -- --ignored --nocapture
//!
//! Chain under test: REST NUT-04 quote -> dummy-pay poll -> mint blind
//! outputs -> unblind -> proofs (dleq?) -> cashuB wire -> OfflineGateValidator
//! DLEQ verify vs pinned /v1/keys -> Open.

#![cfg(feature = "std")]

use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut12::BlindSignatureDleq;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::persistent::PersistentWallet;
use cashu_core_lite::store::MemoryStore;
use cashu_core_lite::token::{TokenV4, TokenV4Token};
use cashu_core_lite::transport::MintClient;
use std::time::Duration;
use walletport::{encode_token_wire, GateDecision, OfflineGateValidator};

const MINT: &str = "https://testnut.cashu.exchange";

fn jget(url: &str) -> serde_json::Value {
    let resp = ureq::get(url)
        .timeout(Duration::from_secs(15))
        .call()
        .expect("HTTP GET");
    serde_json::from_reader(resp.into_reader()).expect("JSON body")
}

fn jpost(url: &str, body: serde_json::Value) -> serde_json::Value {
    let resp = ureq::post(url)
        .timeout(Duration::from_secs(15))
        .send_json(body)
        .expect("HTTP POST");
    serde_json::from_reader(resp.into_reader()).expect("JSON body")
}

fn unused(_: &str) -> CashuError {
    CashuError::Protocol(std::string::String::from("not used by this flow"))
}

#[derive(Clone)]
struct RestMint;

impl MintClient for RestMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        Err(unused("info"))
    }
    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        let v = jget(&format!("{MINT}/v1/keys"));
        let mut keysets = Vec::new();
        for ks in v["keysets"].as_array().expect("keysets array") {
            let id = ks["id"].as_str().expect("id").to_string();
            let unit = ks["unit"].as_str().unwrap_or("sat").to_string();
            let mut keys = Vec::new();
            for (amt_s, pk_hex) in ks["keys"].as_object().expect("keys map") {
                let amount: u64 = amt_s.parse().expect("amount key numeric");
                let b = hex::decode(pk_hex.as_str().expect("pk hex")).expect("pk bytes");
                let mut arr = [0u8; 33];
                arr.copy_from_slice(&b);
                keys.push(nut01::KeyPair {
                    amount,
                    pubkey: PublicKey::from_bytes(&arr).expect("point"),
                });
            }
            keys.sort_by_key(|k| k.amount);
            keysets.push(nut01::KeySet { id, unit, keys });
        }
        Ok(nut01::KeysResponse { keysets })
    }
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        Err(unused("keysets"))
    }
    fn post_mint_quote(
        &mut self,
        r: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        let v = jpost(
            &format!("{MINT}/v1/mint/quote/bolt11"),
            serde_json::json!({"amount": r.amount, "unit": r.unit}),
        );
        Ok(parse_quote(&v))
    }
    fn get_mint_quote(&mut self, id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        let v = jget(&format!("{MINT}/v1/mint/quote/bolt11/{id}"));
        Ok(parse_quote(&v))
    }
    fn post_mint(&mut self, r: nut04::MintRequest) -> Result<nut04::MintResponse, CashuError> {
        let outputs: Vec<_> = r
            .outputs
            .iter()
            .map(|o| {
                serde_json::json!({
                    "amount": o.amount, "id": o.id, "B_": hex::encode(o.b.to_bytes())
                })
            })
            .collect();
        let v = jpost(
            &format!("{MINT}/v1/mint/bolt11"),
            serde_json::json!({"quote": r.quote, "outputs": outputs}),
        );
        let mut sigs = Vec::new();
        for s in v["signatures"].as_array().expect("signatures") {
            let cb = hex::decode(s["C_"].as_str().expect("C_ hex")).expect("C bytes");
            let mut carr = [0u8; 33];
            carr.copy_from_slice(&cb);
            let dleq = s.get("dleq").filter(|d| !d.is_null()).and_then(|d| {
                let eh = hex::decode(d["e"].as_str()?).ok()?;
                let sh = hex::decode(d["s"].as_str()?).ok()?;
                let mut e32 = [0u8; 32];
                e32.copy_from_slice(&eh);
                let mut s32 = [0u8; 32];
                s32.copy_from_slice(&sh);
                Some(BlindSignatureDleq {
                    e: SecretKey::from_slice(&e32).ok()?,
                    s: SecretKey::from_slice(&s32).ok()?,
                })
            });
            sigs.push(nut00::BlindSignature {
                amount: s["amount"].as_u64().expect("amount"),
                id: s["id"].as_str().unwrap_or_default().to_string(),
                c: PublicKey::from_bytes(&carr).expect("C point"),
                dleq,
            });
        }
        Ok(nut04::MintResponse { signatures: sigs })
    }
    fn post_melt_quote(
        &mut self,
        _: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(unused("meltq"))
    }
    fn get_melt_quote(&mut self, _: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        Err(unused("meltq"))
    }
    fn post_melt(&mut self, _: nut05::MeltRequest) -> Result<nut05::MeltResponse, CashuError> {
        Err(unused("melt"))
    }
    fn post_swap(&mut self, _: nut03::SwapRequest) -> Result<nut03::SwapResponse, CashuError> {
        Err(unused("swap"))
    }
    fn post_check_state(
        &mut self,
        _: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        Err(unused("check"))
    }
    fn post_restore(
        &mut self,
        _: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        Err(unused("restore"))
    }
}

fn parse_quote(v: &serde_json::Value) -> nut04::MintQuoteResponse {
    nut04::MintQuoteResponse {
        quote: v["quote"].as_str().expect("quote id").to_string(),
        request: v["request"].as_str().unwrap_or_default().to_string(),
        paid: matches!(v["state"].as_str(), Some("PAID") | Some("ISSUED")),
        state: v["state"].as_str().unwrap_or("UNPAID").to_string(),
        expiry: v["expiry"].as_u64().unwrap_or_default(),
        amount: v["amount"].as_u64().unwrap_or_default(),
        unit: v["unit"].as_str().unwrap_or("sat").to_string(),
        amount_paid: v["amount_paid"].as_u64().unwrap_or_default(),
        amount_issued: v["amount_issued"].as_u64().unwrap_or_default(),
        updated_at: v["updated_at"].as_u64().unwrap_or_default(),
    }
}

#[test]
#[ignore = "network + real mint: mints valueless tokens from the testnut dummy mint"]
fn real_mint_through_offline_gate() {
    // 1. Pinned keyset from /v1/keys
    let mut transport = RestMint;
    let keys_resp = transport.get_keys().expect("keys");
    let keyset = keys_resp
        .keysets
        .into_iter()
        .find(|k| k.unit == "sat" && !k.keys.is_empty())
        .expect("a sat keyset");
    eprintln!("keyset id={} keys={}", keyset.id, keyset.keys.len());
    let keyset_id = keyset.id.clone();

    // 2. Quote + dummy-pay poll
    let quote = transport
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: 8,
            unit: "sat".into(),
        })
        .expect("quote");
    eprintln!(
        "quote {} state={} request={:.40}...",
        quote.quote, quote.state, quote.request
    );
    let mut paid = quote.paid;
    for i in 0..12 {
        if paid {
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
        let st = transport.get_mint_quote(&quote.quote).expect("poll");
        eprintln!("poll {i}: {}", st.state);
        paid = st.paid;
    }
    assert!(paid, "dummy mint did not auto-pay the quote in 60s");

    // 3. Mint through our wallet (blind/sign/unblind, NUT-13 secrets)
    let mut wallet =
        PersistentWallet::new(MINT, RestMint, MemoryStore::new(), [0x5e; 32]).expect("wallet");
    let minted = wallet
        .mint_deterministic(&quote.quote, 8, &keyset_id, &keyset)
        .expect("mint");
    assert_eq!(minted, 8, "minted amount");
    eprintln!("minted {minted} sats, {} proofs", wallet.proof_count());

    // 4. Token wire
    let proofs = wallet.spend(8).expect("spend");
    let dleq_count = proofs.iter().filter(|p| p.dleq.is_some()).count();
    eprintln!("proofs={} dleq-carrying={}", proofs.len(), dleq_count);
    let proofs: Vec<cashu_core_lite::token::Proof> = proofs
        .into_iter()
        .map(|p| cashu_core_lite::token::Proof {
            amount: p.amount,
            keyset_id: p.id.clone(),
            secret: p.secret.clone(),
            c: p.c.to_bytes().to_vec(),
            dleq: p.dleq.as_ref().map(|d| {
                cashu_core_lite::nuts::nut12::ProofDleq::new(d.e.clone(), d.s.clone(), d.r.clone())
            }),
        })
        .collect();
    let wire = encode_token_wire(&TokenV4 {
        mint: MINT.to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: keyset_id.clone(),
            proofs,
        }],
    })
    .expect("wire");
    eprintln!("token: {:.60}...", wire);

    // 5. Offline gate vs pinned keys
    let mut gate =
        OfflineGateValidator::new(vec![MINT.to_string()], vec![keyset], MemoryStore::new())
            .expect("gate");
    match gate.verify_token(&wire, 8) {
        Ok(GateDecision::Open { total_sats }) => {
            eprintln!("GATE OPEN: {total_sats} sats");
            assert_eq!(total_sats, 8);
        }
        Ok(GateDecision::Underpaid { total_sats }) => panic!("underpaid: {total_sats}"),
        Err(e) => panic!("gate rejected: {e:?} (dleq-carrying proofs on wire: {dleq_count})"),
    }
    // 6. Replay must be rejected
    assert!(
        gate.verify_token(&wire, 8).is_err(),
        "replay must be rejected"
    );
    eprintln!("replay rejected OK");
}
