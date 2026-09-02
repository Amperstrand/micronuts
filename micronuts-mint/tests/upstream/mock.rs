//! Minimal std-only HTTP mock of an upstream Cashu mint.
//!
//! Serves the four endpoints the upstream backend uses, with REAL
//! blind-signature crypto (`C_ = k * B_` via cashu-core-lite) so the
//! reserve bootstrap and change-unblind paths are exercised truthfully.
//! One connection per request (`Connection: close`) — ureq reconnects.
//! Raw HTTP framing lives in `http.rs`.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use cashu_core_lite::crypto::sign_message;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use rand::RngCore;
use serde_json::{json, Value};

mod http;

use http::{read_request, write_response};

pub const KEYSET_ID: &str = "mockks01";
pub const MELT_PREIMAGE: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

/// Behavior of `POST /v1/melt/bolt11`.
#[derive(Clone, Copy)]
pub enum MeltMode {
    Paid,
    Pending,
    Failed,
}

struct State {
    mint_quote_polls: HashMap<String, u32>,
    melt_quote_amounts: HashMap<String, u64>,
    melt_mode: MeltMode,
    fail_next_poll: bool,
    garbage: bool,
    counters: u64,
    melt_quotes_created: u32,
}

pub struct MockUpstream {
    pub base_url: String,
    state: Arc<Mutex<State>>,
}

impl State {
    fn new(melt_mode: MeltMode, garbage: bool) -> Self {
        Self {
            mint_quote_polls: HashMap::new(),
            melt_quote_amounts: HashMap::new(),
            melt_mode,
            fail_next_poll: false,
            garbage,
            counters: 0,
            melt_quotes_created: 0,
        }
    }
}

impl MockUpstream {
    pub fn start() -> Self {
        Self::with_melt_mode(MeltMode::Paid)
    }

    pub fn with_melt_mode(mode: MeltMode) -> Self {
        Self::spawn(State::new(mode, false))
    }

    /// Every response is unparseable garbage (shape-error coverage).
    pub fn garbage() -> Self {
        Self::spawn(State::new(MeltMode::Paid, true))
    }

    fn spawn(state: State) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock address");
        let state = Arc::new(Mutex::new(state));
        let thread_state = Arc::clone(&state);
        std::thread::spawn(move || serve(listener, thread_state));
        Self {
            base_url: format!("http://{addr}"),
            state,
        }
    }

    pub fn fail_next_mint_quote_poll(&self) {
        self.state.lock().expect("state").fail_next_poll = true;
    }

    pub fn melt_quotes_created(&self) -> u32 {
        self.state.lock().expect("state").melt_quotes_created
    }
}

fn serve(listener: TcpListener, state: Arc<Mutex<State>>) {
    let mut keys = HashMap::new();
    let mut denomination = 1u64;
    while denomination <= 1024 {
        keys.insert(denomination, random_secret());
        denomination <<= 1;
    }
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { break };
        let request = read_request(&mut stream);
        let (status, body) = route(&request, &keys, &state);
        write_response(&mut stream, status, &body);
    }
}

fn random_secret() -> SecretKey {
    loop {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        if let Ok(sk) = SecretKey::from_slice(&bytes) {
            return sk;
        }
    }
}

