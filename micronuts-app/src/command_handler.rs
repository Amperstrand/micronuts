extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::display;
use crate::hardware::MicronutsHardware;
use crate::protocol::{Command, Response, Status, MAX_PAYLOAD_SIZE};
use crate::qr;
use crate::state::{FirmwareState, SwapState};
use crate::util::pinned_demo_mint_key;
use cashu_core_lite::nuts::nut12::ProofDleq;
use cashu_core_lite::{
    blind_message, decode_token, encode_token, unblind_signature, BlindedMessage, Proof, PublicKey,
    SecretKey, TokenV4, TokenV4Token,
};

pub async fn handle_command<H: MicronutsHardware>(
    command: Command,
    payload: &[u8],
    state: &mut FirmwareState,
    hw: &mut H,
    last_scan_data: &mut Option<Vec<u8>>,
) -> Response {
    match command {
        Command::ImportToken => {
            let token = match decode_token(payload) {
                Ok(t) => t,
                Err(_) => {
                    display::render_error(hw.display(), "Invalid token");
                    return Response::new(Status::InvalidPayload);
                }
            };
            display::render_token_info(hw.display(), &token);
            state.imported_token = Some(token);
            state.swap_state = SwapState::TokenImported;
            Response::new(Status::Ok)
        }
        Command::GetTokenInfo => handle_get_token_info(state),
        Command::GetBlinded => handle_get_blinded(state, hw),
        Command::SendSignatures => handle_send_signatures(payload, state, hw),
        Command::GetProofs => handle_get_proofs(state),
        Command::ScannerStatus => {
            let mut payload = [0u8; MAX_PAYLOAD_SIZE];
            let mut offset = 0;
            payload[offset] = if hw.is_connected() { 1 } else { 0 };
            offset += 1;
            payload[offset] = 0x00; // data_ready: not exposed via Scanner trait
            offset += 1;
            payload[offset] = 0x00; // model: unknown (not exposed via Scanner trait)
            offset += 1;
            Response::with_payload(Status::Ok, &payload[..offset])
                .unwrap_or_else(|| Response::new(Status::Error))
        }
        Command::ScannerTrigger => match hw.set_aim(true).await {
            Ok(()) => match hw.trigger().await {
                Ok(()) => {
                    display::render_status(hw.display(), "Scanning...");
                    Response::new(Status::Ok)
                }
                Err(_) => {
                    display::render_error(hw.display(), "Scanner error");
                    Response::new(Status::ScannerNotConnected)
                }
            },
            Err(_) => {
                display::render_error(hw.display(), "Scanner error");
                Response::new(Status::ScannerNotConnected)
            }
        },
        Command::ScannerHeal => {
            // Crate-owned module heal (gm65-scanner #100 Tier A): deep-sleep
            // reboot keeps settings and baud, then re-init + policy restart.
            // Mid-run recovery for the sustained-load degradation (#92).
            let _ = hw.deep_sleep_reboot().await;
            match hw.reinit_scanner().await {
                Ok(()) => Response::new(Status::Ok),
                Err(_) => Response::new(Status::ScannerNotConnected),
            }
        }
        Command::ScannerFactoryHeal => match hw.factory_heal().await {
            Ok(()) => Response::new(Status::Ok),
            Err(_) => Response::new(Status::ScannerNotConnected),
        },
        Command::ScannerData => match last_scan_data.take() {
            Some(data) => classify_and_assemble(hw, state, &data),
            None => Response::new(Status::NoScanData),
        },
        Command::NfcPoll => {
            if !hw.nfc_is_connected() {
                Response::new(Status::NfcNotConnected)
            } else {
                match hw.nfc_poll().await {
                    Ok(tag_present) => Response::with_payload(Status::Ok, &[tag_present as u8])
                        .unwrap_or_else(|| Response::new(Status::Error)),
                    Err(_) => Response::new(Status::NfcNotConnected),
                }
            }
        }
        Command::NfcData => match hw.nfc_read_ndef().await {
            Some(data) => classify_and_assemble(hw, state, &data),
            None => Response::new(Status::NfcNoTag),
        },
        Command::NfcHeal => match hw.nfc_heal().await {
            Ok(()) => Response::new(Status::Ok),
            Err(_) => Response::new(Status::NfcNotConnected),
        },
    }
}

