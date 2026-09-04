use cashu_core_lite::{decode_token, encode_token, Proof, TokenV4, TokenV4Token};

fn sample_token() -> TokenV4 {
    TokenV4 {
        mint: "https://example.com/mint".to_string(),
        unit: "sat".to_string(),
        memo: Some("test memo".to_string()),
        tokens: vec![TokenV4Token {
            keyset_id: "00".to_string(),
            proofs: vec![
                Proof {
                    amount: 2,
                    keyset_id: "00".to_string(),
                    secret: "secret1".to_string(),
                    c: vec![0x02, 0xAB, 0xCD],

                    dleq: None,
                },
                Proof {
                    amount: 8,
                    keyset_id: "00".to_string(),
                    secret: "secret2".to_string(),
                    c: vec![0x02, 0xEF, 0x01],

                    dleq: None,
                },
            ],
        }],
    }
}

#[test]
fn test_encode_decode_roundtrip() {
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");
    let decoded = decode_token(&encoded).expect("should decode");
    assert_eq!(token, decoded);
}

#[test]
fn test_decode_cashu_b_prefix() {
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");

    let mut with_prefix = b"cashuB".to_vec();
    with_prefix.extend(&encoded);

    let decoded = decode_token(&with_prefix).expect("should decode cashuB");
    assert_eq!(token, decoded);
}

#[test]
fn test_decode_craw_b_prefix() {
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");

    let mut with_prefix = b"crawB".to_vec();
    with_prefix.extend(&encoded);

    let decoded = decode_token(&with_prefix).expect("should decode crawB");
    assert_eq!(token, decoded);
}

#[test]
fn test_decode_raw_cbor() {
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");
    let decoded = decode_token(&encoded).expect("should decode raw CBOR");
    assert_eq!(token, decoded);
}

#[test]
fn test_total_amount() {
    let token = sample_token();
    assert_eq!(token.total_amount(), 10);
}

#[test]
fn test_proof_count_multi_token_set() {
    let token = TokenV4 {
        mint: "https://example.com/mint".to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![
            TokenV4Token {
                keyset_id: "00".to_string(),
                proofs: vec![
                    Proof {
                        amount: 1,
                        keyset_id: "00".to_string(),
                        secret: "s1".to_string(),
                        c: vec![],

                        dleq: None,
                    },
                    Proof {
                        amount: 2,
                        keyset_id: "00".to_string(),
                        secret: "s2".to_string(),
                        c: vec![],

                        dleq: None,
                    },
                ],
            },
            TokenV4Token {
                keyset_id: "01".to_string(),
                proofs: vec![Proof {
                    amount: 4,
                    keyset_id: "01".to_string(),
                    secret: "s3".to_string(),
                    c: vec![],

                    dleq: None,
                }],
            },
        ],
    };
    assert_eq!(token.proof_count(), 3);
    assert_eq!(token.total_amount(), 7);
}

#[test]
fn test_empty_token() {
    let token = TokenV4 {
        mint: "https://example.com/mint".to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![],
    };
    assert_eq!(token.total_amount(), 0);
    assert_eq!(token.proof_count(), 0);

    let encoded = encode_token(&token).expect("should encode empty");
    let decoded = decode_token(&encoded).expect("should decode empty");
    assert_eq!(token, decoded);
}

#[test]
fn test_no_memo() {
    let token = TokenV4 {
        mint: "https://example.com/mint".to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: "00".to_string(),
            proofs: vec![Proof {
                amount: 64,
                keyset_id: "00".to_string(),
                secret: "only proof".to_string(),
                c: vec![0x02; 33],

                dleq: None,
            }],
        }],
    };

    let encoded = encode_token(&token).expect("should encode");
    let decoded = decode_token(&encoded).expect("should decode");
    assert_eq!(decoded.memo, None);
    assert_eq!(token, decoded);
}

#[test]
fn test_unknown_prefix_falls_through_to_raw_cbor() {
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");

    let decoded = decode_token(&encoded).expect("should decode as raw CBOR");
    assert_eq!(token, decoded);
}

#[test]
fn test_empty_cbor_input_errors() {
    let result = decode_token(&[]);
    assert!(result.is_err());
}

#[test]
fn test_total_amount_zero_proofs() {
    let token = TokenV4 {
        mint: "https://example.com/mint".to_string(),
        unit: "sat".to_string(),
        memo: None,
        tokens: vec![TokenV4Token {
            keyset_id: "00".to_string(),
            proofs: vec![],
        }],
    };
    assert_eq!(token.total_amount(), 0);
    assert_eq!(token.proof_count(), 0);
}

#[test]
fn test_decode_cashu_b_base64url() {
    let token = sample_token();
    let raw_cbor = encode_token(&token).expect("should encode");

    let b64 = base64_encode(&raw_cbor);
    let mut with_prefix = b"cashuB".to_vec();
    with_prefix.extend_from_slice(b64.as_bytes());

    let decoded = decode_token(&with_prefix).expect("should decode cashuB + base64url");
    assert_eq!(token, decoded);
}

