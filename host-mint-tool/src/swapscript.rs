//! Same-script-two-runners swap harness (the Trezor device-test pattern
//! in miniature): ONE swap script — import → token info → blinded →
//! signatures → export — executed against either an in-process device
//! (`handle_command` + test hardware, no USB) or the real device over
//! USB CDC frames. `--selftest` runs the in-process leg (CI); `--port`
//! runs the wire leg; neither runs the wire leg against an autodetected
//! CDC port, exiting 77 when no device is present (house gate
//! convention, cf. scripts/test_hw_swap_gate.sh).

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};

use cashu_core_lite::nuts::nut00;
use cashu_core_lite::{blind_message, decode_token, Proof, PublicKey, TokenV4, TokenV4Token};
use micronuts_app::command_handler::handle_command;
use micronuts_app::hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};
use micronuts_app::protocol::{Command, Frame, Response, Status};
use micronuts_app::state::FirmwareState;
use rand::RngCore;

use crate::mint::DemoMint;

/// Deterministic RNG (xorshift64*) so the selftest script is reproducible.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

pub struct NullDisplay;

impl embedded_graphics::prelude::OriginDimensions for NullDisplay {
    fn size(&self) -> embedded_graphics::prelude::Size {
        embedded_graphics::prelude::Size::new(480, 800)
    }
}

impl embedded_graphics::draw_target::DrawTarget for NullDisplay {
    type Color = embedded_graphics::pixelcolor::Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, _pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::prelude::Pixel<Self::Color>>,
    {
        Ok(())
    }
}

/// In-process device hardware: null display, deterministic RNG, no
/// scanner — everything the swap-flow command path touches.
pub struct TestHardware {
    display: NullDisplay,
    rng: XorShift64,
}

impl TestHardware {
    pub fn new() -> Self {
        Self {
            display: NullDisplay,
            rng: XorShift64(0x9E37_79B9_7F4A_7C15),
        }
    }
}

impl Scanner for TestHardware {
    async fn trigger(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
    async fn read_scan(&mut self) -> Option<Vec<u8>> {
        None
    }
    async fn stop(&mut self) {}
    fn is_connected(&self) -> bool {
        false
    }
    async fn set_aim(&mut self, _enabled: bool) -> Result<(), ScanError> {
        Ok(())
    }
    fn debug_dump_settings(&mut self) {}
    async fn deep_sleep_reboot(&mut self) -> bool {
        true
    }
    async fn reinit_scanner(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
    async fn factory_heal(&mut self) -> Result<(), ScanError> {
        Ok(())
    }
}

impl MicronutsHardware for TestHardware {
    type Display = NullDisplay;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.display
    }
    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        let mut offset = 0;
        while offset < dest.len() {
            let chunk = self.rng.next().to_le_bytes();
            let take = (dest.len() - offset).min(chunk.len());
            dest[offset..offset + take].copy_from_slice(&chunk[..take]);
            offset += take;
        }
    }
    async fn transport_recv_frame(&mut self) -> Option<Frame> {
        None
    }
    async fn transport_send(&mut self, _response: &Response) {}
    fn touch_get(&mut self) -> Option<TouchPoint> {
        None
    }
    async fn delay_ms(&mut self, _ms: u32) {}
}

/// Transport-neutral device exchange: one command in, one status +
/// payload out. The two runners are the whole Trezor trick — the script
/// below cannot tell them apart.
trait SwapTransport {
    fn exchange(&mut self, command: Command, payload: &[u8]) -> Result<(Status, Vec<u8>)>;
}

struct InProcessTransport {
    state: FirmwareState,
    hw: TestHardware,
    last_scan: Option<Vec<u8>>,
}

impl InProcessTransport {
    fn new() -> Self {
        Self {
            state: FirmwareState::new(),
            hw: TestHardware::new(),
            last_scan: None,
        }
    }
}

impl SwapTransport for InProcessTransport {
    fn exchange(&mut self, command: Command, payload: &[u8]) -> Result<(Status, Vec<u8>)> {
        let response = embassy_futures::block_on(handle_command(
            command,
            payload,
            &mut self.state,
            &mut self.hw,
            &mut self.last_scan,
        ));
        Ok((response.status, response.payload().to_vec()))
    }
}

struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    fn open(path: &Path, baud: u32) -> Result<Self> {
        let port = serialport::new(path.to_string_lossy(), baud)
            .timeout(std::time::Duration::from_secs(5))
            .open()
            .with_context(|| format!("open {}", path.display()))?;
        Ok(Self { port })
    }
}

impl SwapTransport for SerialTransport {
    fn exchange(&mut self, command: Command, payload: &[u8]) -> Result<(Status, Vec<u8>)> {
        let frame = Frame::with_payload(command, payload)
            .context("command payload exceeds the frame limit")?;
        let mut wire = vec![0u8; frame.encoded_size()];
        let written = frame.encode(&mut wire);
        wire.truncate(written);
        self.port
            .write_all(&wire)
            .context("write command frame to device")?;
        self.port.flush().ok();

        // Response frame = [status:1][len_be:2][payload:len] — the same
        // header shape the robustness leg of test_hw_swap_gate.sh reads;
        // the firmware FrameDecoder is command-keyed and cannot decode
        // response frames (status bytes collide with command codes).
        let mut header = [0u8; 3];
        self.port
            .read_exact(&mut header)
            .context("read response header from device")?;
        let length = u16::from_be_bytes([header[1], header[2]]) as usize;
        let mut body = vec![0u8; length];
        self.port
            .read_exact(&mut body)
            .context("read response body from device")?;

        match header[0] {
            0x00 => Ok((Status::Ok, body)),
            byte => bail!("device returned status 0x{byte:02X}"),
        }
    }
}

pub struct SwapReport {
    pub amount: u64,
    pub proof_count: usize,
    pub export_token: Vec<u8>,
}

/// The ONE swap script. Both runners execute exactly this sequence with
/// exactly these assertions.
fn run_swap_script(
    transport: &mut dyn SwapTransport,
    mint: &DemoMint,
    token_bytes: &[u8],
    amount: u64,
) -> Result<SwapReport> {
    let (status, _) = transport.exchange(Command::ImportToken, token_bytes)?;
    if status != Status::Ok {
        bail!("import failed: {status:?}");
    }

    let (status, info) = transport.exchange(Command::GetTokenInfo, &[])?;
    if status != Status::Ok {
        bail!("token info failed: {status:?}");
    }
    let parsed = parse_token_info(&info)?;
    if parsed.0 != amount {
        bail!("token info amount {} != expected {amount}", parsed.0);
    }
    let expected_count = nut00::decompose_amount(amount).len();
    if parsed.1 as usize != expected_count {
        bail!(
            "token info proof count {} != expected {expected_count}",
            parsed.1
        );
    }

    let (status, blinded) = transport.exchange(Command::GetBlinded, &[])?;
    if status != Status::Ok {
        bail!("get blinded failed: {status:?}");
    }
    if blinded.len() != expected_count * 33 {
        bail!(
            "blinded payload {} bytes != expected {} (33 per proof)",
            blinded.len(),
            expected_count * 33
        );
    }

    let signatures = sign_blinded_outputs(mint, &blinded)?;
    let (status, _) = transport.exchange(Command::SendSignatures, &signatures)?;
    if status != Status::Ok {
        bail!("signatures rejected (DLEQ gate): {status:?}");
    }

    let (status, export) = transport.exchange(Command::GetProofs, &[])?;
    if status != Status::Ok {
        bail!("export failed: {status:?}");
    }

    let decoded = decode_token(&export).context("export token does not decode")?;
    if decoded.total_amount() != amount {
        bail!(
            "export token amount {} != expected {amount}",
            decoded.total_amount()
        );
    }
    if decoded.proof_count() != expected_count {
        bail!(
            "export token proof count {} != expected {expected_count}",
            decoded.proof_count()
        );
    }
    let missing_dleq = decoded
        .tokens
        .iter()
        .flat_map(|group| group.proofs.iter())
        .filter(|proof| proof.dleq.is_none())
        .count();
    if missing_dleq != 0 {
        bail!("{missing_dleq} exported proofs carry no NUT-12 dleq");
    }

    Ok(SwapReport {
        amount,
        proof_count: expected_count,
        export_token: export,
    })
}

