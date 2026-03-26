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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state_is_idle() {
        let state = FirmwareState::new();
        assert_eq!(state.swap_state, SwapState::Idle);
        assert!(state.imported_token.is_none());
        assert!(state.blinded_messages.is_none());
        assert!(state.swap_secrets.is_none());
        assert!(state.swap_amounts.is_none());
        assert!(state.new_proofs.is_none());
        assert!(state.last_scan_data.is_none());
    }

    #[test]
    fn test_swap_state_progression() {
        let mut state = FirmwareState::new();
        assert_eq!(state.swap_state, SwapState::Idle);

        state.swap_state = SwapState::TokenImported;
        assert_eq!(state.swap_state, SwapState::TokenImported);
        assert!(state.swap_state != SwapState::Idle);

        state.swap_state = SwapState::BlindedGenerated;
        assert_eq!(state.swap_state, SwapState::BlindedGenerated);

        state.swap_state = SwapState::ProofsReady;
        assert_eq!(state.swap_state, SwapState::ProofsReady);
    }

    #[test]
    fn test_swap_state_equality() {
        assert_eq!(SwapState::Idle, SwapState::Idle);
        assert_eq!(SwapState::TokenImported, SwapState::TokenImported);
        assert_ne!(SwapState::Idle, SwapState::TokenImported);
        assert_ne!(SwapState::BlindedGenerated, SwapState::ProofsReady);
    }

    #[test]
    fn test_swap_state_copy() {
        let s = SwapState::BlindedGenerated;
        let s2 = s;
        assert_eq!(s, s2);
    }

    #[test]
    fn test_new_state_is_const() {
        const STATE: FirmwareState = FirmwareState::new();
        assert_eq!(STATE.swap_state, SwapState::Idle);
    }
}
