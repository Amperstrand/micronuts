//! Shared test fixtures (test/sign builds only).

extern crate alloc;

use alloc::string::String;
use alloc::vec;

use cashu_core_lite::{Proof, TokenV4, TokenV4Token};

/// A minimal 2-proof, 10-sat token — mirrors the shape used by the
/// command-handler tests and the shotui example.
pub fn minimal_token() -> TokenV4 {
    TokenV4 {
        mint: String::from("https://example.com/mint"),
        unit: String::from("sat"),
        memo: Some(String::from("test memo")),
        tokens: vec![TokenV4Token {
            keyset_id: String::from("00"),
            proofs: vec![
                Proof {
                    amount: 2,
                    keyset_id: String::from("00"),
                    secret: String::from("aabbccdd"),
                    c: vec![0x02, 0xAB, 0xCD],
                    dleq: None,
                },
                Proof {
                    amount: 8,
                    keyset_id: String::from("00"),
                    secret: String::from("11223344"),
                    c: vec![0x02, 0xEF, 0x01],
                    dleq: None,
                },
            ],
        }],
    }
}
