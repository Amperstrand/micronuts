// micronuts-walletd — TollGate wallet sidecar backed by
// micronuts-wallet-core (cashu-core-lite). H5 experiment: the
// minimal-footprint backend, speaking the SAME newline-JSON AF_UNIX
// protocol as cdk-walletd so the parity suite runs against both
// unchanged. Failures against this backend are FINDINGS about the
// minimal-core approach, not harness bugs.
//
// Protocol + semantics: see cdk-walletd (Amperstrand/cdk-walletd
// src/main.rs). Notable mapping differences:
//   - "saga_state" maps to the engine's persisted pending-sends
//     (micronuts journals sends as remove-and-record, not sagas).
//   - Recovery = connect + check_all_pending + reconcile_spent at boot
//     and before first money op per mint (same fail-closed shape).

use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

use micronuts_wallet_core::engine::{HistoryEntry, PendingSend, WalletEngine};
use cashu_core_lite::transport::MintClient;
use micronuts_wallet_core::mint_wire::{JsonTransport, WireMintClient};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Transport (ureq) and proof store (temp-file + rename, per ProofStore docs)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct UreqTransport {
    base: String,
}

impl JsonTransport for UreqTransport {
    fn get_json(&self, path: &str) -> Result<Value, cashu_core_lite::error::CashuError> {
        let resp = ureq::get(&format!("{}{}", self.base, path))
            .timeout(std::time::Duration::from_secs(60))
            .call();
        match resp {
            Ok(r) => r
                .into_json::<Value>()
                .map_err(|e| cashu_core_lite::error::CashuError::Transport(e.to_string())),
            Err(ureq::Error::Status(_code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(cashu_core_lite::error::CashuError::Protocol(body))
            }
            Err(e) => Err(cashu_core_lite::error::CashuError::Transport(e.to_string())),
        }
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<Value, cashu_core_lite::error::CashuError> {
        let resp = ureq::post(&format!("{}{}", self.base, path))
            .timeout(std::time::Duration::from_secs(60))
            .send_json(body.clone());
        match resp {
            Ok(r) => r
                .into_json::<Value>()
                .map_err(|e| cashu_core_lite::error::CashuError::Transport(e.to_string())),
            Err(ureq::Error::Status(_code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(cashu_core_lite::error::CashuError::Protocol(body))
            }
            Err(e) => Err(cashu_core_lite::error::CashuError::Transport(e.to_string())),
        }
    }
}

/// Byte-blob proof store: temp-file + atomic rename (crash-safety per
/// cashu-core-lite's ProofStore contract).
struct FileStore {
    path: PathBuf,
}

impl cashu_core_lite::store::ProofStore for FileStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, cashu_core_lite::store::StoreError> {
        match std::fs::read(&self.path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(cashu_core_lite::store::StoreError::Unavailable),
        }
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), cashu_core_lite::store::StoreError> {
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, blob).map_err(|_| cashu_core_lite::store::StoreError::Unavailable)?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|_| cashu_core_lite::store::StoreError::Unavailable)
    }
}

// ---------------------------------------------------------------------------
// Daemon state
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct PersistedMint {
    history: Vec<HistoryEntry>,
    pending: Vec<PendingSend>,
}

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    seed_hex: String,
    mints: HashMap<String, PersistedMint>,
    accepted: Vec<String>,
}

struct MintSlot {
    engine: WalletEngine<WireMintClient<UreqTransport>, FileStore>,
    recovered: bool,
}

struct Daemon {
    dir: PathBuf,
    seed: [u8; 32],
    accepted: Mutex<Vec<String>>,
    mints: Mutex<HashMap<String, MintSlot>>,
    persisted: Mutex<Persisted>,
    allow_untrusted: bool,
    allowed_uids: Vec<u32>,
}

type DynError = Box<dyn std::error::Error>;

impl Daemon {
    fn canonical(url: &str) -> Result<String, String> {
        let u = url.trim_end_matches('/');
        if u.starts_with("http://") || u.starts_with("https://") {
            Ok(u.to_string())
        } else {
            Err(format!("invalid mint url {url:?}"))
        }
    }

