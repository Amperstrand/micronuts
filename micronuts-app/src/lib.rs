#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;

use embassy_time::Duration;

pub mod command_handler;
pub mod display;
pub mod hardware;
pub mod protocol;
pub mod qr;
pub mod scanflow;
pub mod state;
pub mod util;

#[cfg(any(test, feature = "std"))]
pub mod test_util;

pub use hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};

enum AppScreen {
    Home,
    Scanning,
    ScanResult,
    WaitingToken,
    TokenInfo,
    ShowProofs,
}

pub async fn run<H: MicronutsHardware>(hw: &mut H) -> ! {
    let scanner_connected = hw.is_connected();
    let mut screen = AppScreen::Home;
    let mut touch_active = false;
    display::render_home(hw.display(), scanner_connected, false);
    hw.swap_buffers();

    let buttons = display::home_buttons();
    let back_btn = display::back_button();
    let aim_btn = display::aim_button();
    let mut state = state::FirmwareState::new();
    let mut last_scan_data: Option<Vec<u8>> = None;
    // CDC scan capture window: ScannerTrigger has no UI screen attached,
    // so the ticker arm harvests read_scan() into last_scan_data for the
    // following ScannerData poll (10 s window, one long await per trigger —
    // chopped reads eat UART bytes mid-frame and the frame never completes).
    let mut capture_until: Option<embassy_time::Instant> = None;
    let mut aim_on: bool = false;
    let mut scan_ticks: u32 = 0;
    let mut scan_retries: u32 = 0;
    const SCAN_TIMEOUT_TICKS: u32 = 10 * 200;
    const MAX_SCAN_RETRIES: u32 = 3;

    let mut poll_ticker = embassy_time::Ticker::every(Duration::from_millis(5));

    loop {
        match embassy_futures::select::select(hw.transport_recv_frame(), poll_ticker.next()).await {
            embassy_futures::select::Either::First(maybe_frame) => {
                if let Some(frame) = maybe_frame {
                    let response = command_handler::handle_command(
                        frame.command,
                        frame.payload(),
                        &mut state,
                        hw,
                        &mut last_scan_data,
                    )
                    .await;
                    if frame.command == protocol::Command::ScannerTrigger {
                        last_scan_data = None;
                        capture_until =
                            Some(embassy_time::Instant::now() + Duration::from_secs(10));
                    }
                    if frame.command == protocol::Command::ImportToken {
                        if let AppScreen::WaitingToken = screen {
                            screen = AppScreen::TokenInfo;
                        }
                    }
                    match frame.command {
                        protocol::Command::ImportToken
                        | protocol::Command::ScannerTrigger
                        | protocol::Command::ScannerData
                        | protocol::Command::GetBlinded
                        | protocol::Command::SendSignatures => hw.swap_buffers(),
                        _ => {}
                    }
                    hw.transport_send(&response).await;
                }
            }
            embassy_futures::select::Either::Second(_) => {
                if let Some(deadline) = capture_until {
                    match embassy_time::with_timeout(
                        deadline - embassy_time::Instant::now(),
                        hw.read_scan(),
                    )
                    .await
                    {
                        Ok(Some(data)) => {
                            last_scan_data = Some(data);
                            capture_until = None;
                        }
                        _ => capture_until = None,
                    }
                }

                let mut go_home = false;

                match screen {
                    AppScreen::Home => {
                        if let Some(tp) = hw.touch_get() {
                            if !touch_active {
                                touch_active = true;
                                if buttons[0].hit(tp.x, tp.y) {
                                    screen = AppScreen::Scanning;
                                    last_scan_data = None;
                                    aim_on = true;
                                    state.scan_assembler.reset();
                                    let _ = hw.set_aim(true).await;
                                    let _ = hw.trigger().await;
                                    display::draw_scanning(hw.display(), true);
                                    display::draw_scanning_progress(hw.display(), 0, 10);
                                    hw.swap_buffers();
                                    scan_ticks = 0;
                                    scan_retries = 0;
                                } else if buttons[1].hit(tp.x, tp.y) {
                                    if let Some(data) = last_scan_data.as_ref() {
                                        let payload = qr::decode_qr(data);
                                        screen = AppScreen::ScanResult;
                                        display::render_decoded_scan(hw.display(), &payload);
                                        hw.swap_buffers();
                                    } else {
                                        screen = AppScreen::WaitingToken;
                                        display::render_waiting_token(hw.display());
                                        hw.swap_buffers();
                                    }
                                } else if buttons[2].hit(tp.x, tp.y) {
                                    if state.swap_state == state::SwapState::ProofsReady {
                                        screen = AppScreen::ShowProofs;
                                        display::render_export_qr(hw.display(), &state);
                                        hw.swap_buffers();
                                    } else {
                                        display::render_status(
                                            hw.display(),
                                            "No proofs available yet",
                                        );
                                        screen = AppScreen::Home;
                                        display::render_home(
                                            hw.display(),
                                            scanner_connected,
                                            last_scan_data.is_some(),
                                        );
                                        hw.swap_buffers();
                                    }
                                }
                            }
                        } else {
                            touch_active = false;
                        }
                    }
                    AppScreen::Scanning => {
                        match embassy_time::with_timeout(Duration::from_secs(5), hw.read_scan())
                            .await
                        {
                            Ok(Some(data)) => match state.scan_assembler.process(&data) {
                                scanflow::ScanOutcome::TokenReady(token) => {
                                    let _ = hw.set_aim(false).await;
                                    aim_on = false;
                                    scan_ticks = 0;
                                    scan_retries = 0;
                                    display::render_token_info(hw.display(), &token);
                                    hw.swap_buffers();
                                    state.imported_token = Some(token);
                                    state.swap_state = state::SwapState::TokenImported;
                                    last_scan_data = Some(data);
                                    screen = AppScreen::TokenInfo;
                                }
                                scanflow::ScanOutcome::KeepScanning { received, total } => {
                                    scan_ticks = 0;
                                    scan_retries = 0;
                                    last_scan_data = Some(data);
                                    display::draw_scanning(hw.display(), aim_on);
                                    display::draw_scanning_progress(hw.display(), received, total);
                                    hw.swap_buffers();
                                    let _ = hw.trigger().await;
                                }
                                scanflow::ScanOutcome::InvalidFragment => {
                                    scan_ticks = 0;
                                    last_scan_data = Some(data);
                                    let _ = hw.trigger().await;
                                }
                                scanflow::ScanOutcome::ShowPayload(payload) => {
                                    screen = AppScreen::ScanResult;
                                    let _ = hw.set_aim(false).await;
                                    aim_on = false;
                                    scan_ticks = 0;
                                    scan_retries = 0;
                                    display::render_decoded_scan(hw.display(), &payload);
                                    hw.swap_buffers();
                                    last_scan_data = Some(data);
                                }
                            },
                            _ => {
                                scan_ticks += 1;
                                if scan_ticks.is_multiple_of(200) {
                                    display::draw_scanning(hw.display(), aim_on);
                                    display::draw_scanning_progress(
                                        hw.display(),
                                        scan_ticks / 200,
                                        SCAN_TIMEOUT_TICKS / 200,
                                    );
                                    hw.swap_buffers();
                                }
                                if scan_ticks > SCAN_TIMEOUT_TICKS {
                                    scan_retries += 1;
                                    scan_ticks = 0;
                                    if scan_retries < MAX_SCAN_RETRIES {
                                        display::draw_scanning(hw.display(), aim_on);
                                        display::draw_scanning_retry(hw.display());
                                        hw.swap_buffers();
                                        let _ = hw.trigger().await;
                                    } else {
                                        go_home = true;
                                    }
                                }
                            }
                        }

                        if let Some(tp) = hw.touch_get() {
                            if !touch_active {
                                touch_active = true;
                                if back_btn.hit(tp.x, tp.y) {
                                    go_home = true;
                                } else if aim_btn.hit(tp.x, tp.y) {
                                    aim_on = !aim_on;
                                    let _ = hw.set_aim(aim_on).await;
                                    display::draw_scanning(hw.display(), aim_on);
                                    display::draw_scanning_progress(
                                        hw.display(),
                                        scan_ticks / 200,
                                        SCAN_TIMEOUT_TICKS / 200,
                                    );
                                    hw.swap_buffers();
                                }
                            }
                        } else {
                            touch_active = false;
                        }
                    }
                    AppScreen::ScanResult
                    | AppScreen::WaitingToken
                    | AppScreen::TokenInfo
                    | AppScreen::ShowProofs => {
                        if let Some(tp) = hw.touch_get() {
                            if !touch_active {
                                touch_active = true;
                                if back_btn.hit(tp.x, tp.y) {
                                    go_home = true;
                                }
                            }
                        } else {
                            touch_active = false;
                        }
                    }
                }

                if go_home {
                    let _ = hw.set_aim(false).await;
                    aim_on = false;
                    state.scan_assembler.reset();
                    hw.stop().await;
                    screen = AppScreen::Home;
                    display::render_home(hw.display(), scanner_connected, last_scan_data.is_some());
                    hw.swap_buffers();
                    scan_ticks = 0;
                    scan_retries = 0;
                }
            }
        }
    }
}
