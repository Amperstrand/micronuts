#![no_main]
//! Fuzz the public wire path: arbitrary text -> decode_token_or_err.
//! Property under test: anything that decodes must re-encode and
//! re-decode to the identical token (roundtrip stability).

use libfuzzer_sys::fuzz_target;
use walletport::{decode_token_or_err, encode_token_wire};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(tok) = decode_token_or_err(s) {
        let wire = encode_token_wire(&tok).expect("encode of decoded token must succeed");
        let again = decode_token_or_err(&wire).expect("re-decode of encoded token must succeed");
        assert_eq!(again, tok, "token roundtrip must be stable");
    }
});
