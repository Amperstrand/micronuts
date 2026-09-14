//! micronuts-esp32-wallet — the nucula-mode device core (micronuts#68).
//!
//! M1 scaffold: the persistence keystone first. Every lesson from the
//! nucula C++ campaign (Amperstrand/nucula#1) is encoded here:
//!
//! - ONE NVS key, erase+write+commit in a single call — the atomicity
//!   contract `ProofStore` demands, proven by the mint's `StateStore`
//!   (PERSISTENCE-DESIGN #60) and violated by nothing since.
//! - A LOUD size bound checked before the write: refuse to run on
//!   unsavable state instead of silently degrading to a RAM-only
//!   wallet (the nucula#1 bug class).
//! - No tree serializers anywhere near the money path: the blob is
//!   opaque bytes produced by `cashu-core-lite`'s own encoding.

use cashu_core_lite::store::{ProofStore, StoreError};
pub mod wifi;

use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsPartitionId};

pub const WALLET_NAMESPACE: &str = "micronuts";
pub const WALLET_KEY: &str = "wallet_blob";

/// Atomic NVS-backed [`ProofStore`].
pub struct NvsProofStore<T: NvsPartitionId> {
    nvs: EspNvs<T>,
    key: &'static str,
    max_blob: usize,
}

impl<T: NvsPartitionId> NvsProofStore<T> {
    /// `max_blob` is the fail-stop bound: saves larger than this return
    /// `StoreError::Failed` loudly instead of silently not persisting.
    pub fn new(partition: EspNvsPartition<T>, key: &'static str, max_blob: usize) -> Result<Self, StoreError> {
        let nvs = EspNvs::new(partition, WALLET_NAMESPACE, true)
            .map_err(|_| StoreError::Unavailable)?;
        Ok(Self { nvs, key, max_blob })
    }

    pub fn stored_len(&self) -> Result<usize, StoreError> {
        Ok(self
            .nvs
            .blob_len(self.key)
            .map_err(|e| StoreError::Failed(format!("nvs len: {e:?}")))?
            .unwrap_or(0))
    }
}

impl<T: NvsPartitionId> ProofStore for NvsProofStore<T> {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        match self.nvs.blob_len(self.key) {
            Ok(None) => Ok(None),
            Ok(Some(len)) => {
                let mut buf = vec![0u8; len];
                self.nvs
                    .get_blob(self.key, &mut buf)
                    .map_err(|e| StoreError::Failed(format!("nvs read: {e:?}")))?;
                Ok(Some(buf))
            }
            Err(e) => Err(StoreError::Failed(format!("nvs stat: {e:?}"))),
        }
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        if blob.len() > self.max_blob {
            return Err(StoreError::Failed(format!(
                "blob {} B exceeds the {} B fail-stop bound — wallet refuses to run on unsavable state",
                blob.len(),
                self.max_blob
            )));
        }
        self.nvs
            .set_blob(self.key, blob)
            .map_err(|e| StoreError::Failed(format!("nvs write: {e:?}")))
    }
}