/// Shared capture-transport completion — the QR scanner (ScannerData)
/// and the NFC reader (NfcData) land here alike: classify the payload,
/// render it, feed the state-level assembler, and answer with the
/// ScannerData wire shape (type byte, optional assembler-outcome byte
/// for UR fragments, then the raw payload). Both transports are a
/// differential pair: identical payloads must import byte-identical
/// tokens.
fn classify_and_assemble<H: MicronutsHardware>(
    hw: &mut H,
    state: &mut FirmwareState,
    data: &[u8],
) -> Response {
    let payload = qr::decode_qr(data);
    display::render_decoded_scan(hw.display(), &payload);
    let type_byte: u8 = match &payload {
        qr::QrPayload::CashuV4 { .. } => 0x01,
        qr::QrPayload::CashuV3 { .. } => 0x02,
        qr::QrPayload::UrFragment { .. } => 0x03,
        qr::QrPayload::PlainText(_) => 0x00,
        qr::QrPayload::Binary(_) => 0x04,
    };
    // On-device reassembly for the CDC path (the Scanning screen feeds
    // the same state-level assembler): a completed sequence imports the
    // token immediately. UR responses carry the assembler outcome as a
    // second byte so hosts can observe device-side progress: 0 accepted,
    // 1 invalid (hash mismatch / bad index), 2 completed + imported,
    // 3 completed but decode failed.
    let mut asm_byte = 0u8;
    match state.scan_assembler.process(data) {
        crate::scanflow::ScanOutcome::TokenReady(token) => {
            asm_byte = 2;
            display::render_token_info(hw.display(), &token);
            state.imported_token = Some(token);
            state.swap_state = crate::state::SwapState::TokenImported;
        }
        crate::scanflow::ScanOutcome::InvalidFragment => asm_byte = 1,
        crate::scanflow::ScanOutcome::ShowPayload(crate::qr::QrPayload::PlainText(_)) => {
            asm_byte = 3
        }
        _ => {}
    }
    let header_len = if type_byte == 0x03 { 2 } else { 1 };
    let total = header_len + data.len().min(MAX_PAYLOAD_SIZE - header_len);
    let mut buf = alloc::vec![type_byte; total];
    if type_byte == 0x03 {
        buf[1] = asm_byte;
    }
    buf[header_len..].copy_from_slice(&data[..total - header_len]);
    Response::with_payload(Status::Ok, &buf)
        .unwrap_or_else(|| Response::new(Status::BufferOverflow))
}

fn handle_get_token_info(state: &mut FirmwareState) -> Response {
    match &state.imported_token {
        Some(token) => {
            let mint = token.mint.as_bytes();
            let unit = token.unit.as_bytes();
            let total_len = 1 + mint.len() + 1 + unit.len() + 8 + 4;

            if total_len > MAX_PAYLOAD_SIZE {
                return Response::new(Status::BufferOverflow);
            }

            let mut payload = [0u8; MAX_PAYLOAD_SIZE];
            let mut offset = 0;

            payload[offset] = mint.len() as u8;
            offset += 1;
            payload[offset..offset + mint.len()].copy_from_slice(mint);
            offset += mint.len();

            payload[offset] = unit.len() as u8;
            offset += 1;
            payload[offset..offset + unit.len()].copy_from_slice(unit);
            offset += unit.len();

            let amount = token.total_amount();
            payload[offset..offset + 8].copy_from_slice(&amount.to_be_bytes());
            offset += 8;

            let count = token.proof_count() as u32;
            payload[offset..offset + 4].copy_from_slice(&count.to_be_bytes());
            offset += 4;

            Response::with_payload(Status::Ok, &payload[..offset])
                .unwrap_or_else(|| Response::new(Status::BufferOverflow))
        }
        None => Response::new(Status::Error),
    }
}

fn handle_get_blinded<H: MicronutsHardware>(state: &mut FirmwareState, hw: &mut H) -> Response {
    let token = match &state.imported_token {
        Some(t) => t,
        None => return Response::new(Status::Error),
    };

    let mut blinded_messages: Vec<BlindedMessage> = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    let mut amounts: Vec<u64> = Vec::new();

    for token_part in &token.tokens {
        for proof in &token_part.proofs {
            // NUT-00 secret is a STRING: hash_to_curve operates on its
            // ASCII bytes (cross_vectors.rs hex-looking-secret trap).
            let secret_bytes = proof.secret.as_bytes();

            let mut blinder_bytes = [0u8; 32];
            hw.rng_fill_bytes(&mut blinder_bytes);
            let blinder = match SecretKey::from_slice(&blinder_bytes) {
                Ok(sk) => sk,
                Err(_) => continue,
            };

            let blinded = match blind_message(secret_bytes, Some(blinder)) {
                Ok(b) => b,
                Err(_) => continue,
            };

            secrets.push(proof.secret.clone());
            amounts.push(proof.amount);
            blinded_messages.push(blinded);
        }
    }

    if blinded_messages.is_empty() {
        return Response::new(Status::CryptoError);
    }

    let total_len = blinded_messages.len() * 33;
    if total_len > MAX_PAYLOAD_SIZE {
        return Response::new(Status::BufferOverflow);
    }

    let mut payload = [0u8; MAX_PAYLOAD_SIZE];
    let mut offset = 0;

    for blinded in &blinded_messages {
        let point = blinded.blinded.to_encoded_point(true);
        payload[offset..offset + 33].copy_from_slice(point.as_bytes());
        offset += 33;
    }

    state.blinded_messages = Some(blinded_messages);
    state.swap_secrets = Some(secrets);
    state.swap_amounts = Some(amounts);
    state.swap_state = SwapState::BlindedGenerated;

    display::render_status(hw.display(), "Blinded outputs ready");

    Response::with_payload(Status::Ok, &payload[..offset])
        .unwrap_or_else(|| Response::new(Status::BufferOverflow))
}

