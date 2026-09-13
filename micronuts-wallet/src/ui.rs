//! Slint UI wiring: every `WalletLogic` callback posts a job to the worker
//! that owns the engine + wallet state. Native runs the worker on a thread
//! (results flow back via `Weak::upgrade_in_event_loop`); the browser
//! build executes jobs synchronously on the JS event loop with an
//! embedded demo mint. A snapshot of generic wallet state is re-applied
//! after every job.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rand_core::{OsRng, RngCore};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

use crate::engine::{HistoryEntry, HistoryKind, PendingStatus, WalletEngine};
use crate::flow;
use crate::state::{MintEntry, WalletState};
#[cfg(not(target_arch = "wasm32"))]
use cashu_core_lite::transport::MintClient;

#[cfg(target_arch = "wasm32")]
use crate::demo_mint::DemoMintClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::http::HttpMintClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::state::FileStore;
#[cfg(not(target_arch = "wasm32"))]
use cashu_core_lite::store::StoreError;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;

slint::include_modules!();

const QR_SCALE: usize = 6;
const QR_QUIET_ZONE: usize = 4;

#[cfg(not(target_arch = "wasm32"))]
type Transport = HttpMintClient;
#[cfg(not(target_arch = "wasm32"))]
type Store = FileStore;
#[cfg(target_arch = "wasm32")]
type Transport = ClientForMint;
#[cfg(target_arch = "wasm32")]
type Store = crate::browser_store::BrowserStore;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce(&mut Worker) + Send + 'static>;
#[cfg(target_arch = "wasm32")]
type Job = Box<dyn FnOnce(&mut Worker) + 'static>;

/// The wasm engine's transport: the in-process demo mint or a real mint
/// over XHR — one concrete type so `Worker.engine` stays uniform.
#[cfg(target_arch = "wasm32")]
mod wasm_mint_types {
    pub use cashu_core_lite::error::CashuError;
    pub use cashu_core_lite::nuts::{nut02, nut04, nut05, nut06, nut07, nut09};
}

#[cfg(target_arch = "wasm32")]
use wasm_mint_types::{nut02, nut04, nut05, nut06, nut07, nut09, CashuError};

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub(crate) enum ClientForMint {
    Demo(crate::demo_mint::DemoMintClient),
    Fetch(crate::wasm_http::FetchMintClient),
}

#[cfg(target_arch = "wasm32")]
impl cashu_core_lite::transport::MintClient for ClientForMint {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.get_info(),
            ClientForMint::Fetch(c) => c.get_info(),
        }
    }
    fn get_keys(&mut self) -> Result<cashu_core_lite::nuts::nut01::KeysResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.get_keys(),
            ClientForMint::Fetch(c) => c.get_keys(),
        }
    }
    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.get_keysets(),
            ClientForMint::Fetch(c) => c.get_keysets(),
        }
    }
    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_mint_quote(request),
            ClientForMint::Fetch(c) => c.post_mint_quote(request),
        }
    }
    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.get_mint_quote(quote_id),
            ClientForMint::Fetch(c) => c.get_mint_quote(quote_id),
        }
    }
    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_mint(request),
            ClientForMint::Fetch(c) => c.post_mint(request),
        }
    }
    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_melt_quote(request),
            ClientForMint::Fetch(c) => c.post_melt_quote(request),
        }
    }
    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.get_melt_quote(quote_id),
            ClientForMint::Fetch(c) => c.get_melt_quote(quote_id),
        }
    }
    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_melt(request),
            ClientForMint::Fetch(c) => c.post_melt(request),
        }
    }
    fn post_swap(
        &mut self,
        request: cashu_core_lite::nuts::nut03::SwapRequest,
    ) -> Result<cashu_core_lite::nuts::nut03::SwapResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_swap(request),
            ClientForMint::Fetch(c) => c.post_swap(request),
        }
    }
    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_check_state(request),
            ClientForMint::Fetch(c) => c.post_check_state(request),
        }
    }
    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        match self {
            ClientForMint::Demo(c) => c.post_restore(request),
            ClientForMint::Fetch(c) => c.post_restore(request),
        }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WORKER: std::cell::RefCell<Option<Worker>> =
        const { std::cell::RefCell::new(None) };
    static MIRROR_TIMER: std::cell::RefCell<Option<slint::Timer>> =
        const { std::cell::RefCell::new(None) };
    static DEMO_MINT_SEED: std::cell::RefCell<[u8; 32]> =
        const { std::cell::RefCell::new([0u8; 32]) };
    static DISPATCHER: std::cell::RefCell<Option<Dispatcher>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Clone)]
pub struct Dispatcher {
    #[cfg(not(target_arch = "wasm32"))]
    tx: mpsc::Sender<Job>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) weak: Weak<MainWindow>,
}

impl Dispatcher {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_weak(weak: Weak<MainWindow>) -> Self {
        Self { weak }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn post(&self, job: impl FnOnce(&mut Worker) + Send + 'static) {
        let _ = self.tx.send(Box::new(job));
    }

    #[cfg(target_arch = "wasm32")]
    fn post(&self, job: impl FnOnce(&mut Worker) + 'static) {
        let weak = self.weak.clone();
        WORKER.with(|slot| {
            if let Some(worker) = slot.borrow_mut().as_mut() {
                job(worker);
                finish_job(worker, &weak);
            }
        });
    }
}

pub fn run(dir: PathBuf) -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;

    #[cfg(not(target_arch = "wasm32"))]
    {
        let worker = Worker::new(&dir).expect("wallet data directory");
        let (tx, rx) = mpsc::channel::<Job>();
        let weak = ui.as_weak();
        std::thread::spawn(move || worker_loop(worker, rx, weak));

        let dispatcher = Dispatcher { tx };
        wire_callbacks(&ui, dispatcher.clone());
        start_quote_poller(&ui, dispatcher.clone());
        dispatcher.post(|worker| worker.connect_active());
    }

