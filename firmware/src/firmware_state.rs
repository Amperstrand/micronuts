//! Firmware state management

use alloc::vec::Vec;
use cashu_core_lite::{BlindedMessage, Proof, TokenV4};

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
        }
    }
}
