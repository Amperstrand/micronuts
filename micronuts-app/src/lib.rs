#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;

pub mod command_handler;
pub mod display;
pub mod hardware;
pub mod protocol;
pub mod qr;
pub mod state;
pub mod util;

pub use hardware::{MicronutsHardware, TouchPoint};

enum AppScreen {
    Home,
    Scanning,
    ScanResult,
    WaitingToken,
    TokenInfo,
    ShowProofs,
}

pub fn run<H: MicronutsHardware>(hw: &mut H) -> ! {
    let scanner_connected = hw.scanner_status().connected;
    let mut screen = AppScreen::Home;
    let mut touch_active = false;
    display::render_home(hw.display(), scanner_connected);

    let buttons = display::home_buttons();
    let back_btn = display::back_button();
    let mut state = state::FirmwareState::new();
    let mut last_scan_data: Option<Vec<u8>> = None;
    loop {
        if let Some(frame) = hw.transport_poll() {
            let response = command_handler::handle_command(
                frame.command,
                frame.payload(),
                &mut state,
                hw,
                &mut last_scan_data,
            );
            if frame.command == protocol::Command::ScannerTrigger {
                last_scan_data = None;
            }
            if frame.command == protocol::Command::ImportToken {
                if let AppScreen::WaitingToken = screen {
                    screen = AppScreen::TokenInfo;
                }
            }
            hw.transport_send(&response);
        }

        match screen {
            AppScreen::Home => {
                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if buttons[0].hit(tp.x, tp.y) {
                            screen = AppScreen::Scanning;
                            last_scan_data = None;
                            let _ = hw.scanner_trigger();
                            display::draw_scanning(hw.display());
                        } else if buttons[1].hit(tp.x, tp.y) {
                            screen = AppScreen::WaitingToken;
                            display::render_waiting_token(hw.display());
                        } else if buttons[2].hit(tp.x, tp.y) {
                            if state.swap_state == state::SwapState::ProofsReady {
                                screen = AppScreen::ShowProofs;
                                display::render_status(hw.display(), "Generating proof QR...");
                            } else {
                                display::render_status(hw.display(), "No proofs available yet");
                                screen = AppScreen::Home;
                                display::render_home(hw.display(), scanner_connected);
                            }
                        }
                    }
                } else {
                    touch_active = false;
                }

                for _ in 0..256 {
                    if let Some(data) = hw.scanner_try_read() {
                        let payload = qr::decode_qr(&data);
                        screen = AppScreen::ScanResult;
                        display::render_decoded_scan(hw.display(), &payload);
                        last_scan_data = Some(data);
                        break;
                    }
                }
            }
            AppScreen::Scanning => {
                for _ in 0..256 {
                    if let Some(data) = hw.scanner_try_read() {
                        let payload = qr::decode_qr(&data);
                        screen = AppScreen::ScanResult;
                        display::render_decoded_scan(hw.display(), &payload);
                        last_scan_data = Some(data);
                        break;
                    }
                }

                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if back_btn.hit(tp.x, tp.y) {
                            screen = AppScreen::Home;
                            display::render_home(hw.display(), scanner_connected);
                        }
                    }
                } else {
                    touch_active = false;
                }
            }
            AppScreen::ScanResult => {
                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if back_btn.hit(tp.x, tp.y) {
                            screen = AppScreen::Home;
                            display::render_home(hw.display(), scanner_connected);
                        }
                    }
                } else {
                    touch_active = false;
                }
            }
            AppScreen::WaitingToken => {
                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if back_btn.hit(tp.x, tp.y) {
                            screen = AppScreen::Home;
                            display::render_home(hw.display(), scanner_connected);
                        }
                    }
                } else {
                    touch_active = false;
                }
            }
            AppScreen::TokenInfo => {
                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if back_btn.hit(tp.x, tp.y) {
                            screen = AppScreen::Home;
                            display::render_home(hw.display(), scanner_connected);
                        }
                    }
                } else {
                    touch_active = false;
                }
            }
            AppScreen::ShowProofs => {
                if let Some(tp) = hw.touch_get() {
                    if !touch_active {
                        touch_active = true;
                        if back_btn.hit(tp.x, tp.y) {
                            screen = AppScreen::Home;
                            display::render_home(hw.display(), scanner_connected);
                        }
                    }
                } else {
                    touch_active = false;
                }
            }
        }

        hw.delay_ms(5);
    }
}
