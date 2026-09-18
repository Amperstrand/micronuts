//! `HttpMintClient` wire tests against a one-shot mock mint speaking
//! minimal HTTP/1.1 — asserting routes, JSON field names (`B_`, `C`, `Ys`),
//! response parsing, and error-code mapping.

use micronuts_wallet::http::http_mint_client;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut03, nut04, nut07};
use cashu_core_lite::transport::MintClient;
use micronuts_wallet::http::HttpMintClient;
/// Compressed secp256k1 generator (a valid point for B_/C_/C/Y fields).
const G: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
/// 2*G.
const G2: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
/// Valid secp256k1 scalars for dleq e/s fields.
const SCALAR_1: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const SCALAR_2: &str = "0000000000000000000000000000000000000000000000000000000000000002";

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Serve exactly one request; capture the raw request (request line +
/// headers + body) on the channel. Returns the base URL.
fn spawn_mock(status: u16, body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock mint");
    let addr = listener.local_addr().expect("mock addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        let response = format!(
            "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            reason(status),
            body.len(),
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
        let _ = tx.send(request);
    });
    (format!("http://{addr}"), rx)
}

/// Read a full HTTP request: headers, then `Content-Length` bytes of body.
fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut chunk = [0u8; 2048];
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    loop {
        let n = stream.read(&mut chunk).expect("read request");
        if n == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n]);
        let Some(header_end) = data.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if data.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn captured(rx: &mpsc::Receiver<String>) -> String {
    rx.recv_timeout(Duration::from_secs(5)).expect("capture")
}

fn point(hex_point: &str) -> cashu_core_lite::PublicKey {
    let bytes: [u8; 33] = hex::decode(hex_point).unwrap().try_into().unwrap();
    cashu_core_lite::PublicKey::from_bytes(&bytes).unwrap()
}

#[test]
fn get_keys_parses_amount_keyed_map_sorted_ascending() {
    let body = r#"{"keysets":[{"id":"00aa","unit":"sat","keys":{"2":"G2PH","1":"GPH"}}]}"#
        .replace("G2PH", G2)
        .replace("GPH", G);
    let body = body.leak();
    let (base, _rx) = spawn_mock(200, body);
    let mut client = http_mint_client(&base);
    let keys = client.get_keys().expect("keys");
    assert_eq!(keys.keysets.len(), 1);
    let keyset = &keys.keysets[0];
    assert_eq!(keyset.id, "00aa");
    assert_eq!(keyset.keys.len(), 2);
    assert_eq!(keyset.keys[0].amount, 1);
    assert_eq!(keyset.keys[1].amount, 2);
    assert_eq!(
        hex::encode(keyset.keys[0].pubkey.to_bytes()),
        G.to_lowercase()
    );
}

#[test]
fn mint_quote_posts_amount_unit_and_parses_response() {
    let body = r#"{
        "quote":"q1","request":"lnbc21","paid":true,"state":"PAID",
        "expiry":9999999999,"amount":21,"unit":"sat",
        "amount_paid":21,"amount_issued":0,"updated_at":42,"method":"bolt11"
    }"#;
    let (base, rx) = spawn_mock(200, body);
    let mut client = http_mint_client(&base);
    let quote = client
        .post_mint_quote(nut04::MintQuoteRequest {
            amount: 21,
            unit: String::from("sat"),
            pubkey: None,
        })
        .expect("quote");
    let request = captured(&rx);
    assert!(
        request.starts_with("POST /v1/mint/quote/bolt11 "),
        "{request}"
    );
    assert!(request.contains("\"amount\":21"), "{request}");
    assert!(request.contains("\"unit\":\"sat\""), "{request}");
    assert_eq!(quote.quote, "q1");
    assert_eq!(quote.request, "lnbc21");
    assert!(quote.paid);
    assert_eq!(quote.state, "PAID");
    assert_eq!(quote.expiry, 9999999999);
    assert_eq!(quote.amount, 21);
    assert_eq!(quote.updated_at, 42);
}