fn handle_send_signatures<H: MicronutsHardware>(
    payload: &[u8],
    state: &mut FirmwareState,
    hw: &mut H,
) -> Response {
    let blinded_messages = match &state.blinded_messages {
        Some(bm) => bm,
        None => return Response::new(Status::Error),
    };

    // #54: each entry is [C' 33B || e 32B || s 32B] — a blind signature
    // plus its NUT-12 DLEQ proof. The DLEQ is verified against the PINNED
    // mint key; without it any parseable point would be accepted (the
    // gate-2 W1 forgery).
    const ENTRY_LEN: usize = 33 + 32 + 32;
    if !payload.len().is_multiple_of(ENTRY_LEN) {
        return Response::new(Status::InvalidPayload);
    }

    let sig_count = payload.len() / ENTRY_LEN;
    if sig_count != blinded_messages.len() {
        return Response::new(Status::InvalidPayload);
    }

    let mint_pubkey = match pinned_demo_mint_key() {
        Ok(pk) => pk,
        Err(_) => return Response::new(Status::CryptoError),
    };

    let mut proofs: Vec<Proof> = Vec::new();
    let keyset_id = state
        .imported_token
        .as_ref()
        .and_then(|t| t.tokens.first())
        .map(|t| t.keyset_id.clone())
        .unwrap_or_else(|| alloc::string::String::from("00"));

    for (i, blinded) in blinded_messages.iter().enumerate() {
        let entry = &payload[i * ENTRY_LEN..(i + 1) * ENTRY_LEN];
        let (c_prime_bytes, e_bytes, s_bytes) = (&entry[..33], &entry[33..65], &entry[65..]);

        let blinded_sig = match PublicKey::from_sec1_bytes(c_prime_bytes) {
            Ok(pk) => pk,
            Err(_) => return Response::new(Status::CryptoError),
        };

        // #54: NUT-12 DLEQ — the signer must prove knowledge of the mint
        // scalar behind the PINNED key (fail closed on any invalid entry).
        let (e, s) = match (
            SecretKey::from_slice(e_bytes),
            SecretKey::from_slice(s_bytes),
        ) {
            (Ok(e), Ok(s)) => (e, s),
            _ => return Response::new(Status::CryptoError),
        };
        match cashu_core_lite::nuts::nut12::verify_dleq(
            &blinded.blinded,
            &blinded_sig,
            &e,
            &s,
            &mint_pubkey,
        ) {
            Ok(true) => {}
            _ => return Response::new(Status::CryptoError),
        }

        let unblinded = match unblind_signature(&blinded_sig, &blinded.blinder, &mint_pubkey) {
            Ok(pk) => pk,
            Err(_) => return Response::new(Status::CryptoError),
        };

        let secret = state.swap_secrets.as_ref().unwrap()[i].clone();
        let amount = state.swap_amounts.as_ref().unwrap()[i];

        let c_vec = unblinded.to_bytes().to_vec();

        proofs.push(Proof {
            amount,
            keyset_id: keyset_id.clone(),
            secret,
            c: c_vec,
            dleq: Some(ProofDleq::new(e, s, blinded.blinder.clone())),
        });
    }

    if proofs.is_empty() {
        return Response::new(Status::CryptoError);
    }

    state.new_proofs = Some(proofs);
    state.swap_state = SwapState::ProofsReady;

    display::render_status(hw.display(), "Proofs ready");

    Response::new(Status::Ok)
}

/// The export token for the completed swap: proofs re-wrapped in a V4
/// token carrying the imported mint/unit and the fixed export memo.
/// Shared by the GetProofs wire response and the on-device proof-QR
/// display so both export the SAME token (#29 QR round-trip).
pub fn build_export_token(state: &FirmwareState) -> Option<TokenV4> {
    let proofs = state.new_proofs.as_ref()?;
    let token = state.imported_token.as_ref()?;
    Some(TokenV4 {
        mint: token.mint.clone(),
        unit: token.unit.clone(),
        memo: Some(alloc::string::String::from("Swapped via Micronuts")),
        tokens: alloc::vec![TokenV4Token {
            keyset_id: proofs
                .first()
                .map(|p| p.keyset_id.clone())
                .unwrap_or_else(|| alloc::string::String::from("00")),
            proofs: proofs.clone(),
        }],
    })
}