// The NUT-00 V4 spec example token (nuts/00.md, "V4 tokens") — the
// cross-implementation ground truth also used by the cashu-ts conformance
// harness selftest. cashu-ts 4.10.0 decodes this exact string.
const NUT00_SPEC_EXAMPLE: &str = "cashuBo2F0gqJhaUgA_9SLj17PgGFwgaNhYQFhc3hAYWNjMTI0MzVlN2I4NDg0YzNjZjE4NTAxNDkyMThhZjkwZjcxNmE1MmJmNGE1ZWQzNDdlNDhlY2MxM2Y3NzM4OGFjWCECRFODGd5IXVW-07KaZCvuWHk3WrnnpiDhHki6SCQh88-iYWlIAK0mjE0fWCZhcIKjYWECYXN4QDEzMjNkM2Q0NzA3YTU4YWQyZTIzYWRhNGU5ZjFmNDlmNWE1YjRhYzdiNzA4ZWIwZDYxZjczOGY0ODMwN2U4ZWVhY1ghAjRWqhENhLSsdHrr2Cw7AFrKUL9Ffr1XN6RBT6w659lNo2FhAWFzeEA1NmJjYmNiYjdjYzY0MDZiM2ZhNWQ1N2QyMTc0ZjRlZmY4YjQ0MDJiMTc2OTI2ZDNhNTdkM2MzZGNiYjU5ZDU3YWNYIQJzEpxXGeWZN5qXSmJjY8MzxWyvwObQGr5G1YCCgHicY2FtdWh0dHA6Ly9sb2NhbGhvc3Q6MzMzOGF1Y3NhdA";

#[test]
fn test_decode_nut00_spec_example() {
    let token = decode_token(NUT00_SPEC_EXAMPLE.as_bytes()).expect("spec example must decode");
    assert_eq!(token.mint, "http://localhost:3338");
    assert_eq!(token.unit, "sat");
    assert_eq!(token.memo, None);
    assert_eq!(token.proof_count(), 3);
    assert_eq!(token.total_amount(), 4);

    assert_eq!(token.tokens.len(), 2);
    assert_eq!(token.tokens[0].keyset_id, "00ffd48b8f5ecf80");
    assert_eq!(token.tokens[1].keyset_id, "00ad268c4d1f5826");

    // Every proof inherits its group's keyset id.
    assert!(token.tokens[0]
        .proofs
        .iter()
        .all(|p| p.keyset_id == "00ffd48b8f5ecf80"));
    assert!(token.tokens[1]
        .proofs
        .iter()
        .all(|p| p.keyset_id == "00ad268c4d1f5826"));

    // C values arrive as 33-byte compressed points.
    for p in token.tokens.iter().flat_map(|t| t.proofs.iter()) {
        assert_eq!(p.c.len(), 33, "C must be a 33-byte compressed point");
        assert_eq!(p.dleq, None, "spec example carries no dleq");
    }
}

#[test]
fn test_encode_uses_nut00_single_char_string_keys() {
    // NUT-00 V4: "All keys are single characters and hex strings are
    // encoded in binary." The encoded top-level map must carry text keys
    // m/u/t — integer-keyed CBOR is the wire bug cashu-ts rejects with
    // "Invalid token template" (found 2026-09-04, first external parse).
    let token = sample_token();
    let encoded = encode_token(&token).expect("should encode");

    let mut d = minicbor::Decoder::new(&encoded);
    let n = d
        .map()
        .expect("top level must be a map")
        .expect("definite map");
    assert_eq!(n, 4, "m, u, d (memo), t");
    let mut keys = Vec::new();
    for _ in 0..n {
        keys.push(d.str().expect("keys must be text strings").to_string());
        d.skip().expect("value");
    }
    for k in ["m", "u", "d", "t"] {
        assert!(
            keys.contains(&k.to_string()),
            "missing single-char key '{k}' in {keys:?}"
        );
    }

    let decoded = decode_token(&encoded).expect("should decode");
    assert_eq!(token, decoded);
}

#[test]
fn test_spec_example_roundtrip_is_stable() {
    let token = decode_token(NUT00_SPEC_EXAMPLE.as_bytes()).expect("spec example must decode");
    let encoded = encode_token(&token).expect("should encode");
    let again = decode_token(&encoded).expect("re-decode");
    assert_eq!(token, again);
}

#[test]
fn test_decode_cashu_b_base64url_without_padding() {
    let token = sample_token();
    let raw_cbor = encode_token(&token).expect("should encode");

    let b64 = base64_encode(&raw_cbor);
    let b64_trimmed: String = b64.trim_end_matches('=').to_string();

    let mut with_prefix = b"cashuB".to_vec();
    with_prefix.extend_from_slice(b64_trimmed.as_bytes());

    let decoded = decode_token(&with_prefix).expect("should decode without padding");
    assert_eq!(token, decoded);
}

#[test]
fn test_decode_raw_cbor_still_works() {
    let token = sample_token();
    let raw_cbor = encode_token(&token).expect("should encode");
    let decoded = decode_token(&raw_cbor).expect("raw CBOR still decodes");
    assert_eq!(token, decoded);
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[test]
fn test_encode_token_wire_roundtrip() {
    let token = sample_token();
    let wire = cashu_core_lite::encode_token_wire(&token).expect("should encode wire");
    assert!(wire.starts_with("cashuB"));
    assert!(!wire.contains('='), "base64url must be unpadded");
    let decoded = decode_token(wire.as_bytes()).expect("should decode wire form");
    assert_eq!(token, decoded);
}
