//! Animated-QR scan assembly: turns raw scanner frames into app outcomes.
//!
//! The Scanning screen feeds every scanned frame through
//! [`ScanAssembler::process`]. UR fragments (`ur:<type>/<i>-<N>/<hash>/<data>`,
//! gm65-scanner format) accumulate until the sequence completes; the
//! reassembled payload must then decode as a Cashu V4 token (NUT-00). A
//! single-frame `cashu`-prefixed payload imports directly. Everything else
//! keeps the legacy ScanResult behavior.

extern crate alloc;

use alloc::string::String;

use cashu_core_lite::{decode_token, TokenV4};

use crate::qr::{decode_qr, QrPayload, UrDecoder};

/// What the Scanning screen should do after one scanned frame.
#[derive(Debug, Clone)]
pub enum ScanOutcome {
    /// A UR fragment was accepted; `received`/`total` frames are in.
    KeepScanning { received: u32, total: u32 },
    /// A complete Cashu token is ready to import (reassembled or
    /// single-frame).
    TokenReady(TokenV4),
    /// Not a token: show the payload on the ScanResult screen as before.
    ShowPayload(QrPayload),
    /// A UR fragment was rejected (e.g. hash mismatch with the sequence).
    InvalidFragment,
}

/// Accumulates UR fragments across scans. One assembler per scanning
/// session; [`ScanAssembler::reset`] on exit/restart.
pub struct ScanAssembler {
    ur: UrDecoder,
    active_hash: Option<String>,
}

impl ScanAssembler {
    pub fn new() -> Self {
        Self {
            ur: UrDecoder::new(),
            active_hash: None,
        }
    }

    pub fn reset(&mut self) {
        self.ur.reset();
        self.active_hash = None;
    }

    /// Fragment progress for the UI when no new frame has arrived.
    pub fn progress(&self) -> (u32, u32) {
        self.ur.progress()
    }

    pub fn process(&mut self, data: &[u8]) -> ScanOutcome {
        let payload = decode_qr(data);
        match payload {
            QrPayload::UrFragment { ref parsed } => {
                // Sequence binding: the first fragment's hash owns the
                // sequence; frames from another sequence are rejected.
                match &self.active_hash {
                    None => self.active_hash = Some(parsed.hash.clone()),
                    Some(h) if h == &parsed.hash => {}
                    Some(_) => return ScanOutcome::InvalidFragment,
                }
                if let Some(complete) = self.ur.feed(data) {
                    match decode_token(&complete) {
                        Ok(token) => ScanOutcome::TokenReady(token),
                        Err(_) => {
                            // Reassembled bytes are not a token — show them
                            // as text instead of wedging the scanner.
                            self.reset();
                            ScanOutcome::ShowPayload(QrPayload::PlainText(complete))
                        }
                    }
                } else if self.ur.is_active() {
                    let (received, total) = self.ur.progress();
                    ScanOutcome::KeepScanning { received, total }
                } else {
                    // The decoder refused to start a sequence (e.g. index
                    // out of range).
                    ScanOutcome::InvalidFragment
                }
            }
            QrPayload::CashuV4 { ref encoded } => match decode_token(encoded) {
                Ok(token) => ScanOutcome::TokenReady(token),
                Err(_) => ScanOutcome::ShowPayload(payload),
            },
            other => ScanOutcome::ShowPayload(other),
        }
    }
}