    #[cfg(target_arch = "wasm32")]
    {
        let worker = Worker::new(&dir).expect("browser worker");
        WORKER.with(|slot| *slot.borrow_mut() = Some(worker));
        let dispatcher = Dispatcher { weak: ui.as_weak() };
        wire_callbacks(&ui, dispatcher.clone());
        start_quote_poller(&ui, dispatcher.clone());
        dispatcher.post(|worker| worker.connect_active());

        // Keep the e2e/tooling mirror fresh between snapshots (page
        // changes happen without a worker round-trip).
        let weak = ui.as_weak();
        let mirror_timer = slint::Timer::default();
        mirror_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(250),
            move || {
                if let Some(ui) = weak.upgrade() {
                    mirror_state_to_window(&ui.global::<WalletLogic>());
                }
            },
        );
        MIRROR_TIMER.with(|slot| *slot.borrow_mut() = Some(mirror_timer));

        install_action_api(ui.as_weak(), dispatcher.clone());
    }

    ui.run()
}

/// Semantic action API for browser e2e: `window.__micronutsAct(name, arg)`
/// invokes the exact `WalletLogic` callbacks the on-screen buttons fire —
/// the flows run through the real async pipeline and state machine with
/// zero coordinate coupling. Slint-on-wasm has no DOM/a11y surface and
/// layout-computed geometry (`absolute-position`) does not propagate into
/// the binding graph, so coordinate clicking was the only alternative —
/// this replaces it for money-flow tests (pointer plumbing stays covered
/// by the nav/boot pointer tests).
#[cfg(target_arch = "wasm32")]
fn install_action_api(weak: Weak<MainWindow>, dispatcher: Dispatcher) {
    DISPATCHER.with(|slot| *slot.borrow_mut() = Some(dispatcher));
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let closure = move |name: String, arg: String| {
        let Some(ui) = weak.clone().upgrade() else {
            return js_sys::JsString::from("error: ui gone");
        };
        let logic = ui.global::<WalletLogic>();
        match name.as_str() {
            "navigate" => {
                let page = match arg.as_str() {
                    "home" => Some(Page::Home),
                    "receive" => Some(Page::Receive),
                    "send" => Some(Page::Send),
                    "scan" => Some(Page::Scan),
                    "activity" => Some(Page::Activity),
                    "settings" => Some(Page::Settings),
                    "mints" => Some(Page::Mints),
                    "backup" => Some(Page::Backup),
                    _ => None,
                };
                match page {
                    Some(page) => {
                        ui.invoke_navigate(page);
                        js_sys::JsString::from("ok")
                    }
                    None => js_sys::JsString::from("error: unknown page"),
                }
            }
            "mint-invoice" => {
                logic.invoke_mint_invoice(arg.into());
                js_sys::JsString::from("ok")
            }
            "token-edited" => {
                logic.set_token_in(arg.clone().into());
                logic.invoke_token_edited(arg.into());
                js_sys::JsString::from("ok")
            }
            "receive-token" => {
                logic.invoke_receive_token(arg.into());
                js_sys::JsString::from("ok")
            }
            "send-token" => {
                // arg = "<amount> <memo>" (memo optional)
                let mut parts = arg.splitn(2, ' ');
                let amount = parts.next().unwrap_or("").to_string();
                let memo = parts.next().unwrap_or("").to_string();
                logic.invoke_send_token(amount.into(), memo.into());
                js_sys::JsString::from("ok")
            }
            "invoice-edited" => {
                logic.invoke_invoice_edited(arg.into());
                js_sys::JsString::from("ok")
            }
            "melt-confirm" => {
                logic.invoke_melt_confirm();
                js_sys::JsString::from("ok")
            }
            "add-mint" => {
                if let Some(tx) = DISPATCHER.with(|d| d.borrow().clone()) {
                    tx.post(move |worker| {
                        worker_add_mint(worker, arg);
                    });
                }
                js_sys::JsString::from("ok")
            }
            "switch-mint" => {
                let index = arg.parse::<i32>().unwrap_or(-1);
                if let Some(tx) = DISPATCHER.with(|d| d.borrow().clone()) {
                    tx.post(move |worker| {
                        worker_switch_mint(worker, index);
                    });
                }
                js_sys::JsString::from("ok")
            }
            "snapshot" => {
                mirror_state_to_window(&logic);
                js_sys::JsString::from("ok")
            }
            other => js_sys::JsString::from(format!("error: unknown action {other}").as_str()),
        }
    };
    let closure =
        Closure::wrap(Box::new(closure) as Box<dyn FnMut(String, String) -> js_sys::JsString>);
    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = js_sys::Reflect::set(
        window.as_ref(),
        &wasm_bindgen::JsValue::from_str("__micronutsAct"),
        closure.as_ref().unchecked_ref::<js_sys::Function>(),
    );
    closure.forget();
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_loop(mut worker: Worker, rx: mpsc::Receiver<Job>, weak: Weak<MainWindow>) {
    while let Ok(job) = rx.recv() {
        job(&mut worker);
        finish_job(&mut worker, &weak);
    }
}

fn finish_job(worker: &mut Worker, weak: &Weak<MainWindow>) {
    worker.persist_state();
    let snapshot = worker.snapshot();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        apply_snapshot(&ui, &snapshot);
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn store_error_text(err: StoreError) -> String {
    match err {
        StoreError::Unavailable => String::from("store unavailable"),
        StoreError::Failed(detail) => format!("store failure: {detail}"),
    }
}

struct Worker {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    dir: PathBuf,
    state: WalletState,
    engine: Option<WalletEngine<Transport, Store>>,
    pending_melt: Option<String>,
    status: String,
}

#[derive(Clone)]
struct Snapshot {
    connected: bool,
    has_active_mint: bool,
    balance_text: String,
    mint_name: String,
    mint_url: String,
    status: String,
    seed: String,
    history: Vec<HistoryItem>,
    mints: Vec<MintItem>,
}

impl Worker {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create data dir: {e}"))?;
        let state_path = dir.join("wallet.json");
        let mut state = WalletState::load(&state_path).map_err(store_error_text)?;
        if state.seed_hex.is_none() {
            let mut seed = [0u8; 32];
            OsRng.fill_bytes(&mut seed);
            state.seed_hex = Some(hex::encode(seed));
            state.save(&state_path).map_err(store_error_text)?;
        }
        let mut worker = Self {
            dir: dir.to_path_buf(),
            state,
            engine: None,
            pending_melt: None,
            status: String::new(),
        };
        worker.build_engine();
        Ok(worker)
    }

    #[cfg(target_arch = "wasm32")]
    fn build_engine(&mut self) {
        self.engine = None;
        let Some(active) = self.state.active_mint.clone() else {
            return;
        };
        let client = if active == crate::demo_mint::DEMO_MINT_URL {
            ClientForMint::Demo(crate::demo_mint::DemoMintClient::new(self.demo_mint_seed()))
        } else {
            ClientForMint::Fetch(crate::wasm_http::FetchMintClient::new(&active))
        };
        let engine = WalletEngine::with_pending(
            &active,
            client,
            crate::browser_store::BrowserStore::for_mint(&active),
            self.seed(),
            self.state.history.clone(),
            self.state.pending_sends.clone(),
        );
        match engine {
            Ok(engine) => self.engine = Some(engine),
            Err(err) => self.status = format!("wallet init failed: {err}"),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn demo_mint_seed(&self) -> [u8; 32] {
        // The demo mint instance must live as long as the engine: keep a
        // per-worker seed so rebuilds share the mint's keyset.
        DEMO_MINT_SEED.with(|slot| *slot.borrow())
    }

    #[cfg(target_arch = "wasm32")]
    fn new(_dir: &Path) -> Result<Self, String> {
        // Persisted wallet (real mints + seed survive reloads); a fresh
        // demo wallet is only created when nothing is stored yet.
        if let Some(mut state) = crate::browser_store::load_wallet_state() {
            if let Some(seed_hex) = state.seed_hex.clone() {
                if let Ok(bytes) = hex::decode(&seed_hex) {
                    if bytes.len() == 32 {
                        let mut seed = [0u8; 32];
                        seed.copy_from_slice(&bytes);
                        let mut worker = Self {
                            dir: PathBuf::new(),
                            state,
                            engine: None,
                            pending_melt: None,
                            status: String::from("restored wallet"),
                        };
                        worker.build_engine();
                        return Ok(worker);
                    }
                }
            }
            // Corrupt/seedless state: fall through to a fresh wallet.
            state.mints.clear();
        }
        let mut wallet_seed = [0u8; 32];
        OsRng.fill_bytes(&mut wallet_seed);
        let mut mint_seed = [0u8; 32];
        OsRng.fill_bytes(&mut mint_seed);
        let mut state = WalletState::default();
        state.mints.push(MintEntry {
            url: String::from(crate::demo_mint::DEMO_MINT_URL),
            name: String::from("Browser Demo Mint"),
            trusted: true,
        });
        state.active_mint = Some(String::from(crate::demo_mint::DEMO_MINT_URL));
        state.seed_hex = Some(hex::encode(wallet_seed));
        DEMO_MINT_SEED.with(|slot| *slot.borrow_mut() = mint_seed);
        let engine = WalletEngine::with_pending(
            crate::demo_mint::DEMO_MINT_URL,
            ClientForMint::Demo(DemoMintClient::new(mint_seed)),
            crate::browser_store::BrowserStore::for_mint(crate::demo_mint::DEMO_MINT_URL),
            wallet_seed,
            Vec::new(),
            state.pending_sends.clone(),
        )
        .map_err(|e| format!("browser engine: {e}"))?;
        Ok(Self {
            dir: PathBuf::new(),
            state,
            engine: Some(engine),
            pending_melt: None,
            status: String::from("browser wallet — demo mint; add a real mint in Settings"),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn state_path(&self) -> PathBuf {
        self.dir.join("wallet.json")
    }

    fn seed(&self) -> [u8; 32] {
        let mut seed = [0u8; 32];
        if let Some(seed_hex) = &self.state.seed_hex {
            if let Ok(bytes) = hex::decode(seed_hex) {
                if bytes.len() == 32 {
                    seed.copy_from_slice(&bytes);
                }
            }
        }
        seed
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn store_path(&self, url: &str) -> PathBuf {
        let tag = url.trim_end_matches('/').replace(['/', ':'], "_");
        self.dir.join(format!("proofs-{tag}.bin"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_engine(&mut self) {
        self.engine = None;
        let Some(active) = self.state.active_mint.clone() else {
            return;
        };
        let Ok(store) = FileStore::new(self.store_path(&active)) else {
            self.status = format!("cannot open proof store for {active}");
            return;
        };
        let client = HttpMintClient::new(&active);
        match WalletEngine::with_pending(
            &active,
            client,
            store,
            self.seed(),
            self.state.history.clone(),
            self.state.pending_sends.clone(),
        ) {
            Ok(engine) => self.engine = Some(engine),
            Err(err) => self.status = format!("wallet init failed: {err}"),
        }
    }

    fn connect_active(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            self.status = String::from("add a mint first");
            return;
        };
        match engine.connect() {
            Ok(()) => self.status = String::from("connected"),
            Err(err) => self.status = format!("mint unreachable: {err}"),
        }
        // NUT-07 reconciliation on connect (non-fatal): drop proofs the
        // mint already reports spent so the balance is honest from boot.
        if let Some(engine) = self.engine.as_mut() {
            match engine.reconcile_spent() {
                Ok(0) => {}
                Ok(pruned) => self.status = format!("pruned {pruned} spent proofs"),
                Err(_) => {}
            }
            // Lifecycle check of in-flight sends (non-fatal): finalize
            // any the recipient already claimed.
            if let Ok(statuses) = engine.check_all_pending() {
                let claimed = statuses
                    .iter()
                    .filter(|s| **s == PendingStatus::Claimed)
                    .count();
                if claimed > 0 {
                    self.status = format!(
                        "{claimed} send{} claimed",
                        if claimed == 1 { "" } else { "s" }
                    );
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_state(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            self.state.history = engine.history().to_vec();
            self.state.pending_sends = engine.pending_sends().to_vec();
        }
        if let Err(err) = self.state.save(&self.state_path()) {
            self.status = format!("state save failed: {}", store_error_text(err));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn persist_state(&mut self) {
        let (history, pending) = self
            .engine
            .as_ref()
            .map(|engine| (engine.history().to_vec(), engine.pending_sends().to_vec()))
            .unwrap_or_default();
        self.state.history = history;
        self.state.pending_sends = pending;
        if let Err(err) = crate::browser_store::save_wallet_state(&self.state) {
            self.status = format!("browser persistence failed: {err:?}");
        }
    }

    fn snapshot(&self) -> Snapshot {
        let (connected, balance_text, mint_name, mint_url, history) = match self.engine.as_ref() {
            Some(engine) => (
                !engine.keyset_id().is_empty(),
                crate::format_amount(engine.balance()),
                engine.mint_name().to_string(),
                engine.mint_url().to_string(),
                engine
                    .history()
                    .iter()
                    .rev()
                    .map(|entry| HistoryItem {
                        line1: history_line1(entry).into(),
                        line2: history_line2(entry).into(),
                    })
                    .collect::<Vec<_>>(),
            ),
            None => (
                false,
                crate::format_amount(0),
                String::new(),
                String::new(),
                Vec::new(),
            ),
        };
        let mints = self
            .state
            .mints
            .iter()
            .map(|mint| MintItem {
                url: mint.url.clone().into(),
                name: mint.name.clone().into(),
                active: Some(&mint.url) == self.state.active_mint.as_ref(),
            })
            .collect::<Vec<_>>();
        Snapshot {
            connected,
            has_active_mint: self.state.active_mint.is_some(),
            balance_text,
            mint_name,
            mint_url,
            status: self.status.clone(),
            seed: self.seed_hex(),
            history,
            mints,
        }
    }

    fn seed_hex(&self) -> String {
        self.state.seed_hex.clone().unwrap_or_default()
    }
}

/// Kind-first rail labels per the copy contract (cashubtc/wallet
/// display-title convention: "Ecash received", "Lightning received"…).
fn history_line1(entry: &HistoryEntry) -> String {
    use crate::engine::HistoryStatus;
    let amount = crate::format_amount(entry.amount);
    let base = match entry.kind {
        HistoryKind::Mint => format!("↓ {amount} · Lightning received"),
        HistoryKind::Send => format!("↑ {amount} · Ecash sent"),
        HistoryKind::Receive => format!("↓ {amount} · Ecash received"),
        HistoryKind::Melt => format!("↑ {amount} · Lightning paid"),
    };
    match entry.status {
        HistoryStatus::Complete | HistoryStatus::Reclaimed => base,
        HistoryStatus::Pending => format!("{base} — waiting for recipient"),
    }
}

/// Secondary line: time (Today HH:MM / Mon DD) then the stored detail.
fn history_line2(entry: &HistoryEntry) -> String {
    use crate::engine::HistoryStatus;
    let time = format_ts(entry.ts_secs);
    match entry.status {
        HistoryStatus::Reclaimed => format!("{time} · taken back"),
        _ => format!("{time} · {}", entry.detail),
    }
}

/// Local-time "Today 14:32" / "Sep 12" (web_time keeps this working in
/// the browser build).
fn format_ts(ts_secs: u64) -> String {
    use web_time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs_of_day = |ts: u64| ts % 86_400;
    let (year, month, day, hour, minute) = civil_from_unix(ts_secs as i64);
    if now.saturating_sub(ts_secs) < 86_400 && secs_of_day(now) >= secs_of_day(ts_secs) {
        format!("Today {hour:02}:{minute:02}")
    } else if now.saturating_sub(ts_secs) < 172_800 {
        format!("Yesterday {hour:02}:{minute:02}")
    } else {
        let month_name = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ][(month.clamp(1, 12) - 1) as usize];
        format!("{month_name} {day}, {year}")
    }
}

/// Days-to-civil (Howard Hinnant's algorithm, no std time plumbing).
fn civil_from_unix(ts: i64) -> (i64, u32, u32, u32, u32) {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        m,
        d,
        (secs / 3600) as u32,
        ((secs % 3600) / 60) as u32,
    )
}

fn apply_snapshot(ui: &MainWindow, snapshot: &Snapshot) {
    let logic = ui.global::<WalletLogic>();
    logic.set_busy(false);
    logic.set_connected(snapshot.connected);
    logic.set_has_active_mint(snapshot.has_active_mint);
    logic.set_balance_text(snapshot.balance_text.clone().into());
    logic.set_mint_name(snapshot.mint_name.clone().into());
    logic.set_mint_url(snapshot.mint_url.clone().into());
    logic.set_status_message(snapshot.status.clone().into());
    logic.set_seed(snapshot.seed.clone().into());
    logic.set_history(ModelRc::new(VecModel::from(snapshot.history.clone())));
    logic.set_mints(ModelRc::new(VecModel::from(snapshot.mints.clone())));
    #[cfg(target_arch = "wasm32")]
    mirror_state_to_window(&logic);
}

/// Test/tooling seam for the wasm build: Slint renders to a canvas, so
/// browser e2e (Playwright) has no DOM to assert on. Mirror the semantic
/// wallet state onto `window.__micronuts` — the same pattern the gm65
/// playground uses. Contains no secrets: page, balance text, mint name,
/// connection flags, history length.
#[cfg(target_arch = "wasm32")]
fn mirror_state_to_window(logic: &WalletLogic) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let obj = js_sys::Object::new();
    let set = |key: &str, value: wasm_bindgen::JsValue| {
        let _ = js_sys::Reflect::set(obj.as_ref(), &wasm_bindgen::JsValue::from_str(key), &value);
    };
    set("page", logic.get_page_name().to_string().into());
    set("balance", logic.get_balance_text().to_string().into());
    set("mint", logic.get_mint_name().to_string().into());
    set("connected", logic.get_connected().into());
    set("hasActiveMint", logic.get_has_active_mint().into());
    set(
        "receiveState",
        logic.get_receive_ecash_state().to_string().into(),
    );
    set(
        "receiveLine",
        logic.get_receive_review_line().to_string().into(),
    );
    set("sendState", logic.get_send_ecash_state().to_string().into());
    set("pendingSends", logic.get_pending_send_count().into());
    set("tokenOut", logic.get_token_out().to_string().into());
    set("invoiceState", logic.get_invoice_state().to_string().into());
    set("meltPreimage", logic.get_melt_preimage().to_string().into());
    set(
        "meltQuoteInfo",
        logic.get_melt_quote_info().to_string().into(),
    );
    set(
        "historyLen",
        (slint::Model::row_count(&logic.get_history()) as u32).into(),
    );
    let _ = js_sys::Reflect::set(
        window.as_ref(),
        &wasm_bindgen::JsValue::from_str("__micronuts"),
        obj.as_ref(),
    );
}

fn parse_amount(text: &str) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid amount: {text}"))
}

fn worker_add_mint(worker: &mut Worker, url: String) {
    if url.is_empty() {
        return;
    }
    if worker.state.mints.iter().any(|m| m.url == url) {
        worker.status = format!("mint already known: {url}");
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let info = HttpMintClient::new(&url).get_info();
    #[cfg(target_arch = "wasm32")]
    let info = (|| {
        if url == crate::demo_mint::DEMO_MINT_URL {
            return Err(cashu_core_lite::error::CashuError::Protocol(String::from(
                "demo mint already active",
            )));
        }
        let mut probe = crate::wasm_http::FetchMintClient::new(&url);
        cashu_core_lite::transport::MintClient::get_info(&mut probe)
    })();
    match info {
        Ok(info) => {
            worker.state.mints.push(MintEntry {
                url: url.clone(),
                name: info.name,
                trusted: true,
            });
            worker.state.active_mint = Some(url);
            worker.build_engine();
            worker.connect_active();
        }
        Err(err) => worker.status = format!("mint unreachable: {err}"),
    }
}

fn worker_switch_mint(worker: &mut Worker, index: i32) {
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let Some(entry) = worker.state.mints.get(index) else {
        return;
    };
    let url = entry.url.clone();
    if worker.state.active_mint.as_deref() == Some(&url) {
        return;
    }
    worker.state.active_mint = Some(url);
    worker.build_engine();
    worker.connect_active();
}

fn wire_callbacks(ui: &MainWindow, dispatcher: Dispatcher) {
    let logic = ui.global::<WalletLogic>();
    let weak = ui.as_weak();

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_connect(move || {
            set_busy(&weak);
            tx.post(|worker| worker.connect_active());
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_mint_invoice(move |amount_text: slint::SharedString| {
            let weak = weak.clone();
            set_busy(&weak);
            let amount = match parse_amount(&amount_text) {
                Ok(amount) => amount,
                Err(err) => return report(&weak, err),
            };
            tx.post(move |worker| {
                let Some(engine) = worker.engine.as_mut() else {
                    worker.status = String::from("add a mint first");
                    return;
                };
                match engine.mint_via_invoice(amount) {
                    Ok(quote) => {
                        worker.status = format!("invoice created (quote {})", quote.quote);
                        let quote_id = quote.quote.clone();
                        let request = quote.request.clone();
                        let state = quote.state.clone();
                        let already_paid = state == "PAID";
                        let weak_clone = weak.clone();
                        let quote_id_for_mint = quote_id.clone();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_invoice(request.clone().into());
                            logic.set_invoice_quote_id(quote_id.clone().into());
                            logic.set_invoice_state(state.clone().into());
                            logic.set_invoice_status_text(
                                flow::ReceiveLightningPhase::from_quote_state(&state, amount)
                                    .user_line()
                                    .into(),
                            );
                            logic.set_invoice_amount(amount as i32);
                            logic.set_invoice_amount_text(crate::format_amount(amount).into());
                            if let Some(image) = qr_image(logic.get_invoice().as_str()) {
                                logic.set_invoice_qr(image);
                            }
                        });
                        // Fast-settling mints answer PAID at creation —
                        // the poller would skip (it only watches
                        // transitions), so issue from here too.
                        if already_paid {
                            drop_mint_paid(worker, quote_id_for_mint, amount, weak_clone);
                        }
                    }
                    Err(err) => worker.status = format!("quote failed: {err}"),
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_mint_paid(move |quote_id: slint::SharedString, amount: i32| {
            let weak = weak.clone();
            set_busy(&weak);
            let quote_id = quote_id.to_string();
            let amount = u64::try_from(amount).unwrap_or(0);
            tx.post(move |worker| {
                drop_mint_paid(worker, quote_id, amount, weak);
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_receive_token(move |token: slint::SharedString| {
            let weak = weak.clone();
            set_busy(&weak);
            let _ = weak.upgrade_in_event_loop(|ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_receive_ecash_state(String::from("receiving").into());
                logic.set_receive_review_fee(String::new().into());
            });
            let token = token.to_string();
            tx.post(move |worker| {
                let Some(engine) = worker.engine.as_mut() else {
                    worker.status = String::from("add a mint first");
                    return;
                };
                match engine.receive_token(&token) {
                    Ok(received) => {
                        worker.status = format!("received {received} sat");
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_token_in(String::new().into());
                            logic.set_receive_ecash_state(String::from("received").into());
                            logic.set_receive_review_line(
                                flow::ReceiveEcashPhase::Received { amount: received }
                                    .user_line()
                                    .into(),
                            );
                        });
                    }
                    Err(err) => {
                        worker.status = format!("receive failed: {err}");
                        let line = flow::classify(&err).user_line();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_receive_ecash_state(String::from("failed").into());
                            logic.set_receive_review_line(line.into());
                        });
                    }
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        // Auto-inspect: input arriving (typing, paste, scan) drives the
        // review machinery by itself — "Check" is not a user verb.
        logic.on_token_edited(move |token: slint::SharedString| {
            let token = token.to_string();
            if token.trim().is_empty() {
                let _ = weak.upgrade_in_event_loop(|ui| {
                    let logic = ui.global::<WalletLogic>();
                    logic.set_receive_ecash_state(String::from("input").into());
                    logic.set_receive_review_line(String::new().into());
                    logic.set_receive_review_fee(String::new().into());
                });
                return;
            }
            if !matches!(
                crate::payload::classify(&token),
                crate::payload::ScannedPayload::CashuToken { .. }
            ) {
                // Not a token (yet, while typing) — no mint chatter.
                return;
            }
            inspect_for_review(&weak, &tx, token);
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_send_token(
            move |amount_text: slint::SharedString, memo: slint::SharedString| {
                let weak = weak.clone();
                set_busy(&weak);
                let amount = match parse_amount(&amount_text) {
                    Ok(amount) => amount,
                    Err(err) => return report(&weak, err),
                };
                let memo = memo.to_string();
                let memo_arg = if memo.trim().is_empty() {
                    None
                } else {
                    Some(memo)
                };
                tx.post(move |worker| {
                    let Some(engine) = worker.engine.as_mut() else {
                        worker.status = String::from("add a mint first");
                        return;
                    };
                    match engine.send_token(amount, memo_arg.as_deref()) {
                        Ok(token) => {
                            worker.status = format!("token for {amount} sat created");
                            let line = flow::SendEcashPhase::AwaitingClaim.user_line();
                            let pending_count = engine.pending_sends().len() as i32;
                            let _ = weak.upgrade_in_event_loop(move |ui| {
                                let logic = ui.global::<WalletLogic>();
                                logic.set_send_ecash_state(String::from("ready").into());
                                logic.set_pending_send_line(line.into());
                                logic.set_pending_send_count(pending_count);
                                logic.set_token_out(token.clone().into());
                                if let Some(image) = qr_image(&token) {
                                    logic.set_token_qr(image);
                                }
                            });
                        }
                        Err(err) => {
                            worker.status = format!("send failed: {err}");
                            let line = flow::SendEcashPhase::Failed.user_line();
                            let _ = weak.upgrade_in_event_loop(move |ui| {
                                let logic = ui.global::<WalletLogic>();
                                logic.set_send_ecash_state(String::from("failed").into());
                                logic.set_pending_send_line(line.into());
                            });
                        }
                    }
                });
            },
        );
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_reclaim_send(move |index: i32| {
            let weak = weak.clone();
            set_busy(&weak);
            let index = usize::try_from(index).unwrap_or(0);
            tx.post(move |worker| {
                let Some(engine) = worker.engine.as_mut() else {
                    return;
                };
                match engine.reclaim_pending(index) {
                    Ok(amount) => {
                        worker.status = format!("reclaimed {amount} sat");
                        let pending_count = engine.pending_sends().len() as i32;
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_token_out(String::new().into());
                            logic.set_token_qr(slint::Image::default());
                            logic.set_send_ecash_state(String::from("amount").into());
                            logic.set_pending_send_line(String::new().into());
                            logic.set_pending_send_count(pending_count);
                        });
                    }
                    Err(err) => {
                        worker.status = format!("reclaim failed: {err}");
                        // A claimed send finalizes during the failed
                        // reclaim; refresh whatever remains.
                        let pending_count = engine.pending_sends().len() as i32;
                        let claimed = pending_count == 0;
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_pending_send_count(pending_count);
                            if claimed {
                                logic.set_token_out(String::new().into());
                                logic.set_token_qr(slint::Image::default());
                                logic.set_pending_send_line(
                                    String::from("Received by the recipient.").into(),
                                );
                            }
                        });
                    }
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        // Auto-quote: an invoice in the field produces the review by
        // itself; typing anything else clears it. No "Get quote" verb.
        logic.on_invoice_edited(move |invoice: slint::SharedString| {
            let invoice = invoice.to_string();
            if !matches!(
                crate::payload::classify(&invoice),
                crate::payload::ScannedPayload::LightningInvoice { .. }
            ) {
                let _ = weak.upgrade_in_event_loop(|ui| {
                    ui.global::<WalletLogic>()
                        .set_melt_quote_info(String::new().into());
                });
                return;
            }
            quote_for_review(&weak, &tx, invoice);
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_melt_quote(move |invoice: slint::SharedString| {
            let weak = weak.clone();
            set_busy(&weak);
            let invoice = invoice.to_string();
            tx.post(move |worker| {
                let Some(engine) = worker.engine.as_mut() else {
                    return;
                };
                match engine.quote_melt(&invoice) {
                    Ok(quote) => {
                        worker.pending_melt = Some(invoice.clone());
                        let info = format!(
                            "pay {} sat (fee reserve {} sat) — press Pay to confirm",
                            quote.amount, quote.fee_reserve
                        );
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<WalletLogic>().set_melt_quote_info(info.into());
                        });
                    }
                    Err(err) => worker.status = format!("melt quote failed: {err}"),
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_melt_confirm(move || {
            let weak = weak.clone();
            set_busy(&weak);
            tx.post(move |worker| {
                let Some(invoice) = worker.pending_melt.clone() else {
                    worker.status = String::from("get a quote first");
                    return;
                };
                let Some(engine) = worker.engine.as_mut() else {
                    return;
                };
                match engine.melt(&invoice) {
                    Ok((_quote, outcome)) if outcome.paid => {
                        worker.status = String::from("invoice paid");
                        let preimage = outcome.preimage.clone().unwrap_or_default();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.global::<WalletLogic>()
                                .set_melt_preimage(preimage.into());
                        });
                    }
                    Ok((_quote, _outcome)) => {
                        worker.status = String::from("payment pending — proofs kept");
                    }
                    Err(err) => worker.status = format!("melt failed: {err}"),
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_add_mint(move |url: slint::SharedString| {
            set_busy(&weak);
            let url = url.trim().trim_end_matches('/').to_string();
            tx.post(move |worker| worker_add_mint(worker, url));
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_switch_mint(move |index: i32| {
            set_busy(&weak);
            tx.post(move |worker| worker_switch_mint(worker, index));
        });
    }

    {
        let weak = weak.clone();
        logic.on_toggle_privacy(move || {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_privacy_hide(!logic.get_privacy_hide());
            });
        });
    }

    {
        let weak = weak.clone();
        let tx = dispatcher.clone();
        logic.on_scan_start(move || {
            let weak = weak.clone();
            let tx = tx.clone();
            start_scanner(&weak, tx);
        });
    }

    {
        logic.on_scan_stop(move || {
            stop_scanner();
        });
    }
}

fn post_current_poll(worker: &mut Worker, quote_id: String, weak: Weak<MainWindow>) {
    let Some(engine) = worker.engine.as_mut() else {
        return;
    };
    match engine.poll_mint_quote(&quote_id) {
        Ok(quote) => {
            let state = quote.state.clone();
            let amount = quote.amount;
            let phase = flow::ReceiveLightningPhase::from_quote_state(&state, amount);
            let phase_line = phase.user_line();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_invoice_state(state.into());
                logic.set_invoice_status_text(phase_line.into());
            });
            // Auto-issuance (UX contract): when the invoice settles, the
            // ecash is added without a "Mint" verb. PaidNotIssued is a
            // money-safety state — surface it for a retry, never as a
            // generic failure.
            if matches!(phase, flow::ReceiveLightningPhase::PaidNotIssued { .. }) {
                drop_mint_paid(worker, quote_id, amount, weak);
            }
        }
        Err(err) => worker.status = format!("quote poll failed: {err}"),
    }
}

/// Shared issuance path for auto-mint (poll) and Try-again (button).
fn drop_mint_paid(worker: &mut Worker, quote_id: String, amount: u64, weak: Weak<MainWindow>) {
    let Some(engine) = worker.engine.as_mut() else {
        return;
    };
    let issuing_line = flow::ReceiveLightningPhase::Issuing { amount }.user_line();
    let _ = weak.upgrade_in_event_loop(|ui| {
        let logic = ui.global::<WalletLogic>();
        logic.set_invoice_state(String::from("ISSUING").into());
        logic.set_invoice_status_text(issuing_line.into());
    });
    match engine.mint_paid_quote(&quote_id, amount) {
        Ok(minted) => {
            worker.status = format!("minted {minted} sat");
            let line = flow::ReceiveLightningPhase::Received { amount: minted }.user_line();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_invoice(String::new().into());
                logic.set_invoice_quote_id(String::new().into());
                logic.set_invoice_state(String::new().into());
                logic.set_invoice_status_text(line.into());
                logic.set_invoice_amount_text(String::new().into());
                logic.set_invoice_qr(slint::Image::default());
            });
        }
        Err(err) => {
            // The invoice is paid but issuance failed: keep the quote id
            // so Try again can resume; show the safety-state wording.
            worker.status = format!("issuance failed after payment: {err}");
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_invoice_state(String::from("PAID").into());
                logic.set_invoice_status_text(
                    String::from(
                        "Payment received — adding your ecash didn't finish. Tap Try again.",
                    )
                    .into(),
                );
            });
        }
    }
}

/// Start the platform scanner (GM65 serial on native, camera on wasm)
/// and route decoded payloads through the universal scan router.
#[cfg(not(target_arch = "wasm32"))]
fn start_scanner(weak: &Weak<MainWindow>, dispatcher: Dispatcher) {
    use crate::gm65;

    thread_local! {
        static GM65_STOP: std::cell::RefCell<Option<gm65::Gm65Stop>> =
            const { std::cell::RefCell::new(None) };
    }

    GM65_STOP.with(|slot| *slot.borrow_mut() = None);
    let weak_for_status = weak.clone();
    let weak_for_scan = weak.clone();
    let tx_for_scan = dispatcher;
    match gm65::spawn_gm65_reader(
        move |token| {
            route_scanned(&weak_for_scan, tx_for_scan.clone(), token);
            stop_scanner();
        },
        move |status| {
            let _ = weak_for_status.upgrade_in_event_loop(move |ui| {
                (ui.global::<WalletLogic>()).set_scan_status(status.into());
            });
        },
    ) {
        Ok(stop) => GM65_STOP.with(|slot| *slot.borrow_mut() = Some(stop)),
        Err(err) => {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                (ui.global::<WalletLogic>()).set_scan_status(format!("gm65: {err}").into());
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn start_scanner(weak: &Weak<MainWindow>, _dispatcher: Dispatcher) {
    crate::camera::start_camera(weak.clone());
}

#[cfg(not(target_arch = "wasm32"))]
fn stop_scanner() {
    use crate::gm65::Gm65Stop;
    thread_local! {
        static GM65_STOP: std::cell::RefCell<Option<Gm65Stop>> =
            const { std::cell::RefCell::new(None) };
    }
    GM65_STOP.with(|slot| {
        if let Some(stop) = slot.borrow_mut().take() {
            stop.stop();
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn stop_scanner() {
    crate::camera::stop_camera();
}

fn start_quote_poller(ui: &MainWindow, dispatcher: Dispatcher) {
    thread_local! {
        static KEEP_ALIVE: std::cell::RefCell<Vec<slint::Timer>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(2),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let logic = ui.global::<WalletLogic>();
            if logic.get_busy() || logic.get_invoice_quote_id().is_empty() {
                return;
            }
            let state = logic.get_invoice_state().to_string();
            if state == "PAID" || state == "ISSUED" {
                return;
            }
            let quote_id = logic.get_invoice_quote_id().to_string();
            let weak = weak.clone();
            dispatcher.post(move |worker| {
                post_current_poll(worker, quote_id, weak);
            });
        },
    );
    KEEP_ALIVE.with(|timers| timers.borrow_mut().push(timer));
}

/// Quote an invoice and populate the pay review (UX contract: quote
/// acquisition is machinery, not a verb). Shared by typing and scanning.
fn quote_for_review(weak: &Weak<MainWindow>, tx: &Dispatcher, invoice: String) {
    let weak = weak.clone();
    let tx = tx.clone();
    set_busy(&weak);
    tx.post(move |worker| {
        let Some(engine) = worker.engine.as_mut() else {
            return;
        };
        match engine.quote_melt(&invoice) {
            Ok(quote) => {
                worker.pending_melt = Some(invoice.clone());
                let phase = flow::PayLightningPhase::Review {
                    amount: quote.amount,
                    fee_reserve: quote.fee_reserve,
                };
                let line = phase.user_line();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<WalletLogic>().set_melt_quote_info(line.into());
                });
            }
            Err(err) => {
                worker.status = format!("quote failed: {err}");
                let line = flow::classify(&err).user_line();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<WalletLogic>().set_melt_quote_info(line.into());
                });
            }
        }
    });
}

fn set_busy(weak: &Weak<MainWindow>) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.global::<WalletLogic>().set_busy(true);
    });
}

/// Inspect a token and drive the receive-review state from the semantic
/// phases (UX contract: inspection is wallet machinery, not a button).
fn inspect_for_review(weak: &Weak<MainWindow>, tx: &Dispatcher, token: String) {
    let weak = weak.clone();
    let tx = tx.clone();
    let _ = weak.upgrade_in_event_loop(|ui| {
        ui.global::<WalletLogic>()
            .set_receive_ecash_state(String::from("inspecting").into());
    });
    tx.post(move |worker| {
        let Some(engine) = worker.engine.as_mut() else {
            return;
        };
        let phase = match engine.inspect_token(&token) {
            Ok(inspection) => {
                if inspection.all_spent() {
                    flow::ReceiveEcashPhase::Failed(flow::FlowFailure::AlreadySpent)
                } else {
                    flow::ReceiveEcashPhase::Review(inspection)
                }
            }
            Err(failure) => flow::ReceiveEcashPhase::Failed(failure),
        };
        let (line, fee) = match &phase {
            flow::ReceiveEcashPhase::Review(inspection) => {
                (phase.user_line(), inspection.fee_line())
            }
            _ => (phase.user_line(), String::new()),
        };
        let state = match phase {
            flow::ReceiveEcashPhase::Review(_) => "review",
            flow::ReceiveEcashPhase::Failed(_) => "failed",
            _ => "input",
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let logic = ui.global::<WalletLogic>();
            logic.set_receive_ecash_state(state.into());
            logic.set_receive_review_line(line.into());
            logic.set_receive_review_fee(fee.into());
        });
    });
}

/// Universal scan routing (UX contract): a decode resolves an intent and
/// pre-populates its screen — it never authorizes anything.
pub fn route_scanned(weak: &Weak<MainWindow>, tx: Dispatcher, text: String) {
    use crate::payload::ScannedPayload;
    let _ = weak.upgrade_in_event_loop(move |ui| {
        let logic = ui.global::<WalletLogic>();
        logic.set_scanning(false);
        logic.set_scan_status(String::new().into());
        match crate::payload::classify(&text) {
            ScannedPayload::CashuToken { token } => {
                logic.set_token_in(token.clone().into());
                ui.invoke_navigate(Page::Receive);
                inspect_for_review(&ui.as_weak(), &tx, token);
            }
            ScannedPayload::LightningInvoice { invoice } => {
                logic.set_invoice_in(invoice.clone().into());
                logic.set_send_tab(1);
                ui.invoke_navigate(Page::Send);
                quote_for_review(&ui.as_weak(), &tx, invoice);
            }
            ScannedPayload::MintUrl { url } => {
                logic.set_mint_add_url(url.into());
                ui.invoke_navigate(Page::Mints);
            }
            ScannedPayload::Unknown => {
                logic.set_scan_status(String::from("Not recognized — try a Cashu QR").into());
            }
        }
    });
}

fn report(weak: &Weak<MainWindow>, message: String) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        let logic = ui.global::<WalletLogic>();
        logic.set_busy(false);
        logic.set_status_message(message.into());
    });
}

/// Render `text` as a QR code image (white background, black modules,
/// 4-module quiet zone) for on-screen display.
pub fn qr_image(text: &str) -> Option<slint::Image> {
    use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
    let mut outbuffer = vec![0u8; Version::MAX.buffer_len()];
    let mut tempbuffer = vec![0u8; Version::MAX.buffer_len()];
    let code = QrCode::encode_text(
        text,
        &mut tempbuffer,
        &mut outbuffer,
        QrCodeEcc::Medium,
        Version::MIN,
        Version::MAX,
        None,
        true,
    )
    .ok()?;
    let modules = code.size() as usize;
    let dim = (modules + QR_QUIET_ZONE * 2) * QR_SCALE;
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(dim as u32, dim as u32);
    for y in 0..dim {
        for x in 0..dim {
            let module_x = (x / QR_SCALE) as i32 - QR_QUIET_ZONE as i32;
            let module_y = (y / QR_SCALE) as i32 - QR_QUIET_ZONE as i32;
            let dark = module_x >= 0
                && module_y >= 0
                && (module_x as usize) < modules
                && (module_y as usize) < modules
                && code.get_module(module_x, module_y);
            let value = if dark { 0 } else { 255 };
            let pixel = &mut buffer.make_mut_slice()[y * dim + x];
            *pixel = slint::Rgb8Pixel {
                r: value,
                g: value,
                b: value,
            };
        }
    }
    Some(slint::Image::from_rgb8(buffer))
}
