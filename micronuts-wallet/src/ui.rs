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

use crate::engine::{HistoryEntry, HistoryKind, WalletEngine};
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
#[cfg(target_arch = "wasm32")]
use cashu_core_lite::store::MemoryStore;
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
type Transport = DemoMintClient;
#[cfg(target_arch = "wasm32")]
type Store = MemoryStore;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce(&mut Worker) + Send + 'static>;
#[cfg(target_arch = "wasm32")]
type Job = Box<dyn FnOnce(&mut Worker) + 'static>;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WORKER: std::cell::RefCell<Option<Worker>> =
        const { std::cell::RefCell::new(None) };
    static MIRROR_TIMER: std::cell::RefCell<Option<slint::Timer>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Clone)]
struct Dispatcher {
    #[cfg(not(target_arch = "wasm32"))]
    tx: mpsc::Sender<Job>,
    #[cfg(target_arch = "wasm32")]
    weak: Weak<MainWindow>,
}

impl Dispatcher {
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
    }

    ui.run()
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
    fn new(_dir: &Path) -> Result<Self, String> {
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
        let engine = WalletEngine::new(
            crate::demo_mint::DEMO_MINT_URL,
            DemoMintClient::new(mint_seed),
            MemoryStore::new(),
            wallet_seed,
            Vec::new(),
        )
        .map_err(|e| format!("browser engine: {e}"))?;
        Ok(Self {
            dir: PathBuf::new(),
            state,
            engine: Some(engine),
            pending_melt: None,
            status: String::from("browser demo — embedded mint, state lives in this tab only"),
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
        match WalletEngine::new(
            &active,
            client,
            store,
            self.seed(),
            self.state.history.clone(),
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
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_state(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            self.state.history = engine.history().to_vec();
        }
        if let Err(err) = self.state.save(&self.state_path()) {
            self.status = format!("state save failed: {}", store_error_text(err));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn persist_state(&mut self) {
        self.state.history = self
            .engine
            .as_ref()
            .map(|engine| engine.history().to_vec())
            .unwrap_or_default();
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
                        line2: entry.detail.clone().into(),
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

fn history_line1(entry: &HistoryEntry) -> String {
    let (arrow, label) = match entry.kind {
        HistoryKind::Mint => ("↓", "mint"),
        HistoryKind::Send => ("↑", "send"),
        HistoryKind::Receive => ("↓", "receive"),
        HistoryKind::Melt => ("↑", "melt"),
    };
    format!("{arrow} {amount} sat · {label}", amount = entry.amount)
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
                let Some(engine) = worker.engine.as_mut() else {
                    return;
                };
                match engine.mint_paid_quote(&quote_id, amount) {
                    Ok(minted) => {
                        worker.status = format!("minted {minted} sat");
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            let logic = ui.global::<WalletLogic>();
                            logic.set_invoice(String::new().into());
                            logic.set_invoice_quote_id(String::new().into());
                            logic.set_invoice_state(String::new().into());
                            logic.set_invoice_status_text(
                                flow::ReceiveLightningPhase::Received { amount }
                                    .user_line()
                                    .into(),
                            );
                            logic.set_invoice_amount_text(String::new().into());
                            logic.set_invoice_qr(slint::Image::default());
                        });
                    }
                    Err(err) => worker.status = format!("mint failed: {err}"),
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_receive_token(move |token: slint::SharedString| {
            let weak = weak.clone();
            set_busy(&weak);
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
                            logic.set_token_check_text(String::new().into());
                        });
                    }
                    Err(err) => worker.status = format!("receive failed: {err}"),
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_check_token(move |token: slint::SharedString| {
            let weak = weak.clone();
            set_busy(&weak);
            let token = token.to_string();
            tx.post(move |worker| {
                let Some(engine) = worker.engine.as_mut() else {
                    return;
                };
                // Inspect = parse + mint match + proof health, mapped onto
                // the semantic Review/Failure phases (UX contract).
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
                let report = phase.user_line();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<WalletLogic>()
                        .set_token_check_text(report.into());
                });
            });
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
                            let _ = weak.upgrade_in_event_loop(move |ui| {
                                let logic = ui.global::<WalletLogic>();
                                logic.set_token_out(token.clone().into());
                                if let Some(image) = qr_image(&token) {
                                    logic.set_token_qr(image);
                                }
                            });
                        }
                        Err(err) => worker.status = format!("send failed: {err}"),
                    }
                });
            },
        );
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
            tx.post(move |worker| {
                if url.is_empty() {
                    return;
                }
                if worker.state.mints.iter().any(|m| m.url == url) {
                    worker.status = format!("mint already known: {url}");
                    return;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let mut probe = HttpMintClient::new(&url);
                    match probe.get_info() {
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
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = url;
                    worker.status =
                        String::from("browser demo: the embedded mint is the only mint here");
                }
            });
        });
    }

    {
        let tx = dispatcher.clone();
        let weak = weak.clone();
        logic.on_switch_mint(move |index: i32| {
            set_busy(&weak);
            tx.post(move |worker| {
                #[cfg(not(target_arch = "wasm32"))]
                {
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
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = (index, worker);
                }
            });
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
        logic.on_scan_start(move || {
            let weak = weak.clone();
            start_scanner(&weak);
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
            let phase_line =
                flow::ReceiveLightningPhase::from_quote_state(&state, amount).user_line();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_invoice_state(state.into());
                logic.set_invoice_status_text(phase_line.into());
            });
        }
        Err(err) => worker.status = format!("quote poll failed: {err}"),
    }
}

/// Start the platform scanner (GM65 serial on native, camera on wasm)
/// and route decoded tokens into the Receive field.
#[cfg(not(target_arch = "wasm32"))]
fn start_scanner(weak: &Weak<MainWindow>) {
    use crate::gm65;

    thread_local! {
        static GM65_STOP: std::cell::RefCell<Option<gm65::Gm65Stop>> =
            const { std::cell::RefCell::new(None) };
    }

    GM65_STOP.with(|slot| *slot.borrow_mut() = None);
    let weak_for_status = weak.clone();
    let weak_for_scan = weak.clone();
    match gm65::spawn_gm65_reader(
        move |token| {
            let _ = weak_for_scan.upgrade_in_event_loop(move |ui| {
                let logic = ui.global::<WalletLogic>();
                logic.set_scanning(false);
                logic.set_scan_status(String::new().into());
                logic.set_token_in(token.into());
                ui.invoke_navigate(Page::Receive);
            });
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
fn start_scanner(weak: &Weak<MainWindow>) {
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

fn set_busy(weak: &Weak<MainWindow>) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.global::<WalletLogic>().set_busy(true);
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
