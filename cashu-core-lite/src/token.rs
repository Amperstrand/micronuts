use alloc::string::String;
use alloc::vec::Vec;
use minicbor::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct Proof {
    #[n(0)]
    pub amount: u64,

    #[n(1)]
    pub keyset_id: String,

    #[n(2)]
    pub secret: String,

    #[n(3)]
    pub c: Vec<u8>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct TokenV4Token {
    #[n(0)]
    pub keyset_id: String,

    #[n(1)]
    pub proofs: Vec<Proof>,
}

#[derive(Debug, Clone, Encode, Decode)]
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
    if let Some(stripped) = data.strip_prefix(b"cashuB") {
        minicbor::decode(stripped)
    } else if let Some(stripped) = data.strip_prefix(b"crawB") {
        minicbor::decode(stripped)
    } else {
        minicbor::decode(data)
    }
}

pub fn encode_token(
    token: &TokenV4,
) -> Result<Vec<u8>, minicbor::encode::Error<core::convert::Infallible>> {
    let mut buf = Vec::new();
    minicbor::encode(token, &mut buf)?;
    Ok(buf)
}
