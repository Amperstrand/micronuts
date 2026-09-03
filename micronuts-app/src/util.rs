extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use cashu_core_lite::{PublicKey, SecretKey};
use sha2::{Digest, Sha256};

pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
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

// #54: the swap flow's trust root is a PINNED key (the host-mint-tool demo
// mint's fixed seed), never a key derived from the imported token's mint
// URL — whoever chooses the URL would know the private key.
// Failure carries no diagnostic; callers treat any miss as fatal.
#[allow(clippy::result_unit_err)]
pub fn pinned_demo_mint_key() -> Result<PublicKey, ()> {
    let seed = Sha256::digest(b"demo://micronuts");
    let sk = SecretKey::from_slice(&seed).map_err(|_| ())?;
    Ok(sk.public_key())
}
