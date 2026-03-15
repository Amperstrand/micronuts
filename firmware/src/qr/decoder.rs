//! QR Payload Decoder
//!
//! Decodes scanned QR data into typed payloads.
//! Supports Cashu tokens (V3/V4), UR animated QR, and plain text.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use cashu_core_lite::TokenV4;
use core::fmt;

/// Cashu V4 token prefix
const CASHU_V4_PREFIX: &[u8] = b"cashuB";

/// Cashu V3 token prefix (JSON)
const CASHU_V3_PREFIX: &[u8] = b"cashuA";

/// UR protocol prefix
const UR_PREFIX: &[u8] = b"ur:";

/// Decoded QR payload types
#[derive(Debug, Clone)]
pub enum QrPayload {
    /// Cashu V4 token (cashuB...)
    CashuV4 {
        encoded: Vec<u8>,
    },
    /// Cashu V3 token (JSON format)
    CashuV3 {
        json: Vec<u8>,
    },
    /// UR animated QR fragment
    UrFragment {
        index: u32,
        total: u32,
        hash: String,
        data: Vec<u8>,
    },
    PlainText(Vec<u8>),
    Binary(Vec<u8>),
}

/// Fully decoded payload with parsed token data
#[derive(Debug, Clone)]
pub enum DecodedPayload {
    /// Successfully decoded Cashu V4 token
    CashuToken(TokenV4),
    /// Cashu V4 token data that failed to decode
    CashuV4Raw { encoded: Vec<u8>, error: String },
    /// UR fragment (not yet complete)
    UrFragment { index: u32, total: u32 },
    /// UR complete and decoded
    UrComplete(TokenV4),
    /// Plain text data
    PlainText(Vec<u8>),
    /// Binary data
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
            QrPayload::UrFragment { data, .. } => data,
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

    /// Try to fully decode this payload into a DecodedPayload
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
            QrPayload::UrFragment { index, total, .. } => DecodedPayload::UrFragment {
                index: *index,
                total: *total,
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
            QrPayload::UrFragment { index, total, .. } => {
                write!(f, "UR Fragment {}/{}", index, total)
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

/// Decode QR data into a typed payload
pub fn decode_qr(data: &[u8]) -> QrPayload {
    // Check for Cashu V4 prefix
    if data.starts_with(CASHU_V4_PREFIX) {
        return QrPayload::CashuV4 {
            encoded: data.to_vec(),
        };
    }

    // Check for Cashu V3 prefix
    if data.starts_with(CASHU_V3_PREFIX) {
        return QrPayload::CashuV3 {
            json: data.to_vec(),
        };
    }

    // Check for UR protocol
    if data.starts_with(UR_PREFIX) {
        return parse_ur_fragment(data);
    }

    // Check if it's valid UTF-8 text
    if let Ok(text) = core::str::from_utf8(data) {
        // Check for common QR text patterns
        if text.starts_with("http://") || text.starts_with("https://") {
            return QrPayload::PlainText(data.to_vec());
        }
        // Plain text
        return QrPayload::PlainText(data.to_vec());
    }

    // Binary data
    QrPayload::Binary(data.to_vec())
}

/// Parse UR (Uniform Resources) fragment
///
/// Format: `ur:cashu/<index>-<total>/<hash>/<data>`
fn parse_ur_fragment(data: &[u8]) -> QrPayload {
    // Convert to string for parsing
    let s = match core::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return QrPayload::Binary(data.to_vec()),
    };

    // Parse UR format
    // Example: ur:cashu/1-5/abc123/encodeddata
    let parts: Vec<&str> = s.split('/').collect();

    if parts.len() < 4 {
        return QrPayload::PlainText(data.to_vec());
    }

    // parts[0] = "ur:"
    // parts[1] = "cashu"
    // parts[2] = "1-5" (index-total)
    // parts[3] = hash
    // parts[4+] = data

    let type_part = parts[1].to_lowercase();
    if type_part != "cashu" {
        return QrPayload::PlainText(data.to_vec());
    }

    // Parse index-total
    let index_total: Vec<&str> = parts[2].split('-').collect();
    if index_total.len() != 2 {
        return QrPayload::PlainText(data.to_vec());
    }

    let index = match index_total[0].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return QrPayload::PlainText(data.to_vec()),
    };

    let total = match index_total[1].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return QrPayload::PlainText(data.to_vec()),
    };

    let hash = String::from(parts[3]);

    // Remaining parts are the data
    let data_str = parts[4..].join("/");

    QrPayload::UrFragment {
        index,
        total,
        hash,
        data: data_str.as_bytes().to_vec(),
    }
}

/// UR decoder for accumulating multi-part QR codes
#[derive(Debug)]
pub struct UrDecoder {
    /// Total expected fragments (None until first fragment received)
    total: Option<u32>,
    /// Message hash for matching
    hash: Option<String>,
    /// Received fragments (index -> data)
    fragments: Vec<Option<Vec<u8>>>,
    /// Number of fragments received
    received: u32,
}

impl UrDecoder {
    /// Create a new UR decoder
    pub fn new() -> Self {
        Self {
            total: None,
            hash: None,
            fragments: Vec::new(),
            received: 0,
        }
    }

    /// Reset the decoder state
    pub fn reset(&mut self) {
        self.total = None;
        self.hash = None;
        self.fragments.clear();
        self.received = 0;
    }

    /// Feed a fragment to the decoder
    ///
    /// Returns `Some(complete_data)` when all fragments have been received,
    /// `None` if more fragments are needed.
    pub fn feed(&mut self, payload: &QrPayload) -> Option<Vec<u8>> {
        let (index, total, hash, data) = match payload {
            QrPayload::UrFragment {
                index,
                total,
                hash,
                data,
            } => (*index, *total, hash.clone(), data.clone()),
            _ => return None,
        };

        // First fragment - initialize
        if self.total.is_none() {
            self.total = Some(total);
            self.hash = Some(hash);
            self.fragments = vec![None; total as usize];
        }

        // Validate hash matches
        if self.hash.as_ref() != Some(&hash) {
            return None;
        }

        // Store fragment (1-indexed to 0-indexed)
        let idx = (index - 1) as usize;
        if idx < self.fragments.len() && self.fragments[idx].is_none() {
            self.fragments[idx] = Some(data);
            self.received += 1;
        }

        // Check if complete
        if self.received == self.total? {
            // Combine all fragments
            let mut result = Vec::new();
            for frag in &self.fragments {
                if let Some(data) = frag {
                    result.extend_from_slice(data);
                } else {
                    return None; // Missing fragment
                }
            }
            return Some(result);
        }

        None
    }

    /// Get the progress (received, total)
    pub fn progress(&self) -> (u32, u32) {
        (self.received, self.total.unwrap_or(0))
    }

    /// Check if decoding is in progress
    pub fn is_active(&self) -> bool {
        self.total.is_some()
    }

    /// Check if all fragments have been received
    pub fn is_complete(&self) -> bool {
        self.total.map(|t| self.received == t).unwrap_or(false)
    }
}

impl Default for UrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if data looks like a QR code payload we can handle
pub fn is_qr_payload(data: &[u8]) -> bool {
    // Check for known prefixes
    data.starts_with(CASHU_V4_PREFIX)
        || data.starts_with(CASHU_V3_PREFIX)
        || data.starts_with(UR_PREFIX)
        || core::str::from_utf8(data).is_ok()
}
