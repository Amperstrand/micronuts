extern crate alloc;

use alloc::vec::Vec;

use crate::display;
use crate::hardware::MicronutsHardware;
use crate::protocol::{Command, Response, Status, MAX_PAYLOAD_SIZE};
use crate::qr;
use crate::state::{FirmwareState, SwapState};
use crate::util::{decode_hex, derive_demo_mint_key, encode_hex};
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
        Command::ScannerTrigger => match hw.trigger().await {
            Ok(()) => {
                display::render_status(hw.display(), "Scanning...");
                Response::new(Status::Ok)
            }
            Err(_) => {
                display::render_error(hw.display(), "Scanner error");
                Response::new(Status::ScannerNotConnected)
            }
        },
        Command::ScannerData => match last_scan_data.take() {
            Some(data) => {
                let payload = qr::decode_qr(&data);
                display::render_decoded_scan(hw.display(), &payload);
                let type_byte: u8 = match &payload {
                    qr::QrPayload::CashuV4 { .. } => 0x01,
                    qr::QrPayload::CashuV3 { .. } => 0x02,
                    qr::QrPayload::UrFragment { .. } => 0x03,
                    qr::QrPayload::PlainText(_) => 0x00,
                    qr::QrPayload::Binary(_) => 0x04,
                };
                let max_payload = MAX_PAYLOAD_SIZE;
                let total = 1 + data.len().min(max_payload - 1);
                let mut buf = alloc::vec![type_byte; total];
                buf[1..].copy_from_slice(&data[..total - 1]);
                Response::with_payload(Status::Ok, &buf)
                    .unwrap_or_else(|| Response::new(Status::BufferOverflow))
            }
            None => Response::new(Status::NoScanData),
        },
    }
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
    let mut secrets: Vec<Vec<u8>> = Vec::new();
    let mut amounts: Vec<u64> = Vec::new();

    for token_part in &token.tokens {
        for proof in &token_part.proofs {
            let secret_bytes = match decode_hex(&proof.secret) {
                Some(s) => s,
                None => continue,
            };

            let mut blinder_bytes = [0u8; 32];
            hw.rng_fill_bytes(&mut blinder_bytes);
            let blinder = match SecretKey::from_slice(&blinder_bytes) {
                Ok(sk) => sk,
                Err(_) => continue,
            };

            let blinded = match blind_message(&secret_bytes, Some(blinder)) {
                Ok(b) => b,
                Err(_) => continue,
            };

            secrets.push(secret_bytes);
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

    if payload.len() % 33 != 0 {
        return Response::new(Status::InvalidPayload);
    }

    let sig_count = payload.len() / 33;
    if sig_count != blinded_messages.len() {
        return Response::new(Status::InvalidPayload);
    }

    let mint_pubkey = match derive_demo_mint_key(&state.imported_token) {
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
        let sig_bytes = &payload[i * 33..(i + 1) * 33];
        let mut full_bytes = [0u8; 65];
        full_bytes[0] = 0x04;
        full_bytes[1..34].copy_from_slice(sig_bytes);
        full_bytes[34..].copy_from_slice(&sig_bytes[1..32]);

        let blinded_sig = match PublicKey::from_sec1_bytes(&full_bytes[..65]) {
            Ok(pk) => pk,
            Err(_) => {
                let compressed: [u8; 33] = {
                    let mut arr = [0u8; 33];
                    arr.copy_from_slice(sig_bytes);
                    arr
                };
                match PublicKey::from_sec1_bytes(&compressed) {
                    Ok(pk) => pk,
                    Err(_) => continue,
                }
            }
        };

        let unblinded = match unblind_signature(&blinded_sig, &blinded.blinder, &mint_pubkey) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        let secret = &state.swap_secrets.as_ref().unwrap()[i];
        let amount = state.swap_amounts.as_ref().unwrap()[i];

        let c_vec = unblinded.to_sec1_bytes();

        proofs.push(Proof {
            amount,
            keyset_id: keyset_id.clone(),
            secret: encode_hex(secret),
            c: c_vec,
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

fn handle_get_proofs(state: &mut FirmwareState) -> Response {
    let proofs = match &state.new_proofs {
        Some(p) => p,
        None => return Response::new(Status::Error),
    };

    let token = match &state.imported_token {
        Some(t) => t,
        None => return Response::new(Status::Error),
    };

    let new_token = TokenV4 {
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
    use crate::hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};
    use crate::protocol::{Command, Frame, Status};
    use crate::state::{FirmwareState, SwapState};
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use embedded_graphics::{
        draw_target::DrawTarget,
        geometry::{OriginDimensions, Size},
        pixelcolor::Rgb565,
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
        type Color = Rgb565;
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
    }

    impl MockHardware {
        fn new() -> Self {
            Self {
                display: MockDisplay,
                scanner_connected: false,
                scan_data: None,
            }
        }

        fn with_scanner(connected: bool) -> Self {
            Self {
                display: MockDisplay,
                scanner_connected: connected,
                scan_data: None,
            }
        }
    }

    impl Scanner for MockHardware {
        fn trigger(&mut self) -> impl core::future::Future<Output = Result<(), ScanError>> {
            async move {
                if self.scanner_connected {
                    Ok(())
                } else {
                    Err(ScanError::NotConnected)
                }
            }
        }

        fn try_read(&mut self) -> Option<Vec<u8>> {
            self.scan_data.take()
        }

        fn read_scan(&mut self) -> impl core::future::Future<Output = Option<Vec<u8>>> {
            async move { self.scan_data.take() }
        }

        fn stop(&mut self) -> impl core::future::Future<Output = ()> {
            async move {}
        }

        fn is_connected(&self) -> bool {
            self.scanner_connected
        }

        fn set_aim(
            &mut self,
            _enabled: bool,
        ) -> impl core::future::Future<Output = Result<(), ScanError>> {
            async move { Ok(()) }
        }

        fn debug_dump_settings(&mut self) {}
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

        fn transport_recv_frame(
            &mut self,
        ) -> impl core::future::Future<Output = Option<Frame>> {
            async move { None }
        }

        fn transport_send(
            &mut self,
            _response: &Response,
        ) -> impl core::future::Future<Output = ()> {
            async move {}
        }

        fn touch_get(&mut self) -> Option<TouchPoint> {
            None
        }

        fn delay_ms(&mut self, _ms: u32) -> impl core::future::Future<Output = ()> {
            async move {}
        }
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
                    },
                    cashu_core_lite::Proof {
                        amount: 8,
                        keyset_id: String::from("00"),
                        secret: String::from("11223344"),
                        c: vec![0x02, 0xEF, 0x01],
                    },
                ],
            }],
        }
    }

    #[tokio::test]
    async fn test_import_token_valid() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();

        let response = handle_command(Command::ImportToken, &encoded, &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::TokenImported);
        assert!(state.imported_token.is_some());
    }

    #[tokio::test]
    async fn test_import_token_invalid() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ImportToken, b"garbage data", &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::InvalidPayload);
        assert!(state.imported_token.is_none());
        assert_eq!(state.swap_state, SwapState::Idle);
    }

    #[tokio::test]
    async fn test_import_token_empty() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ImportToken, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::InvalidPayload);
    }

    #[tokio::test]
    async fn test_get_token_info_no_token() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::GetTokenInfo, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_get_token_info_after_import() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(Command::ImportToken, &encoded, &mut state, &mut hw, &mut last_scan).await;

        let response = handle_command(Command::GetTokenInfo, &[], &mut state, &mut hw, &mut last_scan).await;

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

        let response = handle_command(Command::GetBlinded, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_get_blinded_after_import() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(Command::ImportToken, &encoded, &mut state, &mut hw, &mut last_scan).await;

        let response = handle_command(Command::GetBlinded, &[], &mut state, &mut hw, &mut last_scan).await;

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
        let response = handle_command(Command::SendSignatures, &fake_sig, &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_send_signatures_invalid_payload_length() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let bad_payload = [0u8; 34];
        let response = handle_command(Command::SendSignatures, &bad_payload, &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_send_signatures_wrong_count() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let token = sample_token();
        let encoded = cashu_core_lite::encode_token(&token).unwrap();
        let _ = handle_command(Command::ImportToken, &encoded, &mut state, &mut hw, &mut last_scan).await;
        let _ = handle_command(Command::GetBlinded, &[], &mut state, &mut hw, &mut last_scan).await;

        let wrong_sig = [0u8; 33];
        let response = handle_command(Command::SendSignatures, &wrong_sig, &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::InvalidPayload);
    }

    #[tokio::test]
    async fn test_get_proofs_no_proofs() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::GetProofs, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Error);
    }

    #[tokio::test]
    async fn test_scanner_status_disconnected() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ScannerStatus, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.length, 3);
        assert_eq!(response.payload()[0], 0);
    }

    #[tokio::test]
    async fn test_scanner_status_connected() {
        let mut hw = MockHardware::with_scanner(true);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ScannerStatus, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.payload()[0], 1);
    }

    #[tokio::test]
    async fn test_scanner_trigger_disconnected() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ScannerTrigger, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::ScannerNotConnected);
    }

    #[tokio::test]
    async fn test_scanner_trigger_connected() {
        let mut hw = MockHardware::with_scanner(true);
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ScannerTrigger, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::Ok);
    }

    #[tokio::test]
    async fn test_scanner_data_no_data() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = None;

        let response = handle_command(Command::ScannerData, &[], &mut state, &mut hw, &mut last_scan).await;

        assert_eq!(response.status, Status::NoScanData);
    }

    #[tokio::test]
    async fn test_scanner_data_with_data() {
        let mut hw = MockHardware::new();
        let mut state = FirmwareState::new();
        let mut last_scan = Some(vec![0x01, 0x02, 0x03]);

        let response = handle_command(Command::ScannerData, &[], &mut state, &mut hw, &mut last_scan).await;

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

        let r = handle_command(Command::ImportToken, &encoded, &mut state, &mut hw, &mut last_scan).await;
        assert_eq!(r.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::TokenImported);

        let r = handle_command(Command::GetTokenInfo, &[], &mut state, &mut hw, &mut last_scan).await;
        assert_eq!(r.status, Status::Ok);

        let r = handle_command(Command::GetBlinded, &[], &mut state, &mut hw, &mut last_scan).await;
        assert_eq!(r.status, Status::Ok);
        assert_eq!(state.swap_state, SwapState::BlindedGenerated);
        assert_eq!(state.blinded_messages.as_ref().unwrap().len(), 2);

        let blinded_count = state.blinded_messages.as_ref().unwrap().len();
        let fake_sigs = vec![0u8; blinded_count * 33];

        let r = handle_command(Command::SendSignatures, &fake_sigs, &mut state, &mut hw, &mut last_scan).await;
        assert_eq!(r.status, Status::CryptoError);
        assert!(state.new_proofs.is_none());
    }
}