fn route(
    request: &http::Request,
    keys: &HashMap<u64, SecretKey>,
    state: &Arc<Mutex<State>>,
) -> (&'static str, String) {
    let mut state = state.lock().expect("state");
    if state.garbage {
        return ("200 OK", "not-json{{garbage".to_string());
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/keys") => (
            "200 OK",
            json!({
                "keysets": [{
                    "id": KEYSET_ID,
                    "unit": "sat",
                    "active": true,
                    "input_fee_ppk": 0,
                    "keys": keyset_json(keys),
                }]
            })
            .to_string(),
        ),
        ("POST", "/v1/mint/quote/bolt11") => {
            let body = parse_json(request.body.as_deref());
            let amount = body["amount"].as_u64().expect("mock mint-quote amount");
            let quote = next_id(&mut state.counters, "mq");
            (
                "201 Created",
                json!({
                    "quote": quote,
                    "request": format!("lnbcmock{amount}sat1mockup"),
                    "state": "UNPAID",
                    "amount": amount,
                    "unit": "sat",
                    "expiry": 4294967295u64,
                })
                .to_string(),
            )
        }
        ("GET", path) if path.starts_with("/v1/mint/quote/bolt11/") => {
            let quote = path.rsplit('/').next().expect("quote id");
            if state.fail_next_poll {
                state.fail_next_poll = false;
                return (
                    "500 Internal Server Error",
                    "{\"error\":\"blip\"}".to_string(),
                );
            }
            let polls = state.mint_quote_polls.entry(quote.to_string()).or_insert(0);
            *polls += 1;
            let settled = if *polls >= 2 { "PAID" } else { "UNPAID" };
            (
                "200 OK",
                json!({"quote": quote, "state": settled, "paid": settled == "PAID"}).to_string(),
            )
        }
        ("POST", "/v1/melt/quote/bolt11") => {
            let body = parse_json(request.body.as_deref());
            let invoice = body["request"].as_str().expect("mock melt invoice");
            let amount = mock_invoice_amount(invoice).expect("parseable mock invoice");
            let quote = next_id(&mut state.counters, "melq");
            state.melt_quote_amounts.insert(quote.clone(), amount);
            state.melt_quotes_created += 1;
            (
                "201 Created",
                json!({
                    "quote": quote,
                    // Amount as a decimal STRING: covers the number-or-string
                    // wire tolerance on this endpoint.
                    "amount": amount.to_string(),
                    "fee_reserve": 0,
                    "state": "UNPAID",
                    "unit": "sat",
                })
                .to_string(),
            )
        }
        ("POST", "/v1/mint/bolt11") => {
            let body = parse_json(request.body.as_deref());
            let outputs = body["outputs"].as_array().expect("mock outputs");
            let signatures: Vec<Value> = outputs.iter().map(|o| sign_output(o, keys)).collect();
            ("200 OK", json!({"signatures": signatures}).to_string())
        }
        ("POST", "/v1/melt/bolt11") => match state.melt_mode {
            MeltMode::Paid => {
                let body = parse_json(request.body.as_deref());
                // Real-mint NUT-08 behavior: amount-0 blanks are imprinted
                // with the overpay decomposition (largest denomination
                // first, unfilled blanks dropped); explicit outputs are
                // signed as-is.
                let quote_amount = match state
                    .melt_quote_amounts
                    .get(body["quote"].as_str().unwrap_or_default())
                {
                    Some(amount) => *amount,
                    None => {
                        return (
                            "400 Bad Request",
                            json!({"code": "QUOTE_NOT_FOUND"}).to_string(),
                        )
                    }
                };
                let input_sum: u64 = body["inputs"]
                    .as_array()
                    .map(|inputs| inputs.iter().filter_map(|p| p["amount"].as_u64()).sum())
                    .unwrap_or(0);
                let mut remaining = input_sum.saturating_sub(quote_amount);
                let change: Vec<Value> = body["outputs"]
                    .as_array()
                    .map(|outputs| {
                        outputs
                            .iter()
                            .filter_map(|o| {
                                let mut filled = o.clone();
                                if o["amount"].as_u64() == Some(0) {
                                    let denom =
                                        keys.keys().copied().filter(|&k| k <= remaining).max()?;
                                    remaining -= denom;
                                    filled["amount"] = json!(denom);
                                }
                                Some(sign_output(&filled, keys))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    "200 OK",
                    json!({
                        "paid": true,
                        "state": "PAID",
                        "payment_preimage": MELT_PREIMAGE,
                        "change": change,
                    })
                    .to_string(),
                )
            }
            MeltMode::Pending => (
                "200 OK",
                json!({"paid": false, "state": "PENDING", "payment_preimage": null, "change": []})
                    .to_string(),
            ),
            MeltMode::Failed => (
                "200 OK",
                json!({"paid": false, "state": "FAILED", "payment_preimage": null, "change": []})
                    .to_string(),
            ),
        },
        _ => (
            "404 Not Found",
            "{\"detail\":\"mock: unknown route\"}".to_string(),
        ),
    }
}

fn keyset_json(keys: &HashMap<u64, SecretKey>) -> Value {
    let mut entries: Vec<(u64, Value)> = keys
        .iter()
        .map(|(amount, sk)| {
            (
                *amount,
                Value::String(hex::encode(sk.public_key().to_bytes())),
            )
        })
        .collect();
    entries.sort_by_key(|(amount, _)| *amount);
    let map: serde_json::Map<String, Value> = entries
        .into_iter()
        .map(|(amount, pk)| (amount.to_string(), pk))
        .collect();
    Value::Object(map)
}

fn sign_output(output: &Value, keys: &HashMap<u64, SecretKey>) -> Value {
    let amount = output["amount"].as_u64().expect("mock output amount");
    let b_hex = output["B_"].as_str().expect("mock output B_");
    let bytes = hex::decode(b_hex).expect("mock B_ hex");
    let compressed: [u8; 33] = bytes.try_into().expect("mock B_ 33 bytes");
    let blinded = PublicKey::from_bytes(&compressed).expect("mock B_ point");
    let sk = keys.get(&amount).expect("mock denomination key");
    let c_prime = sign_message(sk, &blinded);
    json!({
        "amount": amount,
        "id": KEYSET_ID,
        "C_": hex::encode(c_prime.to_bytes()),
    })
}

fn next_id(counters: &mut u64, prefix: &str) -> String {
    *counters += 1;
    format!("{prefix}{counters}")
}

fn mock_invoice_amount(invoice: &str) -> Option<u64> {
    let stripped = invoice.strip_prefix("lnbcmock")?;
    let end = stripped.find("sat")?;
    stripped[..end].parse().ok()
}

fn parse_json(body: Option<&str>) -> Value {
    serde_json::from_str(body.expect("mock request body")).expect("mock request JSON")
}