#[test]
fn swap_uses_spec_field_names_and_parses_dleq() {
    let body = format!(
        r#"{{"signatures":[{{"amount":8,"id":"00aa","C_":"{G}","dleq":{{"e":"{SCALAR_1}","s":"{SCALAR_2}"}}}}]}}"#
    )
    .leak();
    let (base, rx) = spawn_mock(200, body);
    let mut client = http_mint_client(&base);

    let proof = proof_fixture(8);
    let output = blinded_fixture(8);
    let request = nut03::SwapRequest {
        inputs: vec![proof],
        outputs: vec![output],
    };
    let response = client.post_swap(request).expect("swap");

    let raw = captured(&rx);
    assert!(raw.starts_with("POST /v1/swap "), "{raw}");
    assert!(raw.contains("\"B_\":"), "blinded field must be B_: {raw}");
    assert!(raw.contains("\"C\":"), "proof field must be C: {raw}");
    assert!(raw.contains("\"secret\":\"deadbeef\""), "{raw}");
    assert!(
        !raw.contains("\"b\":"),
        "lowercase b must not appear: {raw}"
    );
    assert!(raw.contains("\"inputs\":"), "{raw}");
    assert!(raw.contains("\"outputs\":"), "{raw}");

    assert_eq!(response.signatures.len(), 1);
    let sig = &response.signatures[0];
    assert_eq!(sig.amount, 8);
    assert_eq!(sig.id, "00aa");
    assert_eq!(hex::encode(sig.c.to_bytes()), G.to_lowercase());
    let dleq = sig.dleq.as_ref().expect("dleq parsed");
    assert_eq!(hex::encode(dleq.e.to_secret_bytes()), SCALAR_1);
    assert_eq!(hex::encode(dleq.s.to_secret_bytes()), SCALAR_2);
}

fn proof_fixture(amount: u64) -> nut00::Proof {
    let bytes: [u8; 33] = hex::decode(G).unwrap().try_into().unwrap();
    nut00::Proof {
        amount,
        id: String::from("00aa"),
        secret: String::from("deadbeef"),
        c: cashu_core_lite::PublicKey::from_bytes(&bytes).unwrap(),
        dleq: None,
        witness: None,
    }
}

fn blinded_fixture(amount: u64) -> nut00::BlindedMessage {
    let bytes: [u8; 33] = hex::decode(G2).unwrap().try_into().unwrap();
    nut00::BlindedMessage {
        amount,
        id: String::from("00aa"),
        b: cashu_core_lite::PublicKey::from_bytes(&bytes).unwrap(),
    }
}

#[test]
fn error_status_maps_code_back_to_cashu_error() {
    let body = r#"{"detail":"nope","code":"KEYSET_NOT_FOUND","error_kind":"KEYSET_NOT_FOUND"}"#;
    let (base, _rx) = spawn_mock(404, body);
    let mut client = http_mint_client(&base);
    let err = client.get_keys().expect_err("must fail");
    assert_eq!(err, CashuError::KeysetNotFound);
}

#[test]
fn unknown_error_code_becomes_protocol_with_detail() {
    let body = r#"{"detail":"mystery","code":"SOMETHING_ELSE"}"#;
    let (base, _rx) = spawn_mock(500, body);
    let mut client = http_mint_client(&base);
    let err = client.get_keysets().expect_err("must fail");
    match err {
        CashuError::Protocol(detail) => assert_eq!(detail, "mystery"),
        other => panic!("expected Protocol, got {other:?}"),
    }
}

#[test]
fn checkstate_posts_capital_ys_and_parses_states() {
    let body = format!(r#"{{"states":[{{"Y":"{G}","state":"SPENT","witness":null}}]}}"#).leak();
    let (base, rx) = spawn_mock(200, body);
    let mut client = http_mint_client(&base);

    let request = nut07::CheckStateRequest { ys: vec![point(G)] };
    let response = client.post_check_state(request).expect("checkstate");

    let raw = captured(&rx);
    assert!(raw.starts_with("POST /v1/checkstate "), "{raw}");
    assert!(raw.contains("\"Ys\":"), "must send capital Ys: {raw}");

    assert_eq!(response.states.len(), 1);
    assert_eq!(response.states[0].state, "SPENT");
    assert_eq!(
        hex::encode(response.states[0].y.to_bytes()),
        G.to_lowercase()
    );
}

#[test]
fn melt_quote_lookup_builds_get_path() {
    let body = r#"{
        "quote":"m1","amount":30,"fee_reserve":0,"paid":false,
        "state":"UNPAID","expiry":123,"request":"lnbc30","unit":"sat","method":"bolt11"
    }"#;
    let (base, rx) = spawn_mock(200, body);
    let mut client = http_mint_client(&base);
    let quote = client.get_melt_quote("m/1 x").expect("melt quote");
    let raw = captured(&rx);
    assert!(
        raw.starts_with("GET /v1/melt/quote/bolt11/m%2F1%20x "),
        "quote id must be url-encoded: {raw}"
    );
    assert_eq!(quote.amount, 30);
    assert_eq!(quote.state, "UNPAID");
    assert_eq!(quote.unit, "sat");
}
