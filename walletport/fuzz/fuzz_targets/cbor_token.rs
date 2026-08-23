#![no_main]
//! Fuzz the raw CBOR token decoder (no base64 layer): arbitrary bytes
//! straight into cashu-core-lite's token::decode_token. Invariant: no
//! panics; every input yields Ok or Err.

use cashu_core_lite::token;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = token::decode_token(data);
});