    fn persisted_path(&self) -> PathBuf {
        self.dir.join("micronuts-walletd-state.json")
    }

    fn load_or_init(dir: &PathBuf) -> Result<Persisted, DynError> {
        let p = dir.join("micronuts-walletd-state.json");
        if let Ok(raw) = std::fs::read_to_string(&p) {
            return Ok(serde_json::from_str(&raw)?);
        }
        Ok(Persisted::default())
    }

    fn save_persisted(&self) {
        let p = self.persisted_path();
        let tmp = p.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(&*self.persisted.lock().unwrap()) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    fn slot_for(&self, mint_url: &str) -> Result<String, String> {
        let canonical = Self::canonical(mint_url)?;
        {
            let mints = self.mints.lock().unwrap();
            if mints.contains_key(&canonical) {
                return Ok(canonical);
            }
        }
        let accepted = self.accepted.lock().unwrap().clone();
        if !self.allow_untrusted && !accepted.contains(&canonical) {
            return Err(format!("mint {canonical} is not in the accepted mints list"));
        }
        let store_path = self.dir.join(format!(
            "proofs-{}.bin",
            canonical
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        ));
        let mut store = FileStore { path: store_path };
        let client = WireMintClient::with_transport(&canonical, UreqTransport { base: canonical.clone() });
        let (history, pending) = {
            let p = self.persisted.lock().unwrap();
            match p.mints.get(&canonical) {
                Some(m) => (m.history.clone(), m.pending.clone()),
                None => (Vec::new(), Vec::new()),
            }
        };
        let engine =
            WalletEngine::with_pending(&canonical, client, store, self.seed, history, pending)
                .map_err(|e| format!("engine for {canonical}: {e:?}"))?;
        self.mints
            .lock()
            .unwrap()
            .insert(canonical.clone(), MintSlot { engine, recovered: false });
        Ok(canonical)
    }

    /// Fail-closed recovery: before the first money op on a mint in this
    /// lifetime, reconnect, check pending sends, reconcile spent proofs.
    fn ensure_recovered(&self, canonical: &str) -> Result<(), String> {
        let needs = {
            let mints = self.mints.lock().unwrap();
            match mints.get(canonical) {
                Some(s) => !s.recovered,
                None => return Err(format!("mint {canonical} not open")),
            }
        };
        if !needs {
            return Ok(());
        }
        let mut mints = self.mints.lock().unwrap();
        let slot = mints.get_mut(canonical).ok_or("slot vanished")?;
        slot.engine
            .check_all_pending()
            .map_err(|e| format!("recovery (pending sends) failed, funds ops refused: {e:?}"))?;
        let reclaimed = slot
            .engine
            .reconcile_spent()
            .map_err(|e| format!("recovery (reconcile) failed, funds ops refused: {e:?}"))?;
        let _ = reclaimed;
        eprintln!("micronuts-walletd: {canonical}: recovery complete");
        slot.recovered = true;
        self.sync_persisted(canonical);
        Ok(())
    }

    fn sync_persisted(&self, canonical: &str) {
        let mints = self.mints.lock().unwrap();
        if let Some(slot) = mints.get(canonical) {
            let mut p = self.persisted.lock().unwrap();
            let entry = p.mints.entry(canonical.to_string()).or_default();
            entry.history = slot.engine.history().to_vec();
            entry.pending = slot.engine.pending_sends().to_vec();
        }
        drop(mints);
        self.save_persisted();
    }
}

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok(id: u64, result: Value) -> Self {
        Response { id, ok: true, result: Some(result), error: None }
    }
    fn err(id: u64, error: String) -> Self {
        Response { id, ok: false, result: None, error: Some(error) }
    }
}

fn p_str<'a>(p: &'a Value, k: &str) -> &'a str {
    p.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

