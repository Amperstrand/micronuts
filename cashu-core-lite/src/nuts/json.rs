//! Minimal strict JSON scanner (crate-private).
//!
//! `cashu-core-lite` deliberately carries no serde_json dependency (no_std,
//! minicbor-only), but NUT-10 spending conditions embed a JSON document
//! inside the `Proof.secret` string, and the NUT-11 witness is a stringified
//! JSON object. This module provides just enough strict JSON *syntax*
//! support (RFC 8259 grammar gate) for those two shapes:
//!
//! - What a value MEANS (which keys, which types per key) is decided by the
//!   callers in [`super::nut10`] and [`super::nut11`].
//! - Numbers are validated for grammar but kept uninterpreted — every field
//!   the NUT-10/11 model consumes is a string, and a number where a string
//!   is expected must fail the parse (NUT-10 tag rules).
//!
//! The full JSON codec (including this scanner) stays inside core-lite so
//! the firmware can eventually display/verify P2PK locks offline; the mint
//! consumes the parsed model without ever seeing raw JSON here.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// Grammar-validated but uninterpreted number.
    Number,
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Object member lookup (first occurrence wins, as in serde's default).
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        self.as_object()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

/// Parse a complete JSON document. Returns `None` on any syntax error,
/// including trailing garbage after the top-level value.
pub(crate) fn parse(input: &str) -> Option<Json> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    parser.skip_ws();
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return None;
    }
    Some(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        if self.bump()? == byte {
            Some(())
        } else {
            None
        }
    }

    fn parse_literal(&mut self, literal: &str, value: Json) -> Option<Json> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Some(value)
        } else {
            None
        }
    }

    fn parse_value(&mut self) -> Option<Json> {
        match self.peek()? {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(Json::String),
            b't' => self.parse_literal("true", Json::Bool(true)),
            b'f' => self.parse_literal("false", Json::Bool(false)),
            b'n' => self.parse_literal("null", Json::Null),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_object(&mut self) -> Option<Json> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(Json::Object(members));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.parse_value()?;
            members.push((key, value));
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b'}' => return Some(Json::Object(members)),
                _ => return None,
            }
        }
    }

    fn parse_array(&mut self) -> Option<Json> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b']' => return Some(Json::Array(items)),
                _ => return None,
            }
        }
    }

    /// Grammar gate for RFC 8259 numbers: `-? int (frac)? (exp)?`.
    fn parse_number(&mut self) -> Option<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.bump()? {
            b'0' => {}
            b'1'..=b'9' => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => {
                self.pos = start;
                return None;
            }
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let mut digits = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                digits += 1;
            }
            if digits == 0 {
                self.pos = start;
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let mut digits = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                digits += 1;
            }
            if digits == 0 {
                self.pos = start;
                return None;
            }
        }
        Some(Json::Number)
    }

    /// Parse a JSON string including escape sequences. Raw (possibly
    /// multi-byte UTF-8) segments are copied verbatim — the input is a
    /// `&str`, so they are valid UTF-8 by construction.
    fn parse_string(&mut self) -> Option<String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.bump()? {
                b'"' => {
                    return String::from_utf8(out).ok();
                }
                b'\\' => {
                    let mut escape_buf = [0u8; 4];
                    match self.bump()? {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let code = self.parse_hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&code) {
                                // High surrogate must pair with \uDC00-\uDFFF.
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let low = self.parse_hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return None;
                                }
                                char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00))?
                            } else if (0xDC00..0xE000).contains(&code) {
                                // Lone low surrogate.
                                return None;
                            } else {
                                char::from_u32(code)?
                            };
                            out.extend_from_slice(ch.encode_utf8(&mut escape_buf).as_bytes());
                        }
                        _ => return None,
                    }
                }
                0x00..=0x1F => return None, // raw control characters
                byte => out.push(byte),
            }
        }
    }

    fn parse_hex4(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = match self.bump()? {
                b'0'..=b'9' => (self.bytes[self.pos - 1] - b'0') as u32,
                b'a'..=b'f' => (self.bytes[self.pos - 1] - b'a' + 10) as u32,
                b'A'..=b'F' => (self.bytes[self.pos - 1] - b'A' + 10) as u32,
                _ => return None,
            };
            value = value.checked_mul(16)?.checked_add(digit)?;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_documents() {
        let doc = parse(
            r#"{"nonce":"ab\"c","data":"0249","tags":[["sigflag","SIG_ALL"],["pubkeys","02a","02b"]]}"#,
        )
        .unwrap();
        assert_eq!(doc.get("nonce").unwrap().as_str(), Some("ab\"c"));
        let tags = doc.get("tags").unwrap().as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[1].as_array().unwrap()[2].as_str(), Some("02b"));
    }

    #[test]
    fn numbers_are_grammar_checked_only() {
        assert!(parse("-12.5e+3").is_some());
        assert!(parse("0").is_some());
        assert!(parse("01").is_none(), "leading zero");
        assert!(parse("1.").is_none(), "frac without digits");
        assert!(parse("-").is_none());
        assert!(parse("1e").is_none());
    }

    #[test]
    fn rejects_syntax_errors() {
        assert!(parse("").is_none());
        assert!(parse("[1,2,]").is_none(), "trailing comma");
        assert!(parse("{\"a\":1,}").is_none());
        assert!(parse("[1] junk").is_none(), "trailing garbage");
        assert!(parse("[1").is_none(), "unterminated");
        assert!(parse("\"\\q\"").is_none(), "bad escape");
        assert!(parse("\"\\uD800\"").is_none(), "lone high surrogate");
    }

    #[test]
    fn accepts_literals_and_empty_containers() {
        assert_eq!(parse("true"), Some(Json::Bool(true)));
        assert_eq!(parse("null"), Some(Json::Null));
        assert_eq!(parse("[]"), Some(Json::Array(Vec::new())));
        assert_eq!(parse("{}"), Some(Json::Object(Vec::new())));
        // Surrogate pair decodes to the composed scalar.
        assert_eq!(
            parse("\"a\\uD83D\\uDE00b\"").unwrap().as_str(),
            Some("a\u{1F600}b")
        );
        // Unknown object keys are preserved for the caller to ignore.
        let doc = parse(r#"{"a":"x","b":null}"#).unwrap();
        assert_eq!(doc.get("b"), Some(&Json::Null));
        assert_eq!(doc.get("missing"), None);
    }
}
