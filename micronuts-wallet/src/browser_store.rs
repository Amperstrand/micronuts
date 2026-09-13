//! Browser persistence (wasm): the proof envelope and wallet metadata
//! in `localStorage`, so real-mint ecash and the seed survive reloads.
//!
//! Without this, the browser wallet generated a fresh seed per boot —
//! closing the tab destroyed the ecash issued to the old deterministic
//! secrets (a money-destroyer once real mints arrived). Layout:
//! - `micronuts:wallet`   → WalletState JSON (mints, seed, history,
//!   pending sends)
//! - `micronuts:proofs:<host-tag>` → hex-encoded per-mint envelope
//!   (the same atomicity contract as FileStore: a write is a full
//!   replace, so a torn write loses the latest change, never corrupts
//!   the store).

use cashu_core_lite::store::{ProofStore, StoreError};

use crate::state::WalletState;

const WALLET_KEY: &str = "micronuts:wallet";
const PROOF_KEY_PREFIX: &str = "micronuts:proofs:";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// localStorage-backed proof envelope, one key per mint host.
#[derive(Clone)]
pub struct BrowserStore {
    key: String,
}

impl BrowserStore {
    /// Key the store to the mint URL (same tag scheme as the file store).
    pub fn for_mint(url: &str) -> Self {
        let tag = url.trim_end_matches('/').replace(['/', ':'], "_");
        Self {
            key: format!("{PROOF_KEY_PREFIX}{tag}"),
        }
    }
}

impl ProofStore for BrowserStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(storage) = storage() else {
            return Err(StoreError::Unavailable);
        };
        let raw = storage
            .get_item(&self.key)
            .map_err(|_| StoreError::Failed(String::from("localStorage read failed")))?;
        match raw {
            None => Ok(None),
            Some(hex) => {
                let bytes = hex::decode(hex.trim())
                    .map_err(|_| StoreError::Failed(String::from("proof store corrupt")))?;
                Ok(Some(bytes))
            }
        }
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        let Some(storage) = storage() else {
            return Err(StoreError::Unavailable);
        };
        let hex = hex::encode(blob);
        storage
            .set_item(&self.key, &hex)
            .map_err(|_| StoreError::Failed(String::from("localStorage quota exceeded")))
    }
}

/// Load the persisted wallet state; `None` = fresh wallet (first run, or
/// storage cleared).
pub fn load_wallet_state() -> Option<WalletState> {
    let storage = storage()?;
    let raw = storage.get_item(WALLET_KEY).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

/// Persist the wallet state (full replace).
pub fn save_wallet_state(state: &WalletState) -> Result<(), StoreError> {
    let Some(storage) = storage() else {
        return Err(StoreError::Unavailable);
    };
    let raw = serde_json::to_string(state)
        .map_err(|_| StoreError::Failed(String::from("state encode failed")))?;
    storage
        .set_item(WALLET_KEY, &raw)
        .map_err(|_| StoreError::Failed(String::from("localStorage write failed")))
}
