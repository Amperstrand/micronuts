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

pub fn handle_command<H: MicronutsHardware>(
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
        Command::ScannerTrigger => match hw.trigger() {
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
