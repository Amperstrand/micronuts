//! File-backed mint-state persistence (host prototype; see
//! docs/PERSISTENCE-DESIGN.md).
//!
//! Crash-consistency model: every mutation snapshots the whole state to
//! `path` via write-temp + fsync + atomic rename — a reader always sees
//! either the previous or the new snapshot, never a torn file. Snapshot
//! cost is O(state) per mutation, accepted at prototype scale.
//!
//! Boot semantics: a corrupt or unreadable state file REFUSES to start
//! (panic) — silently booting an empty store would resurrect spent
//! proofs and re-mint (the double-loss class the cashu-cf AGENTS
//! taxonomy warns about). Restoring from a backup is an operator
//! decision, never automatic.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Serialized form of one NUT-04 mint quote (fields mirror the in-memory
/// `MintQuoteEntry`).
#[derive(Serialize, Deserialize, Debug)]
pub struct MintQuoteSnap {
    pub amount: u64,
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
    pub amount_paid: u64,
    pub amount_issued: u64,
    pub updated_at: u64,
}

/// Serialized form of one NUT-05 melt quote.
#[derive(Serialize, Deserialize, Debug)]
pub struct MeltQuoteSnap {
    pub amount: u64,
    pub fee_reserve: u64,
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
}

/// Serialized NUT-12 DLEQ attachment of a blind signature.
#[derive(Serialize, Deserialize, Debug)]
pub struct DleqSnap {
    pub e_hex: String,
    pub s_hex: String,
}

/// Serialized form of a NUT-00 blind signature (`C_` as compressed-point
/// hex; curve points have no native JSON shape).
#[derive(Serialize, Deserialize, Debug)]
pub struct BlindSignatureSnap {
    pub amount: u64,
    pub id: String,
    pub c_hex: String,
    pub dleq: Option<DleqSnap>,
}

/// Whole-mint durable state. Quotes are stored as (id, entry) pairs so
/// restore does not depend on HashMap iteration order.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct MintStateSnapshot {
    pub mint_quotes: Vec<(String, MintQuoteSnap)>,
    pub melt_quotes: Vec<(String, MeltQuoteSnap)>,
    pub spent_ys: Vec<String>,
    pub issued_outputs: Vec<(String, BlindSignatureSnap)>,
}

/// Atomic-snapshot file store.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Load the snapshot, if a state file exists.
    ///
    /// Returns a descriptive error (instead of `None`) when the file
    /// exists but cannot be parsed — callers must treat that as a
    /// refuse-to-boot condition.
    pub fn load(&self) -> Result<Option<MintStateSnapshot>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path)
            .map_err(|e| format!("mint state {}: unreadable: {e}", self.path.display()))?;
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| format!("mint state {}: corrupt: {e}", self.path.display()))
    }

    /// Atomically replace the state file with `snap`.
    pub fn save(&self, snap: &MintStateSnapshot) -> Result<(), String> {
        let tmp = self.tmp_path();
        let json = serde_json::to_string(snap)
            .map_err(|e| format!("mint state {}: serialize failed: {e}", self.path.display()))?;
        // Write + fsync the temp file, then rename over the target —
        // rename(2) is atomic, so the target is never torn.
        (|| {
            let mut f = fs::File::create(&tmp)
                .map_err(|e| format!("mint state {}: create tmp failed: {e}", tmp.display()))?;
            f.write_all(json.as_bytes())
                .map_err(|e| format!("mint state {}: write failed: {e}", tmp.display()))?;
            f.sync_all()
                .map_err(|e| format!("mint state {}: fsync failed: {e}", tmp.display()))?;
            fs::rename(&tmp, &self.path).map_err(|e| {
                format!(
                    "mint state {}: atomic rename failed: {e}",
                    self.path.display()
                )
            })
        })()
        .inspect_err(|_| {
            // Best-effort cleanup of the orphaned temp file.
            let _ = fs::remove_file(&tmp);
        })
    }

    fn tmp_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".tmp");
        self.path.with_file_name(Path::new(&name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "micronuts-persist-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn load_missing_file_is_none() {
        let store = FileStore::new(temp_path("missing"));
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = temp_path("roundtrip");
        let store = FileStore::new(&path);
        let snap = MintStateSnapshot {
            spent_ys: vec!["ab".into()],
            ..Default::default()
        };
        store.save(&snap).unwrap();
        let loaded = store.load().unwrap().expect("snapshot present");
        assert_eq!(loaded.spent_ys, vec!["ab".to_string()]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn corrupt_file_is_an_error_not_none() {
        let path = temp_path("corrupt");
        std::fs::write(&path, b"{ not json").unwrap();
        let err = FileStore::new(&path).load().unwrap_err();
        assert!(err.contains("corrupt"), "unexpected error: {err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let path = temp_path("notmp");
        let store = FileStore::new(&path);
        store.save(&MintStateSnapshot::default()).unwrap();
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(path.file_name().unwrap().to_string_lossy().as_ref())
            })
            .collect();
        assert_eq!(entries.len(), 1, "only the snapshot file remains");
        std::fs::remove_file(&path).ok();
    }
}
