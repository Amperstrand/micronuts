//! Firmware state management

use crate::qr::{ScannerModel, ScannerState};
use alloc::vec::Vec;
use cashu_core_lite::{BlindedMessage, Proof, TokenV4};

#[derive(Debug, Clone)]
pub struct ScannerInfo {
    pub model: ScannerModel,
    pub state: ScannerState,
    pub last_scan_len: Option<usize>,
    pub connected: bool,
}

impl Default for ScannerInfo {
    fn default() -> Self {
        Self {
            model: ScannerModel::Unknown,
            state: ScannerState::Uninitialized,
            last_scan_len: None,
            connected: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    Idle,
    TokenImported,
    BlindedGenerated,
    ProofsReady,
}

pub struct FirmwareState {
    pub imported_token: Option<TokenV4>,
    pub blinded_messages: Option<Vec<BlindedMessage>>,
    pub swap_secrets: Option<Vec<Vec<u8>>>,
    pub swap_amounts: Option<Vec<u64>>,
    pub new_proofs: Option<Vec<Proof>>,
    pub swap_state: SwapState,
    pub scanner: ScannerInfo,
    pub last_scan_data: Option<Vec<u8>>,
}

impl FirmwareState {
    pub const fn new() -> Self {
        Self {
            imported_token: None,
            blinded_messages: None,
            swap_secrets: None,
            swap_amounts: None,
            new_proofs: None,
            swap_state: SwapState::Idle,
            scanner: ScannerInfo {
                model: ScannerModel::Unknown,
                state: ScannerState::Uninitialized,
                last_scan_len: None,
                connected: false,
            },
            last_scan_data: None,
        }
    }
}