impl Default for ScanAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The gm65-scanner UR wire format, mirrored by the host encoder in
    // tools/hil/urtoken.py: `ur:bytes/<i>-<N>/<hash>/<raw-ascii chunk>`.
    fn fragment(i: u32, total: u32, hash: &str, chunk: &str) -> Vec<u8> {
        format!("ur:bytes/{i}-{total}/{hash}/{chunk}").into_bytes()
    }

    // cashuB + base64url(minicbor(TokenV4)) — the NUT-00 wire form produced
    // by cashu_core_lite::encode_token_wire; a minimal single-proof token.
    fn wire_token() -> String {
        use cashu_core_lite::encode_token_wire;
        let token = crate::test_util::minimal_token();
        encode_token_wire(&token).unwrap()
    }

    #[test]
    fn single_frame_token_imports_directly() {
        let mut asm = ScanAssembler::new();
        let wire = wire_token().into_bytes();
        match asm.process(&wire) {
            ScanOutcome::TokenReady(t) => {
                assert_eq!(
                    t.total_amount(),
                    crate::test_util::minimal_token().total_amount()
                )
            }
            other => panic!("expected TokenReady, got {other:?}"),
        }
    }

    #[test]
    fn ur_sequence_in_order_completes() {
        let wire = wire_token();
        let (a, b) = wire.split_at(wire.len() / 2);
        let mut asm = ScanAssembler::new();
        match asm.process(&fragment(1, 2, "abcd1234", a)) {
            ScanOutcome::KeepScanning {
                received: 1,
                total: 2,
            } => {}
            other => panic!("expected KeepScanning 1/2, got {other:?}"),
        }
        match asm.process(&fragment(2, 2, "abcd1234", b)) {
            ScanOutcome::TokenReady(t) => {
                assert_eq!(
                    t.total_amount(),
                    crate::test_util::minimal_token().total_amount()
                )
            }
            other => panic!("expected TokenReady, got {other:?}"),
        }
    }

    #[test]
    fn ur_sequence_out_of_order_completes() {
        let wire = wire_token();
        let (a, b) = wire.split_at(wire.len() / 2);
        let mut asm = ScanAssembler::new();
        assert!(matches!(
            asm.process(&fragment(2, 2, "abcd1234", b)),
            ScanOutcome::KeepScanning { .. }
        ));
        assert!(matches!(
            asm.process(&fragment(1, 2, "abcd1234", a)),
            ScanOutcome::TokenReady(_)
        ));
    }

    #[test]
    fn duplicate_fragments_are_ignored_not_counted_twice() {
        let mut asm = ScanAssembler::new();
        let f = fragment(1, 3, "h", "aaaa");
        for _ in 0..2 {
            match asm.process(&f) {
                ScanOutcome::KeepScanning {
                    received: 1,
                    total: 3,
                } => {}
                other => panic!("expected KeepScanning 1/3, got {other:?}"),
            }
        }
    }

    #[test]
    fn hash_mismatch_is_invalid_and_does_not_corrupt() {
        let mut asm = ScanAssembler::new();
        asm.process(&fragment(1, 2, "aaaa", "xx"));
        assert!(matches!(
            asm.process(&fragment(2, 2, "bbbb", "yy")),
            ScanOutcome::InvalidFragment
        ));
        // The good second fragment still completes the original sequence.
        assert!(matches!(
            asm.process(&fragment(2, 2, "aaaa", "yy")),
            ScanOutcome::ShowPayload(_) // "xxyy" is not a token
        ));
    }

    #[test]
    fn reassembled_non_token_shows_as_text() {
        let mut asm = ScanAssembler::new();
        asm.process(&fragment(1, 2, "h", "hello "));
        match asm.process(&fragment(2, 2, "h", "world")) {
            ScanOutcome::ShowPayload(QrPayload::PlainText(data)) => {
                assert_eq!(data, b"hello world")
            }
            other => panic!("expected PlainText, got {other:?}"),
        }
    }

    #[test]
    fn plain_text_keeps_legacy_behavior() {
        let mut asm = ScanAssembler::new();
        match asm.process(b"just some text") {
            ScanOutcome::ShowPayload(QrPayload::PlainText(d)) => assert_eq!(d, b"just some text"),
            other => panic!("expected PlainText, got {other:?}"),
        }
    }

    #[test]
    fn bad_single_frame_token_shows_payload_not_import() {
        let mut asm = ScanAssembler::new();
        let junk = b"cashuB!!!!not-base64****";
        match asm.process(junk) {
            // decode_token falls back to raw CBOR for unprefixed data;
            // for garbage it must NOT report a ready token.
            ScanOutcome::TokenReady(_) => panic!("garbage must not import"),
            ScanOutcome::ShowPayload(_) => {}
            other => panic!("expected ShowPayload, got {other:?}"),
        }
    }

    #[test]
    fn reset_clears_partial_sequence() {
        let mut asm = ScanAssembler::new();
        asm.process(&fragment(1, 2, "h", "aa"));
        asm.reset();
        assert_eq!(asm.progress(), (0, 0));
        assert!(matches!(
            asm.process(&fragment(2, 2, "h", "bb")),
            ScanOutcome::KeepScanning {
                received: 1,
                total: 2
            }
        ));
    }

    #[test]
    fn real_token_fifteen_fragments_complete_and_decode() {
        let frags: [&str; 15] = [
            r"ur:bytes/1-15/31697202/cashuBpGFtcGRlbW86Ly9taWNyb251dHNhdWNzYXRhZHVTd2FwcGVkIHZpYSBNaWNyb2",
            r"ur:bytes/2-15/31697202/51dHNhdIGiYWlBAGFwg6RhYRBhc3hANTI1MjhkOTVmZDc0MThiNjJmNmJiNzI4OTkzMz",
            r"ur:bytes/3-15/31697202/liYmY4MTlkMWY3Y2UwZjE3MzljOTE1ZjcyMDA1MGFjYjVhZGFjWCECVLTS4rwCHKKxkL",
            r"ur:bytes/4-15/31697202/tix7DHEkRmovr2DSEq1Can9TzXwJphZKNhZVgg-EH-lQFeTp_YIk56MoV8MN1oO-bDNQ",
            r"ur:bytes/5-15/31697202/CL8Mva8Z8nz7Jhc1ggZ7ZtSNAtgJkLSBjH9xb-2T77y4NBGhS9LFMrBI9b081hclggB0",
            r"ur:bytes/6-15/31697202/t8gpDoVjBIzJ_KsVt8ojDu-ZiY0-XVgrMb8BPIttOkYWEEYXN4QGM1NmY3YmU3MDk0OT",
            r"ur:bytes/7-15/31697202/Q3MTJkMmJmMGU1NDMxZGYwYjM5NWRkY2FjZmVkOGQ2NjM5ZmExNTViN2FmYzEwMGJiY2",
            r"ur:bytes/8-15/31697202/FhY1ghAvsolyC66qbnaQlNNrZnDiF-kjxys8d4RPua9MHYRggGYWSjYWVYIKGDBBxOVv",
            r"ur:bytes/9-15/31697202/dyXRgkl_qRJOP6Z5XSls8Fu-9PgUfTL6GNYXNYIDzR5S3TIFeIc-unfQPW_uFGaAhEl1",
            r"ur:bytes/10-15/31697202/69VvPUMd7F862KYXJYIKAwOD4XvRek1n1VW2D4PHZuw2ky76LTYHoFp8rdWYtwpGFhAW",
            r"ur:bytes/11-15/31697202/FzeEA4NjA4MmY4YTE4NzZmNzA2ZGUxODI4ZWJkNTBkYTE2YjlmMjNjOGU4ZTMyYTUzOD",
            r"ur:bytes/12-15/31697202/Q3MDdmYTMzMGE4ZGVhNjE5YWNYIQNsbP10Tod0VeRI1jP3eVAJPBgUBAgEERrK4ap6Da",
            r"ur:bytes/13-15/31697202/b_jWFko2FlWCAfiTzr_W8SnHvqHDrb33IreLIJ6ZK7-QecF6OqH7VqPGFzWCBRmluzf7",
            r"ur:bytes/14-15/31697202/EdKsdYJJqMV8hHtb2B9hJEucOCoW9IT5judmFyWCD-SWhyxsl9aD8SZhPjohpMOZDba6",
            r"ur:bytes/15-15/31697202/HjUWnTgdCpDwSIeA",
        ];
        let mut asm = ScanAssembler::new();
        let mut last = None;
        for f in frags {
            last = Some(asm.process(f.as_bytes()));
        }
        match last {
            Some(ScanOutcome::TokenReady(t)) => {
                assert_eq!(t.total_amount(), 21);
                assert!(t.proof_count() >= 1);
            }
            other => panic!("expected TokenReady(21 sat), got {other:?}"),
        }
    }
}