fn handle_get_proofs(state: &mut FirmwareState) -> Response {
    let new_token = match build_export_token(state) {
        Some(t) => t,
        None => return Response::new(Status::Error),
    };

    let encoded = match encode_token(&new_token) {
        Ok(e) => e,
        Err(_) => return Response::new(Status::Error),
    };

    if encoded.len() > MAX_PAYLOAD_SIZE {
        return Response::new(Status::BufferOverflow);
    }

    Response::with_payload(Status::Ok, &encoded)
        .unwrap_or_else(|| Response::new(Status::BufferOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{MicronutsHardware, NfcReader, ScanError, Scanner, TouchPoint};
    use crate::protocol::{Command, Frame, Status};
    use crate::state::{FirmwareState, SwapState};
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use embedded_graphics::{
        draw_target::DrawTarget,
        geometry::{OriginDimensions, Size},
        pixelcolor::Rgb888,
        Pixel,
    };
    use rand::RngCore;

    struct MockDisplay;

    impl OriginDimensions for MockDisplay {
        fn size(&self) -> Size {
            Size::new(480, 800)
        }
    }

    impl DrawTarget for MockDisplay {
        type Color = Rgb888;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, _pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            Ok(())
        }

        fn clear(&mut self, _color: Self::Color) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct MockHardware {
        display: MockDisplay,
        scanner_connected: bool,
        scan_data: Option<Vec<u8>>,
        heal_count: u32,
        factory_heal_count: u32,
        nfc_connected: bool,
        ndef_payload: Option<Vec<u8>>,
        nfc_heal_count: u32,
    }

    impl MockHardware {
        fn new() -> Self {
            Self {
                display: MockDisplay,
                scanner_connected: false,
                scan_data: None,
                heal_count: 0,
                factory_heal_count: 0,
                nfc_connected: false,
                ndef_payload: None,
                nfc_heal_count: 0,
            }
        }

        fn with_scanner(connected: bool) -> Self {
            Self {
                display: MockDisplay,
                scanner_connected: connected,
                scan_data: None,
                heal_count: 0,
                factory_heal_count: 0,
                nfc_connected: false,
                ndef_payload: None,
                nfc_heal_count: 0,
            }
        }

        fn with_nfc(ndef: Vec<u8>) -> Self {
            Self {
                display: MockDisplay,
                scanner_connected: false,
                scan_data: None,
                heal_count: 0,
                factory_heal_count: 0,
                nfc_connected: true,
                ndef_payload: Some(ndef),
                nfc_heal_count: 0,
            }
        }
    }

    impl Scanner for MockHardware {
        async fn trigger(&mut self) -> Result<(), ScanError> {
            if self.scanner_connected {
                Ok(())
            } else {
                Err(ScanError::NotConnected)
            }
        }

        async fn read_scan(&mut self) -> Option<Vec<u8>> {
            self.scan_data.take()
        }

        async fn stop(&mut self) {}

        fn is_connected(&self) -> bool {
            self.scanner_connected
        }

        async fn set_aim(&mut self, _enabled: bool) -> Result<(), ScanError> {
            Ok(())
        }

        fn debug_dump_settings(&mut self) {}

        async fn deep_sleep_reboot(&mut self) -> bool {
            self.heal_count += 1;
            true
        }

        async fn reinit_scanner(&mut self) -> Result<(), ScanError> {
            if self.scanner_connected {
                Ok(())
            } else {
                Err(ScanError::NotConnected)
            }
        }

        async fn factory_heal(&mut self) -> Result<(), ScanError> {
            self.factory_heal_count += 1;
            if self.scanner_connected {
                Ok(())
            } else {
                Err(ScanError::NotConnected)
            }
        }
    }

    impl NfcReader for MockHardware {
        fn nfc_is_connected(&self) -> bool {
            self.nfc_connected
        }

        async fn nfc_poll(&mut self) -> Result<bool, ScanError> {
            if self.nfc_connected {
                Ok(self.ndef_payload.is_some())
            } else {
                Err(ScanError::NotConnected)
            }
        }

        async fn nfc_read_ndef(&mut self) -> Option<Vec<u8>> {
            self.ndef_payload.take()
        }

        async fn nfc_heal(&mut self) -> Result<(), ScanError> {
            self.nfc_heal_count += 1;
            if self.nfc_connected {
                Ok(())
            } else {
                Err(ScanError::NotConnected)
            }
        }
    }

    impl MicronutsHardware for MockHardware {
        type Display = MockDisplay;

        fn display(&mut self) -> &mut Self::Display {
            &mut self.display
        }

        fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
            let mut rng = rand::thread_rng();
            rng.fill_bytes(dest);
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

    fn sample_token() -> cashu_core_lite::TokenV4 {
        cashu_core_lite::TokenV4 {
            mint: String::from("https://example.com/mint"),
            unit: String::from("sat"),
            memo: Some(String::from("test memo")),
            tokens: vec![cashu_core_lite::TokenV4Token {
                keyset_id: String::from("00"),
                proofs: vec![
                    cashu_core_lite::Proof {
                        amount: 2,
                        keyset_id: String::from("00"),
                        secret: String::from("aabbccdd"),
                        c: vec![0x02, 0xAB, 0xCD],
                        dleq: None,
                    },
                    cashu_core_lite::Proof {
                        amount: 8,
                        keyset_id: String::from("00"),
                        secret: String::from("11223344"),
                        c: vec![0x02, 0xEF, 0x01],
                        dleq: None,
                    },
                ],
            }],
        }
    }

    #[tokio::test]
    async fn scanner_heal_reboots_and_reinits() {
        use crate::protocol::{Command, Status};
        let mut hw = MockHardware::with_scanner(true);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let resp = handle_command(
            Command::ScannerHeal,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(
            hw.heal_count, 1,
            "heal must issue exactly one module reboot"
        );
    }

    #[tokio::test]
    async fn scanner_heal_reports_disconnected_scanner() {
        use crate::protocol::{Command, Status};
        let mut hw = MockHardware::with_scanner(false);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let resp = handle_command(
            Command::ScannerHeal,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(resp.status, Status::ScannerNotConnected);
    }

    #[tokio::test]
    async fn scanner_data_reassembles_ur_and_imports_token() {
        use crate::protocol::{Command, Status};
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let wire = cashu_core_lite::encode_token_wire(&sample_token()).unwrap();
        let (a, b) = wire.split_at(wire.len() / 2);
        let hash = "deadbeef";
        for (idx, chunk) in [(1usize, a), (2usize, b)] {
            let mut last_scan = Some(format!("ur:bytes/{}-2/{}/{}", idx, hash, chunk).into_bytes());
            let resp = handle_command(
                Command::ScannerData,
                &[],
                &mut state,
                &mut hw,
                &mut last_scan,
            )
            .await;
            assert_eq!(resp.status, Status::Ok);
            if idx == 1 {
                assert!(
                    state.imported_token.is_none(),
                    "no import before the sequence completes"
                );
            }
        }

        assert_eq!(state.swap_state, crate::state::SwapState::TokenImported);
        let info = handle_command(Command::GetTokenInfo, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(info.status, Status::Ok);
        // payload = mint-len || mint || unit-len || unit || amount(8) || proofs(4)
        let p = info.payload();
        let mint_len = p[0] as usize;
        let unit_len = p[1 + mint_len] as usize;
        let off = 1 + mint_len + 1 + unit_len;
        let amount = u64::from_be_bytes(p[off..off + 8].try_into().unwrap());
        let proofs = u32::from_be_bytes(p[off + 8..off + 12].try_into().unwrap());
        assert_eq!((amount, proofs), (10, 2));
    }

    #[tokio::test]
    async fn nfc_poll_reports_tag_present() {
        let mut hw = MockHardware::with_nfc(b"cashuBxyz".to_vec());
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcPoll, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.payload(), &[0x01]);
    }

    #[tokio::test]
    async fn nfc_poll_reports_no_tag() {
        let mut hw = MockHardware::with_nfc(Vec::new());
        hw.ndef_payload = None;
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcPoll, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.payload(), &[0x00]);
    }

    #[tokio::test]
    async fn nfc_poll_reports_reader_absent() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcPoll, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::NfcNotConnected);
    }

    #[tokio::test]
    async fn nfc_data_reports_no_tag() {
        let mut hw = MockHardware::with_nfc(Vec::new());
        hw.ndef_payload = None;
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcData, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::NfcNoTag);
        assert!(state.imported_token.is_none());
    }

    #[tokio::test]
    async fn nfc_data_returns_plain_text_classified() {
        let mut hw = MockHardware::with_nfc(b"just some text".to_vec());
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcData, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.payload()[0], 0x00);
        assert_eq!(&resp.payload()[1..], b"just some text");
        assert!(state.imported_token.is_none());
    }

    #[tokio::test]
    async fn nfc_data_imports_token() {
        let wire = cashu_core_lite::encode_token_wire(&sample_token())
            .unwrap()
            .into_bytes();
        let mut hw = MockHardware::with_nfc(wire);
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcData, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(resp.payload()[0], 0x01);
        assert_eq!(state.swap_state, SwapState::TokenImported);
        assert!(state.imported_token.is_some());
    }

    #[tokio::test]
    async fn nfc_data_reassembles_ur_and_imports_token() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let wire = cashu_core_lite::encode_token_wire(&sample_token()).unwrap();
        let (a, b) = wire.split_at(wire.len() / 2);
        let hash = "deadbeef";
        for (idx, chunk) in [(1usize, a), (2usize, b)] {
            hw.ndef_payload = Some(format!("ur:bytes/{}-2/{}/{}", idx, hash, chunk).into_bytes());
            let resp = handle_command(Command::NfcData, &[], &mut state, &mut hw, &mut None).await;
            assert_eq!(resp.status, Status::Ok);
            if idx == 1 {
                assert!(
                    state.imported_token.is_none(),
                    "no import before the sequence completes"
                );
            }
        }

        assert_eq!(state.swap_state, SwapState::TokenImported);
    }

    #[tokio::test]
    async fn nfc_and_qr_paths_import_identical_tokens() {
        let wire = cashu_core_lite::encode_token_wire(&sample_token()).unwrap();

        let mut hw_qr = MockHardware::new();
        let mut state_qr = FirmwareState::new();
        let mut last_scan = Some(wire.clone().into_bytes());
        let r_qr = handle_command(
            Command::ScannerData,
            &[],
            &mut state_qr,
            &mut hw_qr,
            &mut last_scan,
        )
        .await;
        assert_eq!(r_qr.status, Status::Ok);
        assert_eq!(state_qr.swap_state, SwapState::TokenImported);

        let mut hw_nfc = MockHardware::with_nfc(wire.into_bytes());
        let mut state_nfc = FirmwareState::new();
        let r_nfc = handle_command(
            Command::NfcData,
            &[],
            &mut state_nfc,
            &mut hw_nfc,
            &mut None,
        )
        .await;
        assert_eq!(r_nfc.status, Status::Ok);
        assert_eq!(state_nfc.swap_state, SwapState::TokenImported);

        // Differential invariant: both capture transports converge on
        // byte-identical wire responses and byte-identical tokens.
        assert_eq!(r_qr.payload(), r_nfc.payload());
        let t_qr = state_qr.imported_token.as_ref().unwrap();
        let t_nfc = state_nfc.imported_token.as_ref().unwrap();
        assert_eq!(
            cashu_core_lite::encode_token(t_qr).unwrap(),
            cashu_core_lite::encode_token(t_nfc).unwrap()
        );
        assert_eq!(t_qr.mint, t_nfc.mint);
        assert_eq!(t_qr.unit, t_nfc.unit);
        assert_eq!(t_qr.total_amount(), t_nfc.total_amount());
        assert_eq!(t_qr.proof_count(), t_nfc.proof_count());
    }

    #[tokio::test]
    async fn nfc_heal_reinits_reader() {
        let mut hw = MockHardware::with_nfc(Vec::new());
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcHeal, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::Ok);
        assert_eq!(
            hw.nfc_heal_count, 1,
            "heal must issue exactly one reader re-init"
        );
    }

    #[tokio::test]
    async fn nfc_heal_reports_reader_absent() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();

        let resp = handle_command(Command::NfcHeal, &[], &mut state, &mut hw, &mut None).await;
        assert_eq!(resp.status, Status::NfcNotConnected);
    }

    #[tokio::test]
    async fn test_import_token_valid() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();

        let response = handle_command(
            Command::ImportToken,
            &encoded,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::TokenImported);
        assert!(state.imported_token.is_some());
    }

    #[tokio::test]
    async fn test_import_token_invalid() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ImportToken,
            b"garbage data",
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::InvalidPayload);
        assert!(state.imported_token.is_none());
        assert_eq!(state.swap_state, SwapState::Idle);
    }

    #[tokio::test]
    async fn test_import_token_empty() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ImportToken,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::InvalidPayload);
    }

    #[tokio::test]
    async fn test_get_token_info_no_token() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::GetTokenInfo,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_get_token_info_after_import() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(
            Command::ImportToken,
            &encoded,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        let response = handle_command(
            Command::GetTokenInfo,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert!(response.length > 0);

        let payload = response.payload();
        let mint_len = payload[0] as usize;
        let mint = core::str::from_utf8(&payload[1..1 + mint_len]).unwrap();
        assert_eq!(mint, "https://example.com/mint");

        let offset = 1 + mint_len;
        let unit_len = payload[offset] as usize;
        let unit = core::str::from_utf8(&payload[offset + 1..offset + 1 + unit_len]).unwrap();
        assert_eq!(unit, "sat");

        let amount_offset = offset + 1 + unit_len;
        let mut amount_bytes = [0u8; 8];
        amount_bytes.copy_from_slice(&payload[amount_offset..amount_offset + 8]);
        let amount = u64::from_be_bytes(amount_bytes);
        assert_eq!(amount, 10);

        let count_offset = amount_offset + 8;
        let mut count_bytes = [0u8; 4];
        count_bytes.copy_from_slice(&payload[count_offset..count_offset + 4]);
        let count = u32::from_be_bytes(count_bytes);
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_get_blinded_no_token() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::GetBlinded,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_get_blinded_after_import() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(
            Command::ImportToken,
            &encoded,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        let response = handle_command(
            Command::GetBlinded,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::BlindedGenerated);
        assert!(state.blinded_messages.is_some());
        assert!(state.swap_secrets.is_some());
        assert!(state.swap_amounts.is_some());

        let blinded = state.blinded_messages.as_ref().unwrap();
        assert_eq!(blinded.len(), 2);
        assert_eq!(response.length, 2 * 33);
    }

    #[tokio::test]
    async fn test_send_signatures_no_blinded() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let fake_sig = [0u8; 66];
        let response = handle_command(
            Command::SendSignatures,
            &fake_sig,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_send_signatures_invalid_payload_length() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let bad_payload = [0u8; 34];
        let response = handle_command(
            Command::SendSignatures,
            &bad_payload,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_send_signatures_wrong_count() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(
            Command::ImportToken,
            &encoded,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        let _ = handle_command(
            Command::GetBlinded,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        let wrong_sig = [0u8; 33];
        let response = handle_command(
            Command::SendSignatures,
            &wrong_sig,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::InvalidPayload);
    }

    #[tokio::test]
    async fn test_get_proofs_no_proofs() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response =
            handle_command(Command::GetProofs, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_scanner_status_disconnected() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ScannerStatus,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.length, 3);
        assert_eq!(response.payload()[0], 0);
    }

    #[tokio::test]
    async fn test_scanner_status_connected() {
        let mut hw = MockHardware::with_scanner(true);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ScannerStatus,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.payload()[0], 1);
    }

    #[tokio::test]
    async fn test_scanner_trigger_disconnected() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ScannerTrigger,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::ScannerNotConnected);
    }

    #[tokio::test]
    async fn test_scanner_trigger_connected() {
        let mut hw = MockHardware::with_scanner(true);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ScannerTrigger,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
    }

    #[tokio::test]
    async fn test_scanner_data_no_data() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(
            Command::ScannerData,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::NoScanData);
    }

    #[tokio::test]
    async fn test_scanner_data_with_data() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = Some(vec![0x01, 0x02, 0x03]);

        let response = handle_command(
            Command::ScannerData,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;

        assert_eq!(response.status, Status::Ok);
        assert!(response.length > 0);
        assert!(last_scan.is_none());
    }

    #[tokio::test]
    async fn test_full_swap_flow() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        assert_eq!(state.swap_state, SwapState::Idle);

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();

        let r = handle_command(
            Command::ImportToken,
            &encoded,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(r.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::TokenImported);

        let r = handle_command(
            Command::GetTokenInfo,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(r.status, Status::Ok);

        let r = handle_command(
            Command::GetBlinded,
            &[],
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(r.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::BlindedGenerated);
        assert_eq!(state.blinded_messages.as_ref().unwrap().len(), 2);

        let blinded_count = state.blinded_messages.as_ref().unwrap().len();
        let fake_sigs = vec![0u8; blinded_count * (33 + 32 + 32)];

        let r = handle_command(
            Command::SendSignatures,
            &fake_sigs,
            &mut state,
            &mut hw,
            &mut last_scan,
        )
        .await;
        assert_eq!(r.status, Status::CryptoError);
        assert!(state.new_proofs.is_none());
    }

    // #54 forgery regressions: SendSignatures must verify the NUT-12 DLEQ
    // against the PINNED demo mint key, never the token's claimed mint URL.
    // Assertions pin Status::CryptoError exactly (not `!= Ok`) so a
    // regression to length-based rejection cannot masquerade as verification.

    fn token_with_mint(mint: &str) -> cashu_core_lite::TokenV4 {
        cashu_core_lite::TokenV4 {
            mint: String::from(mint),
            unit: String::from("sat"),
            memo: None,
            tokens: vec![cashu_core_lite::TokenV4Token {
                keyset_id: String::from("00"),
                proofs: vec![cashu_core_lite::Proof {
                    amount: 8,
                    keyset_id: String::from("00"),
                    secret: String::from("aabbccdd"),
                    c: vec![0x02, 0xAB, 0xCD],
                    dleq: None,
                }],
            }],
        }
    }

    fn pinned_mint_secret() -> cashu_core_lite::SecretKey {
        use sha2::{Digest, Sha256};
        cashu_core_lite::SecretKey::from_slice(&Sha256::digest(b"demo://micronuts")).unwrap()
    }

    async fn import_and_blind(mint: &str, state: &mut FirmwareState, hw: &mut MockHardware) {
        let enc = cashu_core_lite::encode_token(&token_with_mint(mint)).unwrap();
        let r = handle_command(Command::ImportToken, &enc, state, hw, &mut None).await;
        assert_eq!(r.status, Status::Ok);
        let r = handle_command(Command::GetBlinded, &[], state, hw, &mut None).await;
        assert_eq!(r.status, Status::Ok);
    }

    #[tokio::test]
    async fn url_forged_key_signature_rejected() {
        let mut state = FirmwareState::new();
        let mut hw = MockHardware::new();
        import_and_blind("https://attacker.example", &mut state, &mut hw).await;

        // The attacker derives the mint scalar for THEIR url and produces
        // a perfectly valid signature + DLEQ against their own key. Only
        // pinning (not signature validity) rejects it.
        use sha2::{Digest, Sha256};
        let atk_sk = cashu_core_lite::SecretKey::from_slice(&Sha256::digest(
            state.imported_token.as_ref().unwrap().mint.as_bytes(),
        ))
        .unwrap();
        let mut payload: Vec<u8> = Vec::new();
        {
            let blinded = state.blinded_messages.as_ref().unwrap();
            for bm in blinded.iter() {
                let c_prime = cashu_core_lite::sign_message(&atk_sk, &bm.blinded);
                let dleq =
                    cashu_core_lite::nuts::nut12::prove_dleq(&bm.blinded, &atk_sk, None).unwrap();
                payload.extend_from_slice(&c_prime.to_bytes());
                payload.extend_from_slice(&dleq.e.to_secret_bytes());
                payload.extend_from_slice(&dleq.s.to_secret_bytes());
            }
        }
        let r = handle_command(
            Command::SendSignatures,
            &payload,
            &mut state,
            &mut hw,
            &mut None,
        )
        .await;
        assert_eq!(r.status, Status::CryptoError);
        assert!(state.new_proofs.is_none());
    }

    fn k256_sec1_generator_compressed() -> [u8; 33] {
        let mut g = [0u8; 33];
        g[0] = 0x02;
        g[1..].copy_from_slice(&[
            0x79, 0xBE, 0x66, 0x7E, 0xF9, 0xDC, 0xBB, 0xAC, 0x55, 0xA0, 0x62, 0x95, 0xCE, 0x87,
            0x0B, 0x07, 0x02, 0x9B, 0xFC, 0xDB, 0x2D, 0xCE, 0x28, 0xD9, 0x59, 0xF2, 0x81, 0x5B,
            0x16, 0xF8, 0x17, 0x98,
        ]);
        g
    }

    #[tokio::test]
    async fn generator_point_signature_rejected() {
        let mut state = FirmwareState::new();
        let mut hw = MockHardware::new();
        import_and_blind("demo://micronuts", &mut state, &mut hw).await;

        // C' = the generator point parses as a valid pubkey; e/s are valid
        // scalars. Only the DLEQ check can reject the entry.
        let gen =
            cashu_core_lite::PublicKey::from_sec1_bytes(&k256_sec1_generator_compressed()).unwrap();
        let mut payload: Vec<u8> = Vec::new();
        for _ in state.blinded_messages.as_ref().unwrap().iter() {
            payload.extend_from_slice(&gen.to_bytes());
            let mut scalar = [0u8; 32];
            scalar[31] = 0x01;
            payload.extend_from_slice(&scalar);
            scalar[31] = 0x02;
            payload.extend_from_slice(&scalar);
        }
        let r = handle_command(
            Command::SendSignatures,
            &payload,
            &mut state,
            &mut hw,
            &mut None,
        )
        .await;
        assert_eq!(r.status, Status::CryptoError);
        assert!(state.new_proofs.is_none());
    }

    #[tokio::test]
    async fn pinned_mint_signature_with_dleq_accepted() {
        let mut state = FirmwareState::new();
        let mut hw = MockHardware::new();
        import_and_blind("demo://micronuts", &mut state, &mut hw).await;

        let mint_sk = pinned_mint_secret();
        let mut payload: Vec<u8> = Vec::new();
        let blinded_count;
        {
            let blinded = state.blinded_messages.as_ref().unwrap();
            blinded_count = blinded.len();
            for bm in blinded.iter() {
                let c_prime = cashu_core_lite::sign_message(&mint_sk, &bm.blinded);
                let dleq =
                    cashu_core_lite::nuts::nut12::prove_dleq(&bm.blinded, &mint_sk, None).unwrap();
                payload.extend_from_slice(&c_prime.to_bytes());
                payload.extend_from_slice(&dleq.e.to_secret_bytes());
                payload.extend_from_slice(&dleq.s.to_secret_bytes());
            }
        }
        let r = handle_command(
            Command::SendSignatures,
            &payload,
            &mut state,
            &mut hw,
            &mut None,
        )
        .await;
        assert_eq!(r.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::ProofsReady);
        let proofs = state.new_proofs.as_ref().unwrap();
        assert_eq!(proofs.len(), blinded_count);

        // NUT-00: C is the COMPRESSED 33-byte point — a 65-byte
        // uncompressed encoding is not what conformant wallets parse.
        for proof in proofs {
            assert_eq!(proof.c.len(), 33);
        }

        // Offline re-verification with the proof-level DLEQ. The secret is
        // a STRING (cross_vectors.rs pins the hex-looking-secret trap: it
        // must be hashed as its ASCII characters, never hex-decoded).
        let mint_pk = crate::util::pinned_demo_mint_key().unwrap();
        for proof in proofs {
            let c = cashu_core_lite::PublicKey::from_sec1_bytes(&proof.c).unwrap();
            assert!(cashu_core_lite::nuts::nut12::verify_proof_dleq(
                proof.secret.as_bytes(),
                &c,
                proof.dleq.as_ref().unwrap(),
                &mint_pk
            ));
        }
    }
}
