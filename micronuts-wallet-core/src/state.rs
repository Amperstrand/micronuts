//! File-backed wallet persistence: the proof-store blob (atomic
//! temp-file + rename per the `ProofStore` contract) and the wallet.json
//! metadata file (mints, seed, history).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use cashu_core_lite::store::{ProofStore, StoreError};
use serde::{Deserialize, Serialize};

use crate::engine::{HistoryEntry, PendingSend};

/// Byte-blob proof store backed by one file.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| StoreError::Unavailable)?;
        }
        Ok(Self { path })
    }
}

impl ProofStore for FileStore {
    fn load(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(StoreError::Failed(String::from("proof store read failed"))),
        }
    }

    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        let tmp = self.path.with_extension("tmp");
        let write = || -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(blob)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, &self.path)
        };
        write().map_err(|_| StoreError::Failed(String::from("proof store write failed")))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MintEntry {
    pub url: String,
    pub name: String,
    pub trusted: bool,
}

/// wallet.json — everything except the proofs themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletState {
    #[serde(default)]
    pub mints: Vec<MintEntry>,
    #[serde(default)]
    pub active_mint: Option<String>,
    #[serde(default)]
    pub seed_hex: Option<String>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// In-flight ecash sends (UX contract: token ≠ payment). Persisted so
    /// claim-checking and reclaim survive restarts.
    #[serde(default)]
    pub pending_sends: Vec<PendingSend>,
}

impl WalletState {
    /// Missing file = fresh wallet. A present-but-corrupt file is an
    /// error (same stance as the mint's state file: refuse to boot).
    pub fn load(path: &Path) -> Result<Self, StoreError> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err(StoreError::Failed(String::from("wallet state read failed"))),
        };
        serde_json::from_str(&raw)
            .map_err(|_| StoreError::Failed(String::from("wallet state is corrupt")))
    }

    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        let raw = serde_json::to_string_pretty(self)
            .map_err(|_| StoreError::Failed(String::from("wallet state encode failed")))?;
        let tmp = path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(raw.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, path)
        };
        write().map_err(|_| StoreError::Failed(String::from("wallet state write failed")))
    }
}