fn dispatch(d: &Daemon, req: &Request) -> (Response, bool) {
    let p = &req.params;
    match req.method.as_str() {
        "info" => (
            Response::ok(
                req.id,
                serde_json::json!({
                    "backend": "micronuts",
                    "kind": "sidecar",
                    "version": env!("CARGO_PKG_VERSION"),
                    "contract": { "nut_07": true, "nut_09": true, "restore": true, "cgo": false },
                    "licence": "0BSD",
                }),
            ),
            false,
        ),

        "decode_token" => {
            let s = p_str(p, "token");
            let mut mints = d.mints.lock().unwrap();
            // Offline decode via any open engine's inspect (no mint wire).
            let res = mints.values_mut().next().map(|slot| {
                slot.engine
                    .inspect_token(s)
                    .map(|insp| serde_json::json!({
                        "token": s,
                        "mint": insp.summary.mint,
                        "amount": insp.summary.amount,
                        "fee": insp.fee,
                        "spent": insp.spent,
                        "pending": insp.pending,
                    }))
                    .map_err(|e| format!("{e:?}"))
            });
            match res {
                Some(Ok(v)) => (Response::ok(req.id, v), false),
                Some(Err(e)) => (Response::err(req.id, format!("decode_token: {e}")), false),
                None => (Response::err(req.id, "decode_token: no open mint".into()), false),
            }
        }

        "get_balance" => {
            let mints = d.mints.lock().unwrap();
            let total: u64 = mints.values().map(|s| s.engine.balance()).sum();
            (Response::ok(req.id, serde_json::json!(total)), false)
        }

        "get_balance_by_mint" => {
            let mint = p_str(p, "mint_url");
            match d.slot_for(mint) {
                Ok(c) => {
                    let b = {
                        let mints = d.mints.lock().unwrap();
                        mints.get(&c).map(|s| s.engine.balance()).unwrap_or(0)
                    };
                    (Response::ok(req.id, serde_json::json!(b)), false)
                }
                Err(e) => (Response::err(req.id, format!("get_balance_by_mint: {e}")), false),
            }
        }

        "get_all_mint_balances" => {
            let mints = d.mints.lock().unwrap();
            let mut map = serde_json::Map::new();
            for (url, s) in mints.iter() {
                map.insert(url.clone(), serde_json::json!(s.engine.balance()));
            }
            (Response::ok(req.id, Value::Object(map)), false)
        }

        "swap_fee_sats" => {
            let s = p_str(p, "token");
            let mut mints = d.mints.lock().unwrap();
            let res = mints.values_mut().next().map(|slot| {
                slot.engine.inspect_token(s).map(|i| i.fee).map_err(|e| format!("{e:?}"))
            });
            match res {
                Some(Ok(f)) => (Response::ok(req.id, serde_json::json!(f)), false),
                Some(Err(e)) => (Response::err(req.id, format!("swap_fee_sats: {e}")), false),
                None => (Response::err(req.id, "swap_fee_sats: no open mint".into()), false),
            }
        }

        "check_state" => {
            let s = p_str(p, "token");
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("check_state: {e}")), false),
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = match mints.get_mut(&c) {
                Some(s) => s,
                None => return (Response::err(req.id, "check_state: slot gone".into()), false),
            };
            match slot.engine.check_token_state(s) {
                Ok(states) => {
                    let mut any_spent = false;
                    let mut ambiguous = false;
                    for st in &states {
                        match st.state.as_str() {
                            "SPENT" => any_spent = true,
                            "UNSPENT" => {}
                            _ => ambiguous = true,
                        }
                    }
                    if any_spent {
                        (Response::ok(req.id, serde_json::json!(true)), false)
                    } else if ambiguous {
                        (Response::err(req.id, "check_state: pending proof state — ambiguous".into()), false)
                    } else {
                        (Response::ok(req.id, serde_json::json!(false)), false)
                    }
                }
                Err(e) => (Response::err(req.id, format!("check_state: {e:?}")), false),
            }
        }

        "receive" => {
            let s = p_str(p, "token");
            // Decode first for the mint URL (offline summary needs an
            // engine; use any open one for the summary parse).
            let mint_guess = {
                let mints = d.mints.lock().unwrap();
                mints.keys().next().cloned()
            };
            let c = match d.slot_for(mint_guess.as_deref().unwrap_or("")) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("receive: {e}")), false),
            };
            if let Err(e) = d.ensure_recovered(&c) {
                return (Response::err(req.id, format!("receive: {e}")), false);
            }
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.receive_token(s) {
                Ok(n) => {
                    drop(mints);
                    d.sync_persisted(&c);
                    (Response::ok(req.id, serde_json::json!(n)), false)
                }
                Err(e) => (Response::err(req.id, format!("receive: {e:?}")), false),
            }
        }

        "request_mint_quote" => {
            let amount = p.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("request_mint_quote: {e}")), false),
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.mint_via_invoice(amount) {
                Ok(q) => {
                    mint_quote_amounts().lock().unwrap().insert(q.quote.clone(), (amount, c.clone()));
                    (
                        Response::ok(
                            req.id,
                            serde_json::json!({
                                "quote_id": q.quote,
                                "request": q.request,
                                "state": if q.paid { 1 } else { 0 },
                                "amount": amount,
                                "expiry": 0,
                            }),
                        ),
                        false,
                    )
                }
                Err(e) => (Response::err(req.id, format!("request_mint_quote: {e:?}")), false),
            }
        }

        "mint_quote_state" => {
            let qid = p_str(p, "quote_id");
            let mint = if p_str(p, "mint_url").is_empty() {
                match mint_quote_amounts().lock().unwrap().get(qid) {
                    Some((_, m)) => m.clone(),
                    None => String::new(),
                }
            } else {
                p_str(p, "mint_url").to_string()
            };
            let c = match d.slot_for(&mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("mint_quote_state: {e}")), false),
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.poll_mint_quote(qid) {
                Ok(q) => (
                    Response::ok(req.id, serde_json::json!(if q.paid { 1 } else { 0 })),
                    false,
                ),
                Err(e) => (Response::err(req.id, format!("mint_quote_state: {e:?}")), false),
            }
        }

        "mint_tokens" => {
            let qid = p_str(p, "quote_id");
            let amount = p.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
            let mint = if p_str(p, "mint_url").is_empty() {
                match mint_quote_amounts().lock().unwrap().get(qid) {
                    Some((_, m)) => m.clone(),
                    None => String::new(),
                }
            } else {
                p_str(p, "mint_url").to_string()
            };
            let c = match d.slot_for(&mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("mint_tokens: {e}")), false),
            };
            if let Err(e) = d.ensure_recovered(&c) {
                return (Response::err(req.id, format!("mint_tokens: {e}")), false);
            }
            let amt = if amount > 0 {
                amount
            } else {
                mint_quote_amounts().lock().unwrap().get(qid).map(|(a, _)| *a).unwrap_or(0)
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.mint_paid_quote(qid, amt) {
                Ok(n) => {
                    drop(mints);
                    d.sync_persisted(&c);
                    (Response::ok(req.id, serde_json::json!(n)), false)
                }
                Err(e) => (Response::err(req.id, format!("mint_tokens: {e:?}")), false),
            }
        }

        "send" => {
            let amount = p.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("send: {e}")), false),
            };
            if let Err(e) = d.ensure_recovered(&c) {
                return (Response::err(req.id, format!("send: {e}")), false);
            }
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.send_token(amount, None) {
                Ok(tok) => {
                    drop(mints);
                    d.sync_persisted(&c);
                    (Response::ok(
                        req.id,
                        serde_json::json!({ "token": tok, "mint": c, "amount": amount }),
                    ), false)
                }
                Err(e) => (Response::err(req.id, format!("send: {e:?}")), false),
            }
        }

        "send_with_overpayment" => {
            // The minimal core composes exact sends only; overpayment
            // tolerance is a cdk-walletd capability. Honest refusal.
            (Response::err(
                req.id,
                "send_with_overpayment: not implemented by micronuts backend".into(),
            ), false)
        }

        "drain" => {
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("drain: {e}")), false),
            };
            if let Err(e) = d.ensure_recovered(&c) {
                return (Response::err(req.id, format!("drain: {e}")), false);
            }
            let bal = {
                let mints = d.mints.lock().unwrap();
                mints.get(&c).unwrap().engine.balance()
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.send_token(bal, None) {
                Ok(tok) => {
                    drop(mints);
                    d.sync_persisted(&c);
                    (Response::ok(
                        req.id,
                        serde_json::json!({ "token": tok, "mint": c, "amount": bal }),
                    ), false)
                }
                Err(e) => (Response::err(req.id, format!("drain: {e:?}")), false),
            }
        }

        "request_melt_quote" => {
            let invoice = p_str(p, "invoice");
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("request_melt_quote: {e}")), false),
            };
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.quote_melt(invoice) {
                Ok(q) => {
                    cache_melt_quote(&q.quote, invoice);
                    (
                        Response::ok(
                            req.id,
                            serde_json::json!({
                                "quote_id": q.quote,
                                "amount": q.amount,
                                "fee_reserve": q.fee_reserve,
                                "state": 0,
                                "expiry": 0,
                            }),
                        ),
                        false,
                    )
                }
                Err(e) => (Response::err(req.id, format!("request_melt_quote: {e:?}")), false),
            }
        }

        "melt" => {
            let qid = p_str(p, "quote_id");
            // micronuts melts by INVOICE; map quote -> invoice via the
            // in-memory melt quote cache (quotes are short-lived on the
            // bench).
            let invoice = match melt_cache().lock().unwrap().get(qid) {
                Some(i) => i.clone(),
                None => {
                    return (
                        Response::err(req.id, format!("melt: unknown quote ID: {qid}")),
                        false,
                    )
                }
            };
            let mint = p_str(p, "mint_url");
            let c = match d.slot_for(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("melt: {e}")), false),
            };
            if let Err(e) = d.ensure_recovered(&c) {
                return (Response::err(req.id, format!("melt: {e}")), false);
            }
            let mut mints = d.mints.lock().unwrap();
            let slot = mints.get_mut(&c).unwrap();
            match slot.engine.melt(&invoice) {
                Ok((_q, outcome)) => {
                    drop(mints);
                    d.sync_persisted(&c);
                    let paid = outcome.paid;
                    (Response::ok(
                        req.id,
                        serde_json::json!({ "quote_id": qid, "paid": paid, "preimage": "" }),
                    ), false)
                }
                Err(e) => (Response::err(req.id, format!("melt: {e:?}")), false),
            }
        }

        "melt_to_lightning" => (
            Response::err(req.id, "melt_to_lightning: not implemented by micronuts backend".into()),
            false,
        ),

        "accept_mint" => {
            let mint = p_str(p, "mint_url");
            let c = match Daemon::canonical(mint) {
                Ok(c) => c,
                Err(e) => return (Response::err(req.id, format!("accept_mint: {e}")), false),
            };
            {
                let mut acc = d.accepted.lock().unwrap();
                if !acc.contains(&c) {
                    acc.push(c.clone());
                }
                d.persisted.lock().unwrap().accepted = acc.clone();
            }
            d.save_persisted();
            match d.slot_for(&c) {
                Ok(_) => (Response::ok(req.id, serde_json::json!({"accepted": c})), false),
                Err(e) => (
                    Response::ok(
                        req.id,
                        serde_json::json!({"accepted": c, "warning": e}),
                    ),
                    false,
                ),
            }
        }

        "saga_state" => {
            // Map the engine's persisted pending sends (its journaling
            // discipline: remove-and-record) into the saga_state shape.
            let mints = d.mints.lock().unwrap();
            let mut list = Vec::new();
            for (url, slot) in mints.iter() {
                for ps in slot.engine.pending_sends() {
                    list.push(serde_json::json!({
                        "id": format!("pending-{}", ps.ts_secs),
                        "kind": "Send",
                        "state": "Micronuts(PendingSend)",
                        "amount": ps.amount,
                        "mint_url": url,
                        "quote_id": Value::Null,
                        "created_at": ps.ts_secs,
                        "updated_at": ps.ts_secs,
                    }));
                }
            }
            (Response::ok(req.id, serde_json::json!(list)), false)
        }

        "shutdown" => (Response::ok(req.id, serde_json::json!({})), true),

        other => (Response::err(req.id, format!("not implemented: {other}")), false),
    }
}

