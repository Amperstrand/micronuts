extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use cashu_core_lite::TokenV4;
use core::fmt;

use gm65_scanner::{parse_ur_fragment, ParsedUrFragment, PayloadType};

#[derive(Debug, Clone)]
pub enum QrPayload {
    CashuV4 { encoded: Vec<u8> },
    CashuV3 { json: Vec<u8> },
    UrFragment { parsed: ParsedUrFragment },
    PlainText(Vec<u8>),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum DecodedPayload {
    CashuToken(TokenV4),
    CashuV4Raw { encoded: Vec<u8>, error: String },
    UrFragment { index: u32, total: u32 },
    UrComplete(TokenV4),
    PlainText(Vec<u8>),
    Binary(Vec<u8>),
}

impl QrPayload {
    pub fn is_cashu(&self) -> bool {
        matches!(self, QrPayload::CashuV4 { .. } | QrPayload::CashuV3 { .. })
    }

    pub fn is_ur(&self) -> bool {
        matches!(self, QrPayload::UrFragment { .. })
    }

    pub fn raw_data(&self) -> &[u8] {
        match self {
            QrPayload::CashuV4 { encoded } => encoded,
            QrPayload::CashuV3 { json } => json,
            QrPayload::UrFragment { parsed } => &parsed.data,
            QrPayload::PlainText(data) => data,
            QrPayload::Binary(data) => data,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            QrPayload::CashuV4 { .. } => "Cashu V4 Token",
            QrPayload::CashuV3 { .. } => "Cashu V3 Token",
            QrPayload::UrFragment { .. } => "UR Fragment",
            QrPayload::PlainText(_) => "Plain Text",
            QrPayload::Binary(_) => "Binary Data",
        }
    }

    pub fn decode(&self) -> DecodedPayload {
        match self {
            QrPayload::CashuV4 { encoded } => match cashu_core_lite::decode_token(encoded) {
                Ok(token) => DecodedPayload::CashuToken(token),
                Err(e) => DecodedPayload::CashuV4Raw {
                    encoded: encoded.clone(),
                    error: alloc::format!("{:?}", e),
                },
            },
            QrPayload::CashuV3 { json } => DecodedPayload::PlainText(json.clone()),
            QrPayload::UrFragment { parsed } => DecodedPayload::UrFragment {
                index: parsed.index,
                total: parsed.total,
            },
            QrPayload::PlainText(data) => DecodedPayload::PlainText(data.clone()),
            QrPayload::Binary(data) => DecodedPayload::Binary(data.clone()),
        }
    }
}

impl fmt::Display for QrPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QrPayload::CashuV4 { encoded } => {
                write!(f, "CashuV4({} bytes)", encoded.len())
            }
            QrPayload::CashuV3 { json } => {
                write!(f, "CashuV3({} bytes)", json.len())
            }
            QrPayload::UrFragment { parsed } => {
                write!(f, "UR Fragment {}/{}", parsed.index, parsed.total)
            }
            QrPayload::PlainText(data) => {
                write!(f, "PlainText({} bytes)", data.len())
            }
            QrPayload::Binary(data) => {
                write!(f, "Binary({} bytes)", data.len())
            }
        }
    }
}

impl fmt::Display for DecodedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodedPayload::CashuToken(token) => {
                write!(
                    f,
                    "CashuToken(mint={}, amount={})",
                    token.mint,
                    token.total_amount()
                )
            }
            DecodedPayload::CashuV4Raw { encoded, error } => {
                write!(f, "CashuV4Raw({} bytes, error={})", encoded.len(), error)
            }
            DecodedPayload::UrFragment { index, total } => {
                write!(f, "UrFragment({}/{})", index, total)
            }
            DecodedPayload::UrComplete(token) => {
                write!(
                    f,
                    "UrComplete(mint={}, amount={})",
                    token.mint,
                    token.total_amount()
                )
            }
            DecodedPayload::PlainText(data) => {
                write!(f, "PlainText({} bytes)", data.len())
            }
            DecodedPayload::Binary(data) => {
                write!(f, "Binary({} bytes)", data.len())
            }
        }
    }
}

pub fn decode_qr(data: &[u8]) -> QrPayload {
    if let Some(fragment) = parse_ur_fragment(data) {
        return QrPayload::UrFragment { parsed: fragment };
    }

    let payload_type = gm65_scanner::classify_payload(data);
    match payload_type {
        PayloadType::CashuV4 => QrPayload::CashuV4 {
            encoded: data.to_vec(),
        },
        PayloadType::CashuV3 => QrPayload::CashuV3 {
            json: data.to_vec(),
        },
        PayloadType::Url | PayloadType::PlainText => QrPayload::PlainText(data.to_vec()),
        PayloadType::Binary => QrPayload::Binary(data.to_vec()),
        PayloadType::UrFragment => QrPayload::PlainText(data.to_vec()),
    }
}

pub fn is_qr_payload(data: &[u8]) -> bool {
    data.starts_with(b"cashuB")
        || data.starts_with(b"cashuA")
        || data.starts_with(b"ur:")
        || core::str::from_utf8(data).is_ok()
}

use gm65_scanner::UrDecoder as Gm65UrDecoder;

pub struct UrDecoder(Gm65UrDecoder);

impl UrDecoder {
    pub fn new() -> Self {
        Self(Gm65UrDecoder::new())
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub fn feed(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.0.feed(data)
    }

    pub fn progress(&self) -> (u32, u32) {
        self.0.progress()
    }

    pub fn is_active(&self) -> bool {
        self.0.is_active()
    }

    pub fn is_complete(&self) -> bool {
        self.0.is_complete()
    }
}

impl Default for UrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

pub use gm65_scanner::ScannerModel;
pub use gm65_scanner::ScannerState;
