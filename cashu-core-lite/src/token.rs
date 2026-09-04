#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::string::String;
#[cfg(feature = "std")]
use std::vec::Vec;

use core::convert::Infallible;
use minicbor::data::Type;
use minicbor::encode::{Error, Write};
use minicbor::{Decode, Decoder, Encoder};

// The `i` (keyset id), `c` (signature) and DLEQ scalar values are byte strings
// on the wire; a proof's keyset id is not repeated per proof — it lives once on
// the enclosing group's `i` and is re-attached to each proof on decode.
// NUT #00: V4 tokens are a space-efficient way of serializing tokens using the CBOR binary format. All keys are single characters and hex strings are encoded in binary.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub amount: u64,

    pub keyset_id: String,

    pub secret: String,

    /// `C`: the unblinded signature, compressed-point bytes.
    pub c: Vec<u8>,

    /// NUT-12 proof-level DLEQ (e, s, r) — enables public-key-only
    /// offline verification of this proof.
    pub dleq: Option<crate::nuts::nut12::ProofDleq>,
}

impl<C> minicbor::Encode<C> for Proof {
    fn encode<W: Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), Error<W::Error>> {
        e.map(match &self.dleq {
            Some(_) => 4,
            None => 3,
        })?
        .str("a")?
        .u64(self.amount)?
        .str("s")?
        .str(&self.secret)?
        .str("c")?
        .bytes(&self.c)?;
        if let Some(dleq) = &self.dleq {
            e.str("d")?;
            dleq.encode(e, &mut ())?;
        }
        Ok(())
    }
}

impl<'b, C> Decode<'b, C> for Proof {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let mut amount = None;
        let mut secret = None;
        let mut c = None;
        let mut dleq = None;
        foreach_key(d, |d, key| {
            match key {
                "a" => amount = Some(d.u64()?),
                "s" => secret = Some(String::from(d.str()?)),
                "c" => c = Some(d.bytes()?.to_vec()),
                "d" => dleq = Some(d.decode()?),
                _ => d.skip()?,
            }
            Ok(())
        })?;
        Ok(Proof {
            amount: amount.ok_or_else(|| minicbor::decode::Error::message("missing 'a'"))?,
            keyset_id: String::new(),
            secret: secret.ok_or_else(|| minicbor::decode::Error::message("missing 's'"))?,
            c: c.ok_or_else(|| minicbor::decode::Error::message("missing 'c'"))?,
            dleq,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenV4Token {
    pub keyset_id: String,

    pub proofs: Vec<Proof>,
}

impl<C> minicbor::Encode<C> for TokenV4Token {
    fn encode<W: Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), Error<W::Error>> {
        let id_bytes =
            hex::decode(&self.keyset_id).map_err(|_| Error::message("keyset id is not hex"))?;
        e.map(2)?
            .str("i")?
            .bytes(&id_bytes)?
            .str("p")?
            .array(self.proofs.len() as u64)?;
        for proof in &self.proofs {
            proof.encode(e, &mut ())?;
        }
        Ok(())
    }
}

impl<'b, C> Decode<'b, C> for TokenV4Token {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let mut keyset_id = None;
        let mut proofs: Option<Vec<Proof>> = None;
        foreach_key(d, |d, key| {
            match key {
                "i" => keyset_id = Some(hex::encode(d.bytes()?)),
                "p" => proofs = Some(decode_array(d)?),
                _ => d.skip()?,
            }
            Ok(())
        })?;
        let keyset_id = keyset_id.ok_or_else(|| minicbor::decode::Error::message("missing 'i'"))?;
        let mut proofs = proofs.ok_or_else(|| minicbor::decode::Error::message("missing 'p'"))?;
        for proof in &mut proofs {
            proof.keyset_id = keyset_id.clone();
        }
        Ok(TokenV4Token { keyset_id, proofs })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenV4 {
    pub mint: String,

    pub unit: String,

    pub memo: Option<String>,

    pub tokens: Vec<TokenV4Token>,
}

impl<C> minicbor::Encode<C> for TokenV4 {
    fn encode<W: Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), Error<W::Error>> {
        e.map(match &self.memo {
            Some(_) => 4,
            None => 3,
        })?
        .str("m")?
        .str(&self.mint)?
        .str("u")?
        .str(&self.unit)?;
        if let Some(memo) = &self.memo {
            e.str("d")?.str(memo)?;
        }
        e.str("t")?.array(self.tokens.len() as u64)?;
        for group in &self.tokens {
            group.encode(e, &mut ())?;
        }
        Ok(())
    }
}

impl<'b, C> Decode<'b, C> for TokenV4 {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let mut mint = None;
        let mut unit = None;
        let mut memo = None;
        let mut tokens = None;
        foreach_key(d, |d, key| {
            match key {
                "m" => mint = Some(String::from(d.str()?)),
                "u" => unit = Some(String::from(d.str()?)),
                "d" => memo = Some(String::from(d.str()?)),
                "t" => tokens = Some(decode_array(d)?),
                _ => d.skip()?,
            }
            Ok(())
        })?;
        Ok(TokenV4 {
            mint: mint.ok_or_else(|| minicbor::decode::Error::message("missing 'm'"))?,
            unit: unit.ok_or_else(|| minicbor::decode::Error::message("missing 'u'"))?,
            memo,
            tokens: tokens.ok_or_else(|| minicbor::decode::Error::message("missing 't'"))?,
        })
    }
}

fn foreach_key<'b, F>(d: &mut Decoder<'b>, mut on_entry: F) -> Result<(), minicbor::decode::Error>
where
    F: FnMut(&mut Decoder<'b>, &str) -> Result<(), minicbor::decode::Error>,
{
    let len = d.map()?;
    match len {
        Some(n) => {
            for _ in 0..n {
                let key = d.str()?;
                on_entry(d, key)?;
            }
        }
        None => loop {
            if d.datatype()? == Type::Break {
                d.skip()?;
                break;
            }
            let key = d.str()?;
            on_entry(d, key)?;
        },
    }
    Ok(())
}

fn decode_array<'b, T>(d: &mut Decoder<'b>) -> Result<Vec<T>, minicbor::decode::Error>
where
    T: minicbor::Decode<'b, ()>,
{
    let len = d.array()?;
    let mut out = Vec::new();
    match len {
        Some(n) => {
            for _ in 0..n {
                out.push(d.decode()?);
            }
        }
        None => loop {
            if d.datatype()? == Type::Break {
                d.skip()?;
                break;
            }
            out.push(d.decode()?);
        },
    }
    Ok(out)
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

/// Encode a V4 token to its NUT-00 wire form: `cashuB` + unpadded
/// base64url of the CBOR body. Inverse of [`decode_token`].
pub fn encode_token_wire(token: &TokenV4) -> Result<String, minicbor::encode::Error<Infallible>> {
    let bytes = encode_token(token)?;
    Ok([String::from("cashuB"), encode_base64url(&bytes)].concat())
}

/// Base64url (RFC 4648 §5, no padding) — byte-identical alphabet and
/// chunking to walletport's encoder; one shared implementation.
fn encode_base64url(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 63] as char);
        }
    }
    out
}