fn melt_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_melt_quote(qid: &str, invoice: &str) {
    melt_cache().lock().unwrap().insert(qid.to_string(), invoice.to_string());
}

fn mint_quote_amounts() -> &'static Mutex<HashMap<String, (u64, String)>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, (u64, String)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc == 0 {
        Some(cred.uid)
    } else {
        None
    }
}

fn main() -> Result<(), DynError> {
    let mut socket = String::from("/tmp/micronuts-walletd.sock");
    let mut work_dir = String::from("/tmp/micronuts-walletd");
    let mut mints: Vec<String> = Vec::new();
    let mut allow_untrusted = false;
    let mut allowed_uids: Vec<u32> = Vec::new();
    let mut seed_hex: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket = args.next().unwrap_or(socket),
            "--work-dir" => work_dir = args.next().unwrap_or(work_dir),
            "--mint" => {
                if let Some(m) = args.next() {
                    mints.push(m);
                }
            }
            "--allow-untrusted-mints" => allow_untrusted = true,
            "--seed-hex" => seed_hex = args.next(),
            "--allow-uid" => {
                if let Some(u) = args.next().and_then(|u| u.parse().ok()) {
                    allowed_uids.push(u);
                }
            }
            _ => {}
        }
    }

    std::fs::create_dir_all(&work_dir)?;
    let dir = PathBuf::from(&work_dir);

    let mut persisted = Daemon::load_or_init(&dir)?;
    if persisted.seed_hex.is_empty() {
        // Fresh random seed (hex, 32 bytes).
        let mut seed = [0u8; 32];
        getrandom_fallback(&mut seed)?;
        persisted.seed_hex = hex(&seed);
        eprintln!("micronuts-walletd: generated seed {}", persisted.seed_hex);
    }
    for m in &mints {
        let c = Daemon::canonical(m)?;
        if !persisted.accepted.contains(&c) {
            persisted.accepted.push(c);
        }
    }
    let seed = hex_to_32(&persisted.seed_hex)?;

    let d: &Daemon = Box::leak(Box::new(Daemon {
        dir: dir.clone(),
        seed,
        accepted: Mutex::new(persisted.accepted.clone()),
        mints: Mutex::new(HashMap::new()),
        persisted: Mutex::new(persisted),
        allow_untrusted,
        allowed_uids: {
            allowed_uids.push(unsafe { libc::getuid() });
            allowed_uids
        },
    }));

    // Boot: open + recover every accepted mint (best-effort; router boots
    // before network — money ops stay fail-closed until recovery runs).
    let boot: Vec<String> = d.accepted.lock().unwrap().clone();
    for m in boot {
        match d.slot_for(&m) {
            Ok(c) => {
                let _ = d.ensure_recovered(&c);
                eprintln!("micronuts-walletd: wallet open for {m}");
            }
            Err(e) => eprintln!("micronuts-walletd: {e} (will retry on first use)"),
        }
    }

    let _ = std::fs::remove_file(&socket);
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    eprintln!(
        "micronuts-walletd {} listening on {socket}",
        env!("CARGO_PKG_VERSION")
    );

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        match peer_uid(&stream) {
            Some(uid) if d.allowed_uids.contains(&uid) => {}
            Some(uid) => {
                eprintln!("micronuts-walletd: refusing connection from uid {uid}");
                continue;
            }
            None => continue,
        }
        let d = &*d;
        std::thread::spawn(move || {
            let stream = match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = handle(d, stream);
        });
    }
    Ok(())
}

fn handle(d: &Daemon, mut stream: std::os::unix::net::UnixStream) -> std::io::Result<()> {
    let reader = std::io::BufReader::new(stream.try_clone()?);
    let mut out = stream;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::err(0, format!("bad request: {e}"));
                let mut b = serde_json::to_vec(&resp).unwrap_or_default();
                b.push(b'\n');
                out.write_all(&b)?;
                continue;
            }
        };
        let (resp, stop) = dispatch(d, &req);
        let mut b = serde_json::to_vec(&resp).unwrap_or_default();
        b.push(b'\n');
        out.write_all(&b)?;
        out.flush()?;
        if stop {
            break;
        }
    }
    Ok(())
}

fn getrandom_fallback(buf: &mut [u8]) -> Result<(), DynError> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_to_32(s: &str) -> Result<[u8; 32], DynError> {
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}
