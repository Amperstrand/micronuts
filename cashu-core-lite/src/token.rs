#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::vec::Vec;

use core::convert::Infallible;
use minicbor::{Decode, Encode};

/// Decode a base64url-encoded string to bytes.
///
/// Handles both standard base64 and base64url alphabets, with or without
/// padding. Returns `None` if the input contains invalid characters.
fn decode_base64url(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() {
        return Some(Vec::new());
    }

    const DECODE_TABLE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (26 + i) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            table[(b'0' + i) as usize] = (52 + i) as i8;
            i += 1;
        }
        table[b'+' as usize] = 62;
        table[b'-' as usize] = 62;
        table[b'/' as usize] = 63;
        table[b'_' as usize] = 63;
        table
    };

    let mut len = input.len();
    while len > 0 && input[len - 1] == b'=' {
        len -= 1;
    }
    let input = &input[..len];

    let full_groups = input.len() / 4;
    let remainder = input.len() % 4;

    let mut output = Vec::with_capacity(
        full_groups * 3
            + match remainder {
                2 => 1,
                3 => 2,
                _ => 0,
            },
    );

    for i in 0..full_groups {
        let off = i * 4;
        let a = DECODE_TABLE[input[off] as usize];
        let b = DECODE_TABLE[input[off + 1] as usize];
        let c = DECODE_TABLE[input[off + 2] as usize];
        let d = DECODE_TABLE[input[off + 3] as usize];

        if a < 0 || b < 0 || c < 0 || d < 0 {
            return None;
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));
        output.push(((b as u8 & 0x0F) << 4) | ((c as u8) >> 2));
        output.push(((c as u8 & 0x03) << 6) | (d as u8));
    }

    if remainder >= 2 {
        let off = full_groups * 4;
        let a = DECODE_TABLE[input[off] as usize];
        let b = DECODE_TABLE[input[off + 1] as usize];

        if a < 0 || b < 0 {
            return None;
        }

        output.push(((a as u8) << 2) | ((b as u8) >> 4));

        if remainder == 3 {
            let c = DECODE_TABLE[input[off + 2] as usize];
            if c < 0 {
                return None;
            }
            output.push(((b as u8 & 0x0F) << 4) | ((c as u8) >> 2));
        }
    }

    Some(output)
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Proof {
    #[n(0)]
    pub amount: u64,

    #[n(1)]
    pub keyset_id: String,

    #[n(2)]
    pub secret: String,

    /// `C`: the unblinded signature, compressed-point bytes.
    #[n(3)]
    pub c: Vec<u8>,

    /// NUT-12 proof-level DLEQ (e, s, r) — enables public-key-only
    /// offline verification of this proof.
    #[n(4)]
    pub dleq: Option<crate::nuts::nut12::ProofDleq>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TokenV4Token {
    #[n(0)]
    pub keyset_id: String,

    #[n(1)]
    pub proofs: Vec<Proof>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TokenV4 {
    #[n(0)]
    pub mint: String,

    #[n(1)]
    pub unit: String,

    #[n(2)]
    pub memo: Option<String>,

    #[n(3)]
    pub tokens: Vec<TokenV4Token>,
}

impl TokenV4 {
    pub fn total_amount(&self) -> u64 {
        self.tokens
            .iter()
            .flat_map(|t| t.proofs.iter())
            .map(|p| p.amount)
            .sum()
    }

    pub fn proof_count(&self) -> usize {
        self.tokens.iter().map(|t| t.proofs.len()).sum()
    }
}

pub fn decode_token(data: &[u8]) -> Result<TokenV4, minicbor::decode::Error> {
    let payload = if let Some(stripped) = data.strip_prefix(b"cashuB") {
        stripped
    } else if let Some(stripped) = data.strip_prefix(b"crawB") {
        stripped
    } else {
        data
    };

    // Standard Cashu V4 tokens are base64url-encoded after the prefix.
    // Try base64url first; fall back to raw CBOR for backward compat.
    if let Some(decoded) = decode_base64url(payload) {
        if let Ok(token) = minicbor::decode(&decoded) {
            return Ok(token);
        }
    }

    minicbor::decode(payload)
}

pub fn encode_token(token: &TokenV4) -> Result<Vec<u8>, minicbor::encode::Error<Infallible>> {
    let mut buf = Vec::new();
    minicbor::encode(token, &mut buf)?;
    Ok(buf)
}
