extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use k256::{PublicKey, SecretKey};
use sha2::{Digest, Sha256};

pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut result = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        result.push(byte);
    }
    Some(result)
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX_CHARS[(*byte >> 4) as usize] as char);
        result.push(HEX_CHARS[(*byte & 0x0F) as usize] as char);
    }
    result
}

pub fn derive_demo_mint_key(token: &Option<cashu_core_lite::TokenV4>) -> Result<PublicKey, ()> {
    let mint_url = token
        .as_ref()
        .map(|t| t.mint.as_str())
        .unwrap_or("demo://micronuts");

    let mut hasher = Sha256::new();
    Digest::update(&mut hasher, mint_url.as_bytes());
    let seed = Digest::finalize(hasher);

    let sk = SecretKey::from_slice(&seed).map_err(|_| ())?;
    Ok(sk.public_key())
}