/// GetTokenInfo payload: [mint_len][mint][unit_len][unit][amount:8be]
/// [count:4be] — parse out (amount, proof_count).
fn parse_token_info(info: &[u8]) -> Result<(u64, u32)> {
    let mut offset = 0;
    let mint_len = *info
        .first()
        .context("token info payload truncated (mint length)")? as usize;
    offset += 1 + mint_len;
    let unit_len = *info
        .get(offset)
        .context("token info payload truncated (unit length)")? as usize;
    offset += 1 + unit_len;
    let amount = u64::from_be_bytes(
        info.get(offset..offset + 8)
            .context("token info payload truncated (amount)")?
            .try_into()
            .expect("8 byte slice"),
    );
    offset += 8;
    let count = u32::from_be_bytes(
        info.get(offset..offset + 4)
            .context("token info payload truncated (count)")?
            .try_into()
            .expect("4 byte slice"),
    );
    Ok((amount, count))
}

/// Demo-mint test token: power-of-two proofs over the pinned
/// single-key demo mint (the device's DLEQ trust root).
pub fn generate_test_token(mint: &DemoMint, amount: u64) -> Result<Vec<u8>> {
    let mut proofs = Vec::new();
    let mut remaining = amount;

    let keyset_id = "00".to_string();

    while remaining > 0 {
        let value = 2u64.pow(remaining.ilog2());
        remaining -= value;

        // NUT-00 secret is a STRING: hex-encoded random bytes, hashed as
        // ASCII — hex-decoding first yields a plausible but wrong Y
        // (cross_vectors.rs hex-looking-secret trap).
        let mut secret_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let secret = hex::encode(secret_bytes);

        let blinded = blind_message(secret.as_bytes(), None)?;
        let blinded_sig = mint.sign(&blinded.blinded);
        let sig =
            cashu_core_lite::unblind_signature(&blinded_sig, &blinded.blinder, &mint.public_key())
                .map_err(|_| anyhow::anyhow!("Failed to unblind signature"))?;

        let c = sig.to_bytes().to_vec();

        proofs.push(Proof {
            amount: value,
            keyset_id: keyset_id.clone(),
            secret,
            c,
            dleq: None,
        });
    }

    let token = TokenV4 {
        mint: "demo://micronuts".to_string(),
        unit: "sat".to_string(),
        memo: Some("Generated test token".to_string()),
        tokens: vec![TokenV4Token { keyset_id, proofs }],
    };

    let encoded = cashu_core_lite::encode_token(&token)?;
    Ok(encoded)
}

/// Sign the device's blinded outputs: each entry is [C' 33B || e 32B ||
/// s 32B] — a blind signature plus its NUT-12 DLEQ proof against the
/// pinned demo key.
pub fn sign_blinded_outputs(mint: &DemoMint, payload: &[u8]) -> Result<Vec<u8>> {
    if !payload.len().is_multiple_of(33) {
        anyhow::bail!("Invalid blinded outputs payload: length not multiple of 33");
    }

    let mut signatures = Vec::new();

    for chunk in payload.chunks(33) {
        let blinded = PublicKey::from_sec1_bytes(chunk).context("Invalid blinded public key")?;
        let sig = mint.sign(&blinded);
        signatures.extend_from_slice(sig.to_encoded_point(true).as_bytes());
        let dleq = mint
            .prove_dleq(&blinded)
            .context("Failed to produce NUT-12 DLEQ proof")?;
        signatures.extend_from_slice(&dleq.e.to_secret_bytes());
        signatures.extend_from_slice(&dleq.s.to_secret_bytes());
    }

    Ok(signatures)
}

/// Load a wallet-minted cashuB token (one token line, as printed by
/// `micronuts-wallet --example mint_demo_token`). Returns the import
/// payload and the token's total amount.
fn load_token_file(path: &Path) -> Result<(Vec<u8>, u64)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read token file {}", path.display()))?;
    let text = text.trim();
    anyhow::ensure!(
        text.starts_with("cashuB"),
        "not a cashuB token: {}…",
        &text[..text.len().min(12)]
    );
    let bytes = text.as_bytes().to_vec();
    let total = decode_token(&bytes)
        .context("token file does not decode")?
        .total_amount();
    Ok((bytes, total))
}

