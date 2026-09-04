//! NVS-backed persistence for the device mint (#60 device leg, #56).
//!
//! Two blobs in the default NVS partition, namespace `micronuts`:
//! - `keyset_seed` — the 32-byte mint keyset seed, generated once on
//!   first boot (see main.rs); the served keyset re-derives from it.
//! - `mint_state` — whole-[`MintStateSnapshot`] JSON blob written
//!   through the [`StateStore`] seam (snapshot-per-mutation).
//!
//! Crash consistency: `EspNvs::set_blob` erases + rewrites the key and
//! runs `nvs_commit` before returning, so a reader sees either the
//! previous or the new snapshot, never a torn one. Same fail-stop
//! policy as the host file backend (persist.rs): a blob that exists but
//! cannot be decoded is an `Err`, never a silent empty store — that
//! would resurrect spent proofs and re-mint.

use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use micronuts_mint::persist::{MintStateSnapshot, StateStore};

/// NVS namespace for all micronuts keys (15-char NVS limit).
const NVS_NAMESPACE: &str = "micronuts";
/// Whole-snapshot JSON blob key.
const KEY_STATE: &str = "mint_state";
/// 32-byte keyset-seed blob key.
const KEY_SEED: &str = "keyset_seed";

/// Serialization bound for the snapshot blob, checked BEFORE writing:
/// exceeding it fails loudly (the mint's fail-stop panics on the error)
/// instead of wedging the finite NVS partition.
const MAX_STATE_BLOB: usize = 32 * 1024;

/// [`StateStore`] over the default NVS partition.
///
/// `EspNvs` is already `Send` (esp-idf-svc implements it — the ESP-IDF
/// NVS APIs are documented thread-safe), so the store satisfies the
/// `StateStore: Send` bound as-is.
pub struct NvsStateStore {
    nvs: EspNvs<NvsDefault>,
}

impl NvsStateStore {
    /// Open the `micronuts` namespace read-write.
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self, String> {
        let nvs = EspNvs::new(partition, NVS_NAMESPACE, true)
            .map_err(|e| format!("nvs: open namespace '{NVS_NAMESPACE}' failed: {e}"))?;
        Ok(Self { nvs })
    }

    /// The persisted keyset seed, or `None` on first boot.
    pub fn load_seed(&self) -> Result<Option<[u8; 32]>, String> {
        // Size first: a blob that is not exactly 32 bytes is corrupt and
        // must refuse to boot, not silently zero-pad through get_blob.
        let Some(len) = self
            .nvs
            .blob_len(KEY_SEED)
            .map_err(|e| format!("nvs: read '{KEY_SEED}' size failed: {e}"))?
        else {
            return Ok(None);
        };
        if len != 32 {
            return Err(format!(
                "nvs: '{KEY_SEED}' corrupt: expected 32 bytes, found {len}"
            ));
        }
        let mut seed = [0u8; 32];
        self.nvs
            .get_blob(KEY_SEED, &mut seed)
            .map_err(|e| format!("nvs: read '{KEY_SEED}' failed: {e}"))?;
        Ok(Some(seed))
    }

    /// Persist the keyset seed (first boot only).
    pub fn store_seed(&self, seed: &[u8; 32]) -> Result<(), String> {
        self.nvs
            .set_blob(KEY_SEED, seed)
            .map_err(|e| format!("nvs: write '{KEY_SEED}' failed: {e}"))
    }
}

impl StateStore for NvsStateStore {
    fn load(&self) -> Result<Option<MintStateSnapshot>, String> {
        let Some(len) = self
            .nvs
            .blob_len(KEY_STATE)
            .map_err(|e| format!("nvs: read '{KEY_STATE}' size failed: {e}"))?
        else {
            return Ok(None);
        };
        if len > MAX_STATE_BLOB {
            return Err(format!(
                "nvs: '{KEY_STATE}' {len} bytes exceeds NVS bound {MAX_STATE_BLOB}"
            ));
        }
        let mut buf = vec![0u8; len];
        let Some(bytes) = self
            .nvs
            .get_blob(KEY_STATE, &mut buf)
            .map_err(|e| format!("nvs: read '{KEY_STATE}' failed: {e}"))?
        else {
            // blob_len saw it, get_blob did not — inconsistent store.
            return Err(format!(
                "nvs: '{KEY_STATE}' vanished between size query and read"
            ));
        };
        serde_json::from_slice(bytes)
            .map(Some)
            .map_err(|e| format!("nvs: '{KEY_STATE}' corrupt: {e}"))
    }

    fn save(&self, snap: &MintStateSnapshot) -> Result<(), String> {
        let bytes = serde_json::to_vec(snap)
            .map_err(|e| format!("nvs: '{KEY_STATE}' serialize failed: {e}"))?;
        if bytes.len() > MAX_STATE_BLOB {
            return Err(format!(
                "nvs: '{KEY_STATE}' snapshot {} bytes exceeds NVS bound {MAX_STATE_BLOB} — refusing to write",
                bytes.len()
            ));
        }
        self.nvs
            .set_blob(KEY_STATE, &bytes)
            .map_err(|e| format!("nvs: write '{KEY_STATE}' failed: {e}"))
    }
}
