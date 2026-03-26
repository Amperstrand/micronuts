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
                },
                Proof {
                    amount: 8,
                    keyset_id: "00".to_string(),
                    secret: "secret2".to_string(),
                    c: vec![0x02, 0xEF, 0x01],
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
                    },
                    Proof {
                        amount: 2,
                        keyset_id: "00".to_string(),
                        secret: "s2".to_string(),
                        c: vec![],
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