/// The device export as a cashuB line so host scripts (the offline-gate
/// leg of the QR handoff) can consume swap output directly.
fn print_export(report: &SwapReport) {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    println!(
        "EXPORT: cashuB{}",
        URL_SAFE_NO_PAD.encode(&report.export_token)
    );
}

fn find_cdc_port() -> Option<String> {
    const VID: u16 = 0x16C0;
    const PID: u16 = 0x27DD;
    for port in serialport::available_ports().ok()? {
        if let serialport::SerialPortType::UsbPort(info) = port.port_type {
            if info.vid == VID && info.pid == PID {
                return Some(port.port_name);
            }
        }
    }
    None
}

/// CLI entry: selftest | wire (port or autodetect, exit 77 when absent).
/// `token_file` hands off a wallet-minted cashuB token instead of a
/// harness-generated one (the browser→device QR-handoff path).
pub fn run_swap(
    selftest: bool,
    port: Option<&Path>,
    amount: u64,
    baud: u32,
    token_file: Option<&Path>,
) -> Result<()> {
    let mint = DemoMint::new();

    let (token_bytes, expected, source) = match token_file {
        Some(path) => {
            let (bytes, total) = load_token_file(path)?;
            (bytes, total, format!("wallet token {}", path.display()))
        }
        None => (
            generate_test_token(&mint, amount)?,
            amount,
            String::from("harness token"),
        ),
    };

    if selftest {
        let mut transport = InProcessTransport::new();
        let report = run_swap_script(&mut transport, &mint, &token_bytes, expected)?;
        println!(
            "SWAP SELFTEST PASS: {} sats, {} proofs, export {} bytes (in-process device, {source})",
            report.amount,
            report.proof_count,
            report.export_token.len()
        );
        print_export(&report);

        // Negative leg: a corrupted signature must fail the DLEQ gate,
        // never produce proofs (the #54 forgery class).
        let mut hostile = InProcessTransport::new();
        let token_bytes = generate_test_token(&mint, amount)?;
        let (status, _) = hostile.exchange(Command::ImportToken, &token_bytes)?;
        if status != Status::Ok {
            bail!("negative leg: import failed unexpectedly: {status:?}");
        }
        let (status, blinded) = hostile.exchange(Command::GetBlinded, &[])?;
        if status != Status::Ok {
            bail!("negative leg: blinded failed unexpectedly: {status:?}");
        }
        let mut signatures = sign_blinded_outputs(&mint, &blinded)?;
        // Flip a bit inside the first DLEQ scalar 'e' (byte 33 of entry 0)
        // — a well-formed point signature with a broken proof.
        signatures[33] ^= 0x01;
        let (status, _) = hostile.exchange(Command::SendSignatures, &signatures)?;
        if status != Status::CryptoError {
            bail!("negative leg: corrupted DLEQ was NOT rejected (got {status:?})");
        }
        let (status, _) = hostile.exchange(Command::GetProofs, &[])?;
        if status == Status::Ok {
            bail!("negative leg: proofs exported despite rejected signatures");
        }
        println!("SWAP SELFTEST NEGATIVE PASS: corrupted DLEQ rejected, no proofs");
        return Ok(());
    }

    let port_name = match port {
        Some(path) => path.to_string_lossy().into_owned(),
        None => match find_cdc_port() {
            Some(name) => name,
            None => {
                eprintln!(
                    "BLOCKED: no Micronuts CDC device (16c0:27dd) found — connect the USB OTG FS cable (CN5, micro-B)"
                );
                std::process::exit(77);
            }
        },
    };

    let mut transport = SerialTransport::open(Path::new(&port_name), baud)?;
    let report = run_swap_script(&mut transport, &mint, &token_bytes, expected)?;
    println!(
        "SWAP WIRE PASS on {port_name}: {} sats, {} proofs, export {} bytes",
        report.amount,
        report.proof_count,
        report.export_token.len()
    );
    print_export(&report);
    Ok(())
}
