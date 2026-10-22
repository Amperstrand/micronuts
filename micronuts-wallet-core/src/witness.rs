//! Persistence witness (#102): the instrumentation whose absence let a
//! silent save-failure class run both ecosystem wallets as RAM-only
//! wallets (nucula fork issue #15; the parity matrix round-2 finding).
//! Counters are process-global and lock-free — the single-wallet device
//! console (`wstat`) reads them on demand.

// Width is target-dependent: the 32-bit Xtensa ESP32 target has no
// stable `AtomicU64` (no CAS for 8 bytes). The counters are diagnostics
// (the `wstat` console verb) — 32 bits cannot wrap in any device
// session, and host builds keep full width.
#[cfg(target_pointer_width = "64")]
use std::sync::atomic::{AtomicU64 as AtomicCount, Ordering};
#[cfg(not(target_pointer_width = "64"))]
use std::sync::atomic::{AtomicUsize as AtomicCount, Ordering};

/// Global store-layer witness. Updated by the storage implementation
/// (NVS on device, any `ProofStore` host-side); read by the console.
pub static STORE_WITNESS: Witness = Witness::new();

pub struct Witness {
    saves: AtomicCount,
    save_fails: AtomicCount,
    loads: AtomicCount,
    load_fails: AtomicCount,
    last_blob_len: AtomicCount,
    /// 1 = the last save succeeded, 0 = failed.
    last_save_ok: AtomicCount,
    /// Compact error fingerprint of the last failure (length of the
    /// message — enough to distinguish bound-exceeded from nvs-write
    /// classes without a string allocation in the hot path).
    last_err_len: AtomicCount,
    last_err: std::sync::Mutex<String>,
}

impl Default for Witness {
    fn default() -> Self {
        Self::new()
    }
}

impl Witness {
    pub const fn new() -> Self {
        Witness {
            saves: AtomicCount::new(0),
            save_fails: AtomicCount::new(0),
            loads: AtomicCount::new(0),
            load_fails: AtomicCount::new(0),
            last_blob_len: AtomicCount::new(0),
            last_save_ok: AtomicCount::new(0),
            last_err_len: AtomicCount::new(0),
            last_err: std::sync::Mutex::new(String::new()),
        }
    }

    pub fn save_ok(&self, blob_len: usize) {
        self.saves.fetch_add(1, Ordering::Relaxed);
        self.last_blob_len.store(blob_len as _, Ordering::Relaxed);
        self.last_save_ok.store(1, Ordering::Relaxed);
    }

    pub fn save_fail(&self, blob_len: usize, err: &str) {
        self.save_fails.fetch_add(1, Ordering::Relaxed);
        self.last_blob_len.store(blob_len as _, Ordering::Relaxed);
        self.last_save_ok.store(0, Ordering::Relaxed);
        self.last_err_len.store(err.len() as _, Ordering::Relaxed);
        if let Ok(mut slot) = self.last_err.lock() {
            slot.clear();
            slot.push_str(err);
        }
    }

    pub fn load_ok(&self) {
        self.loads.fetch_add(1, Ordering::Relaxed);
    }

    pub fn load_fail(&self, err: &str) {
        self.load_fails.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut slot) = self.last_err.lock() {
            slot.clear();
            slot.push_str(err);
        }
    }

    /// One-line `wstat`-style report, fork-compatible in spirit:
    /// `saves=N fails=M last=ok|FAIL err=<msg> blob=B loads=L load_fails=K`
    pub fn report(&self) -> String {
        let last = if self.last_save_ok.load(Ordering::Relaxed) == 1 {
            "ok"
        } else {
            "FAIL"
        };
        let err = self.last_err.lock().map(|s| s.clone()).unwrap_or_default();
        format!(
            "saves={} fails={} last={} err={} blob={} loads={} load_fails={}",
            self.saves.load(Ordering::Relaxed),
            self.save_fails.load(Ordering::Relaxed),
            last,
            if err.is_empty() { "-" } else { &err },
            self.last_blob_len.load(Ordering::Relaxed),
            self.loads.load(Ordering::Relaxed),
            self.load_fails.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_reflects_counters() {
        let w = Witness::new();
        assert!(w.report().contains("saves=0 fails=0 last=FAIL"));
        w.save_ok(409);
        w.save_ok(512);
        assert!(w
            .report()
            .contains("saves=2 fails=0 last=ok err=- blob=512"));
        w.save_fail(600, "nvs write: no space");
        assert!(w.report().contains("fails=1 last=FAIL err=nvs write"));
    }

    #[test]
    fn load_counters_track_failures() {
        let w = Witness::new();
        w.load_ok();
        w.load_fail("nvs stat: busy");
        assert!(w.report().contains("loads=1 load_fails=1"));
    }
}
