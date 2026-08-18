//! Persistence for wallet state: proofs plus the deterministic-derivation
//! counter (NUT-13).
//!
//! The store trait is deliberately byte-oriented — implementers persist an
//! opaque blob and own the physical medium (RAM, internal flash, NVS,
//! file). Serialization and framing live in [`crate::persistent`], so a
//! buggy medium shows up as a corrupt blob (treated as a fresh wallet and
//! re-restorable via NUT-09), never as silent proof corruption.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Persistence failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The medium is unavailable (not mounted, wrong mode, …).
    Unavailable,
    /// The medium rejected the write or returned an I/O error.
    Failed(String),
}

/// Byte-blob persistence for wallet state.
///
/// # Contract
/// - [`ProofStore::save`] must be atomic: on return, the stored blob is
///   either the complete previous one or the complete new one. A torn write
///   violates the crash-safety the wallet relies on (flash impls:
///   erase-then-write within one call; file impls: temp-file + rename).
/// - [`ProofStore::load`] returns `Ok(None)` when nothing was ever stored.
pub trait ProofStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError>;
}

/// Volatile in-memory store — tests, and production runs that rely purely
/// on NUT-09 restore across reboots.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    blob: Option<Vec<u8>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self { blob: None }
    }
}

impl ProofStore for MemoryStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.blob.clone())
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        self.blob = Some(blob.to_vec());
        Ok(())
    }
}
