extern crate alloc;

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    Idle,
    TokenImported,
    BlindedGenerated,
    ProofsReady,
}

pub struct FirmwareState {
    pub imported_token: Option<cashu_core_lite::TokenV4>,
    pub blinded_messages: Option<Vec<cashu_core_lite::BlindedMessage>>,
    pub swap_secrets: Option<Vec<Vec<u8>>>,
    pub swap_amounts: Option<Vec<u64>>,
    pub new_proofs: Option<Vec<cashu_core_lite::Proof>>,
    pub swap_state: SwapState,
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
            last_scan_data: None,
        }
    }
}
