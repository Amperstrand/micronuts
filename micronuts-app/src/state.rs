extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use cashu_core_lite::{Proof, TokenV4, TokenV4Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    Idle,
    TokenImported,
    BlindedGenerated,
    ProofsReady,
}

pub struct FirmwareState {
    pub imported_token: Option<TokenV4>,
    pub blinded_messages: Option<Vec<cashu_core_lite::BlindedMessage>>,
    pub swap_secrets: Option<Vec<Vec<u8>>>,
    pub swap_amounts: Option<Vec<u64>>,
    pub new_proofs: Option<Vec<Proof>>,
    pub swap_state: SwapState,
    pub last_scan_data: Option<Vec<u8>>,
}

pub fn build_swap_token(token: &TokenV4, proofs: &[Proof]) -> TokenV4 {
    TokenV4 {
        mint: token.mint.clone(),
        unit: token.unit.clone(),
        memo: Some(String::from("Swapped via Micronuts")),
        tokens: vec![TokenV4Token {
            keyset_id: proofs
                .first()
                .map(|p| p.keyset_id.clone())
                .unwrap_or_else(|| String::from("00")),
            proofs: proofs.to_vec(),
        }],
    }
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

    #[test]
    fn test_build_swap_token() {
        let token = TokenV4 {
            mint: String::from("https://mint.example.com"),
            unit: String::from("sat"),
            memo: Some(String::from("original")),
            tokens: vec![],
        };
        let proofs = vec![
            Proof {
                amount: 2,
                keyset_id: String::from("00"),
                secret: String::from("secret1"),
                c: vec![0x02, 0xAB],
            },
            Proof {
                amount: 8,
                keyset_id: String::from("00"),
                secret: String::from("secret2"),
                c: vec![0x02, 0xCD],
            },
        ];

        let result = build_swap_token(&token, &proofs);

        assert_eq!(result.mint, "https://mint.example.com");
        assert_eq!(result.unit, "sat");
        assert_eq!(result.memo.as_deref(), Some("Swapped via Micronuts"));
        assert_eq!(result.tokens.len(), 1);
        assert_eq!(result.tokens[0].keyset_id, "00");
        assert_eq!(result.tokens[0].proofs.len(), 2);
        assert_eq!(result.tokens[0].proofs[0].amount, 2);
        assert_eq!(result.tokens[0].proofs[1].amount, 8);
    }

    #[test]
    fn test_build_swap_token_empty_proofs() {
        let token = TokenV4 {
            mint: String::from("https://mint.example.com"),
            unit: String::from("sat"),
            memo: None,
            tokens: vec![],
        };
        let proofs: Vec<Proof> = vec![];

        let result = build_swap_token(&token, &proofs);

        assert_eq!(result.tokens[0].keyset_id, "00");
        assert_eq!(result.tokens[0].proofs.len(), 0);
    }

    #[test]
    fn test_build_swap_token_uses_first_keyset_id() {
        let token = TokenV4 {
            mint: String::from("https://mint.example.com"),
            unit: String::from("sat"),
            memo: None,
            tokens: vec![],
        };
        let proofs = vec![
            Proof {
                amount: 1,
                keyset_id: String::from("aa"),
                secret: String::from("s"),
                c: vec![0x02],
            },
            Proof {
                amount: 2,
                keyset_id: String::from("bb"),
                secret: String::from("t"),
                c: vec![0x02],
            },
        ];

        let result = build_swap_token(&token, &proofs);
        assert_eq!(result.tokens[0].keyset_id, "aa");
    }
}
