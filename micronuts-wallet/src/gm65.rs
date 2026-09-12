//! GM65 USB-serial scanner transport (host). Consumes the gm65-scanner
//! crate's core — line assembly (`ScanBuffer`), payload classification
//! (`decode_payload`), and animated-UR accumulation (`UrDecoder`) — and
//! adds only the serial pipe. Passive read: the module is expected to be
//! in continuous-scan mode (as the QR rig configures it); no protocol
//! commands are written.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gm65_scanner::decoder::{decode_payload, PayloadType, UrDecoder};
use gm65_scanner::ScanBuffer;

/// What a completed scan line (or UR sequence) resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanResult {
    Token(String),
    /// Animated-UR sequence completed; raw assembled bytes.
    UrBytes(Vec<u8>),
    /// A well-formed line that is not something we act on (URL, text,
    /// cashuA, or a UR progress fragment like "ur 1/3").
    Ignored(String),
}

/// Feed serial bytes; get classified scan lines out. One `feed` may
/// complete zero, one, or many lines. An overlong line without EOL puts
/// the assembler into drop-mode until the next EOL (standard serial-line
/// discipline: the garbage line's tail is indistinguishable from junk).
pub struct ScanAssembler {
    buffer: ScanBuffer,
    ur: UrDecoder,
    dropping: bool,
}

impl ScanAssembler {
    pub fn new() -> Self {
        Self {
            buffer: ScanBuffer::new(),
            ur: UrDecoder::new(),
            dropping: false,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<ScanResult> {
        let mut results = Vec::new();
        for &byte in bytes {
            if self.dropping {
                if byte == b'\r' || byte == b'\n' {
                    self.dropping = false;
                    self.buffer.clear();
                }
                continue;
            }
            if !self.buffer.push(byte) {
                self.dropping = true;
                self.buffer.clear();
                continue;
            }
            if self.buffer.has_eol() {
                let line = self.buffer.data_without_eol().to_vec();
                self.buffer.clear();
                if !line.is_empty() {
                    results.push(self.classify(line));
                }
            }
        }
        results
    }

    fn classify(&mut self, line: Vec<u8>) -> ScanResult {
        let payload = decode_payload(&line);
        match payload.payload_type {
            PayloadType::CashuV4 => match payload.as_str() {
                Some(token) => ScanResult::Token(token.to_string()),
                None => ScanResult::Ignored(String::from("cashuB (non-utf8)")),
            },
            PayloadType::UrFragment => match self.ur.feed(&line) {
                Some(assembled) => ScanResult::UrBytes(assembled),
                None => {
                    let (received, total) = self.ur.progress();
                    ScanResult::Ignored(format!("ur {received}/{total}"))
                }
            },
            other => ScanResult::Ignored(format!("{other}")),
        }
    }
}

impl Default for ScanAssembler {
    fn default() -> Self {
        Self::new()
    }
}

/// UR output is raw assembled bytes; surface it as a token only when it
/// decodes to UTF-8 text shaped like one.
pub fn ur_bytes_to_result(bytes: Vec<u8>) -> ScanResult {
    match String::from_utf8(bytes) {
        Ok(text) if text.starts_with("cashuB") => ScanResult::Token(text),
        Ok(_) => ScanResult::Ignored(String::from("ur (not a cashuB token)")),
        Err(_) => ScanResult::Ignored(String::from("ur (binary payload)")),
    }
}

/// Handle that stops the reader thread (checked once per read timeout).
#[derive(Clone)]
pub struct Gm65Stop(Arc<AtomicBool>);

impl Gm65Stop {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Resolve the serial port: `MICRONUTS_GM65_PORT` overrides autodetect
/// (first of /dev/ttyUSB*, then /dev/ttyACM*).
pub fn detect_port() -> Result<String, String> {
    if let Ok(port) = std::env::var("MICRONUTS_GM65_PORT") {
        return Ok(port);
    }
    let mut candidates: Vec<String> = Vec::new();
    for prefix in ["/dev/ttyUSB", "/dev/ttyACM"] {
        let mut index = 0;
        while let Ok(entry) = std::fs::read_link(format!("{prefix}{index}")) {
            let _ = entry;
            candidates.push(format!("{prefix}{index}"));
            index += 1;
        }
        if !candidates.is_empty() {
            break;
        }
    }
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| String::from("no serial port found (set MICRONUTS_GM65_PORT)"))
}

/// Spawn the GM65 reader thread. `on_scan` receives token strings (both
/// direct cashuB lines and UR-assembled tokens); `on_status` receives
/// human status/errors (also called from the thread — marshal to the UI
/// yourself, e.g. via `slint::invoke_from_event_loop`).
pub fn spawn_gm65_reader(
    on_scan: impl Fn(String) + Send + 'static,
    on_status: impl Fn(String) + Send + 'static,
) -> Result<Gm65Stop, String> {
    let port_path = detect_port()?;
    let baud: u32 = std::env::var("MICRONUTS_GM65_BAUD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(115_200);
    let port = serialport::new(&port_path, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|err| format!("open {port_path}: {err}"))?;
    on_status(format!("gm65: listening on {port_path} @ {baud}"));

    let stop = Arc::new(AtomicBool::new(false));
    let handle = Gm65Stop(stop.clone());
    std::thread::spawn(move || {
        let mut port = port;
        let mut assembler = ScanAssembler::new();
        let mut chunk = [0u8; 256];
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match port.read(&mut chunk) {
                Ok(0) => {}
                Ok(n) => {
                    for result in assembler.feed(&chunk[..n]) {
                        let outcome = match result {
                            ScanResult::Token(token) => ScanResult::Token(token),
                            ScanResult::UrBytes(bytes) => ur_bytes_to_result(bytes),
                            ignored @ ScanResult::Ignored(_) => ignored,
                        };
                        if let ScanResult::Token(token) = outcome {
                            on_scan(token);
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
                Err(err) => {
                    on_status(format!("gm65: read error: {err}"));
                    break;
                }
            }
        }
    });
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "cashuBpGFtdWh0dHA6Ly8xMjcuMC4wLjE6MzAzMHVpc2F0";

    #[test]
    fn single_cashu_line() {
        let mut asm = ScanAssembler::new();
        let results = asm.feed(format!("{TOKEN}\r\n").as_bytes());
        assert_eq!(results, vec![ScanResult::Token(String::from(TOKEN))]);
    }

    #[test]
    fn line_split_across_feeds() {
        let mut asm = ScanAssembler::new();
        assert!(asm.feed(b"cashuB").is_empty());
        let results = asm.feed(b"AA\r\n");
        assert_eq!(results, vec![ScanResult::Token(String::from("cashuBAA"))]);
    }

    #[test]
    fn lone_cr_completes_a_line() {
        let mut asm = ScanAssembler::new();
        let results = asm.feed(b"cashuBAA\r");
        assert_eq!(results, vec![ScanResult::Token(String::from("cashuBAA"))]);
    }

    #[test]
    fn two_lines_in_one_feed() {
        let mut asm = ScanAssembler::new();
        let results = asm.feed(b"cashuBone\ncashuBtwo\r\n");
        assert_eq!(
            results,
            vec![
                ScanResult::Token(String::from("cashuBone")),
                ScanResult::Token(String::from("cashuBtwo")),
            ]
        );
    }

    #[test]
    fn v3_and_plain_text_are_ignored() {
        let mut asm = ScanAssembler::new();
        let results = asm.feed(b"cashuAlegacy\r\nhello world\nhttps://x.example\n");
        assert_eq!(results.len(), 3);
        assert!(matches!(&results[0], ScanResult::Ignored(s) if s.contains("Cashu V3")));
        assert!(matches!(&results[1], ScanResult::Ignored(_)));
        assert!(matches!(&results[2], ScanResult::Ignored(s) if s.contains("URL")));
    }

    #[test]
    fn ur_fragments_accumulate_to_bytes() {
        let mut asm = ScanAssembler::new();
        assert_eq!(
            asm.feed(b"ur:bytes/1-2/hash1/part-a\n"),
            vec![ScanResult::Ignored(String::from("ur 1/2"))]
        );
        let results = asm.feed(b"ur:bytes/2-2/hash1/part-b\n");
        assert_eq!(results, vec![ScanResult::UrBytes(b"part-apart-b".to_vec())]);
        assert_eq!(
            ur_bytes_to_result(b"part-apart-b".to_vec()),
            ScanResult::Ignored(String::from("ur (not a cashuB token)"))
        );
        assert_eq!(
            ur_bytes_to_result(b"cashuBz".to_vec()),
            ScanResult::Token(String::from("cashuBz"))
        );
    }

    #[test]
    fn garbage_bytes_never_panic() {
        let mut asm = ScanAssembler::new();
        let results = asm.feed(&[0xFF, 0xFE, 0x00, 0x01, b'\r', b'\n']);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], ScanResult::Ignored(_)));
    }

    #[test]
    fn overflow_line_is_dropped_not_stuck() {
        let mut asm = ScanAssembler::new();
        let flood = vec![b'x'; 4096];
        assert!(asm.feed(&flood).is_empty());
        // The overlong garbage line runs into this feed; its EOL
        // terminates drop-mode, so only the following line surfaces.
        let results = asm.feed(b"garbage-tail\r\n");
        assert!(results.is_empty());
        let results = asm.feed(format!("{TOKEN}\r\n").as_bytes());
        assert_eq!(results, vec![ScanResult::Token(String::from(TOKEN))]);
    }
}
