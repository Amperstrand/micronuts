//! Micronuts Firmware - Cashu Hardware Wallet POC
//!
//! Main firmware loop for STM32F469I-Discovery board.
//! Handles USB CDC communication and Cashu blind signature operations.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;
use alloc::vec::Vec;

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use cashu_core_lite::{
    blind_message, decode_token, encode_token, unblind_signature, BlindedMessage, Proof, TokenV4,
    TokenV4Token,
};
use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565, prelude::*};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{PublicKey, SecretKey};
use static_cell::ConstStaticCell;
use stm32f469i_disc::{
    hal,
    hal::gpio::alt::fmc as alt,
    hal::ltdc::{Layer, LtdcFramebuffer, PixelFormat},
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    hal::rng::RngExt,
    lcd, sdram, touch, usb,
};

use firmware::display::{self, Button};
use firmware::firmware_state::{FirmwareState, SwapState};
use firmware::qr::{Gm65Scanner, ScannerDriverSync};
use firmware::usb::{CdcPort, Command, Response, Status};
use hal::otg_fs::UsbBus;
use hal::rng::Rng;
use hal::serial::Serial6;
use rand_core::RngCore as _;
use usb_device::prelude::*;

enum Screen {
    Home,
    Scanning,
    ScanResult,
}

static EP_MEMORY: ConstStaticCell<[u32; 1024]> = ConstStaticCell::new([0; 1024]);

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let cp = CorePeripherals::take().unwrap();

    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .pclk2(32.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );
    let mut delay = cp.SYST.delay(&rcc.clocks);

    defmt::info!("Micronuts firmware starting...");

    let mut rng = dp.RNG.constrain(&mut rcc);

    let gpioa = dp.GPIOA.split(&mut rcc);
    let gpiob = dp.GPIOB.split(&mut rcc);
    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let mut gpiog = dp.GPIOG.split(&mut rcc);
    let scanner_tx = gpiog.pg14;
    let scanner_rx = gpiog.pg9;
    let gpioh = dp.GPIOH.split(&mut rcc);
    let gpioi = dp.GPIOI.split(&mut rcc);

    let ts_int = gpioc.pc1.into_pull_down_input();

    let mut lcd_reset = gpioh.ph7.into_push_pull_output();
    lcd_reset.set_low();
    delay.delay_ms(20u32);
    lcd_reset.set_high();
    delay.delay_ms(10u32);

    defmt::info!("Initializing SDRAM...");

    let mut sdram = sdram::Sdram::new(
        dp.FMC,
        sdram::sdram_pins!(gpioc, gpiod, gpioe, gpiof, gpiog, gpioh, gpioi),
        &rcc.clocks,
        &mut delay,
    );

    {
        const HEAP_SIZE: usize = 128 * 1024;
        let heap_start = sdram.mem as *mut u8;
        unsafe {
            let heap_ptr = heap_start.add(lcd::FB_SIZE * 2);
            ALLOCATOR.lock().init(heap_ptr as *mut u8, HEAP_SIZE);
        }
    }

    defmt::info!("Initializing display...");

    let (display_ctrl, _controller) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::Unknown,
        PixelFormat::RGB565,
    );

    let mut dbl_fb = lcd::DoubleFramebuffer::new(
        &mut sdram,
        display_ctrl,
        lcd::BoardHint::Unknown,
        PixelFormat::RGB565,
    );

    defmt::info!("Display initialized");

    defmt::info!("Initializing touch...");
    let mut touch_i2c = touch::init_i2c(dp.I2C1, gpiob.pb8, gpiob.pb9, &mut rcc);
    let mut touch_ctrl = touch::init_ft6x06(&touch_i2c, ts_int);
    let touch_available = touch_ctrl.is_some();
    if touch_available {
        defmt::info!("Touch controller ready");
    } else {
        defmt::warn!("Touch controller not found");
    }

    // --- Boot splash animation ---
    {
        defmt::info!("Running boot splash...");
        let mut splash_state = firmware::boot_splash::SplashState::new();
        let mut splash_done = false;
        // Auto-exit after 2 full variant cycles (2 × 3 variants × 90 frames/variant)
        const MAX_SPLASH_FRAMES: u32 = 2 * 3 * 90;
        // Run splash: cycle through variants, touch to exit
        while !splash_done {
            firmware::boot_splash::render_frame(
                dbl_fb.back_buffer(),
                lcd::WIDTH as u32,
                lcd::HEIGHT as u32,
                &mut splash_state,
            );
            dbl_fb.swap();

            // Simple frame pacing: ~33ms delay for ~30 FPS
            delay.delay_ms(33u32);

            // Check touch to exit
            if let Some(ref mut t) = touch_ctrl {
                if let Ok(status) = t.td_status(&mut touch_i2c) {
                    if status > 0 {
                        defmt::info!("Touch detected, exiting splash");
                        splash_done = true;
                    }
                }
            }

            if splash_state.global_frame >= MAX_SPLASH_FRAMES {
                defmt::info!("Splash timeout, continuing boot");
                splash_done = true;
            }
        }
        defmt::info!("Boot splash complete");
    }

    let mut fb = LtdcFramebuffer::new(dbl_fb.into_front_buffer(), lcd::WIDTH, lcd::HEIGHT);

    defmt::info!("Initializing USB...");
    let usb_periph = usb::init(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        gpioa.pa11,
        gpioa.pa12,
        &rcc.clocks,
    );

    let usb_bus = UsbBus::new(usb_periph, EP_MEMORY.take());

    let serial = usbd_serial::SerialPort::new(&usb_bus);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .device_class(usbd_serial::USB_CLASS_CDC)
        .strings(&[StringDescriptors::default()
            .manufacturer("Micronuts")
            .product("Cashu Hardware Wallet")
            .serial_number("F4691")])
        .unwrap()
        .build();

    let mut cdc_port = CdcPort::new(serial);

    defmt::info!("Initializing QR scanner (USART6)...");
    let baud_rates: [u32; 3] = [9600, 57600, 115200];
    let mut scanner: Option<Gm65Scanner<Serial6>> = None;
    let mut scanner_usart = Some(dp.USART6);
    let mut scanner_pins = Some((scanner_tx, scanner_rx));
    let mut probe_baud: u32 = 9600;

    for &baud in &baud_rates {
        let (usart, pins) = match (scanner_usart.take(), scanner_pins.take()) {
            (Some(u), Some(p)) => (u, p),
            _ => break,
        };
        let uart = usart.serial(pins, baud.bps(), &mut rcc).unwrap();
        let mut s = Gm65Scanner::with_default_config(uart);
        defmt::info!("Probing scanner at {} bps...", baud);
        if s.ping() {
            defmt::info!("Scanner found at {} bps", baud);
            probe_baud = baud;
            scanner = Some(s);
            break;
        }
        defmt::info!("No response at {} bps, trying next...", baud);
        let (raw_usart, raw_pins) = s.release().release();
        scanner_usart = Some(raw_usart);
        let tx_pin: hal::gpio::Pin<'G', 14> = raw_pins.0.unwrap().try_into().ok().unwrap();
        let rx_pin: hal::gpio::Pin<'G', 9> = raw_pins.1.unwrap().try_into().ok().unwrap();
        scanner_pins = Some((tx_pin, rx_pin));
    }

    let mut scanner = match scanner {
        Some(s) => s,
        None => {
            let (usart, pins) = match (scanner_usart.take(), scanner_pins.take()) {
                (Some(u), Some(p)) => (u, p),
                _ => panic!("No USART6 available"),
            };
            let uart = usart.serial(pins, 9600.bps(), &mut rcc).unwrap();
            let mut s = Gm65Scanner::with_default_config(uart);
            defmt::warn!("QR scanner not found at any baud rate, using 9600 default");
            s
        }
    };

    match scanner.init() {
        Ok(model) => {
            defmt::info!("QR scanner ready: {}", model);
            if probe_baud != 115200 {
                defmt::info!("Re-initializing UART at 115200 bps...");
                let (raw_usart, raw_pins) = scanner.release().release();
                let tx_pin: hal::gpio::Pin<'G', 14> = raw_pins.0.unwrap().try_into().ok().unwrap();
                let rx_pin: hal::gpio::Pin<'G', 9> = raw_pins.1.unwrap().try_into().ok().unwrap();
                let uart = raw_usart
                    .serial((tx_pin, rx_pin), 115200.bps(), &mut rcc)
                    .unwrap();
                scanner = Gm65Scanner::with_default_config(uart);
                if scanner.ping() {
                    defmt::info!("UART re-init at 115200 bps confirmed");
                } else {
                    defmt::warn!("UART re-init at 115200 bps failed");
                }
            }
        }
        Err(e) => defmt::warn!("QR scanner init failed: {}", e),
    }

    defmt::info!("USB initialized, entering main loop");

    let scanner_connected = scanner.state() == gm65_scanner::ScannerState::Ready;
    let mut screen = Screen::Home;
    let mut touch_active = false;
    firmware::display::render_home(&mut fb, scanner_connected);

    let buttons = display::home_buttons();
    let back_btn = display::back_button();
    let mut state = FirmwareState::new();
    let mut last_scan_data: Option<Vec<u8>> = None;
    let mut scan_active = false;

    loop {
        if usb_dev.poll(&mut [cdc_port.serial_mut()]) {
            if let Some(frame) = cdc_port.receive_frame() {
                let response = handle_command(
                    frame.command,
                    frame.payload(),
                    &mut state,
                    &mut rng,
                    &mut fb,
                    &mut scanner,
                    &mut last_scan_data,
                );
                if frame.command == Command::ScannerTrigger {
                    scan_active = true;
                    last_scan_data = None;
                }
                cdc_port.send_response(&response);
            }
        }

        match screen {
            Screen::Home => {
                if let Some(ref mut t) = touch_ctrl {
                    if let Ok(status) = t.td_status(&mut touch_i2c) {
                        if status > 0 && !touch_active {
                            touch_active = true;
                            if let Ok(tp) = t.get_touch(&mut touch_i2c, 1) {
                                if tp.detected {
                                    if buttons[0].hit(tp.x, tp.y) {
                                        defmt::info!("SCAN QR button pressed");
                                        screen = Screen::Scanning;
                                        scan_active = true;
                                        last_scan_data = None;
                                        let _ = scanner.trigger_scan();
                                        firmware::display::draw_scanning(&mut fb);
                                    }
                                }
                            }
                        } else if status == 0 {
                            touch_active = false;
                        }
                    }
                }

                for _ in 0..256 {
                    if let Some(data) = scanner.try_read_scan() {
                        defmt::info!("Scan data received: {} bytes", data.len());
                        let payload = firmware::qr::decode_qr(&data);
                        screen = Screen::ScanResult;
                        firmware::display::render_decoded_scan(&mut fb, &payload);
                        last_scan_data = Some(data);
                        scan_active = false;
                        break;
                    }
                }
            }
            Screen::Scanning => {
                for _ in 0..256 {
                    if let Some(data) = scanner.try_read_scan() {
                        defmt::info!("Scan data received: {} bytes", data.len());
                        let payload = firmware::qr::decode_qr(&data);
                        screen = Screen::ScanResult;
                        firmware::display::render_decoded_scan(&mut fb, &payload);
                        last_scan_data = Some(data);
                        scan_active = false;
                        break;
                    }
                }

                if let Some(ref mut t) = touch_ctrl {
                    if let Ok(status) = t.td_status(&mut touch_i2c) {
                        if status > 0 && !touch_active {
                            touch_active = true;
                            if let Ok(tp) = t.get_touch(&mut touch_i2c, 1) {
                                if tp.detected && back_btn.hit(tp.x, tp.y) {
                                    screen = Screen::Home;
                                    scan_active = false;
                                    firmware::display::render_home(&mut fb, scanner_connected);
                                }
                            }
                        } else if status == 0 {
                            touch_active = false;
                        }
                    }
                }
            }
            Screen::ScanResult => {
                if let Some(ref mut t) = touch_ctrl {
                    if let Ok(status) = t.td_status(&mut touch_i2c) {
                        if status > 0 && !touch_active {
                            touch_active = true;
                            if let Ok(tp) = t.get_touch(&mut touch_i2c, 1) {
                                if tp.detected && back_btn.hit(tp.x, tp.y) {
                                    screen = Screen::Home;
                                    firmware::display::render_home(&mut fb, scanner_connected);
                                }
                            }
                        } else if status == 0 {
                            touch_active = false;
                        }
                    }
                }
            }
        }
    }
}

fn handle_command(
    command: Command,
    payload: &[u8],
    state: &mut FirmwareState,
    rng: &mut Rng,
    fb: &mut LtdcFramebuffer<u16>,
    scanner: &mut Gm65Scanner<Serial6>,
    last_scan_data: &mut Option<Vec<u8>>,
) -> Response {
    match command {
        Command::ImportToken => handle_import_token(payload, state, fb),
        Command::GetTokenInfo => handle_get_token_info(state),
        Command::GetBlinded => handle_get_blinded(state, rng, fb),
        Command::SendSignatures => handle_send_signatures(payload, state, fb),
        Command::GetProofs => handle_get_proofs(state),
        Command::ScannerStatus => handle_scanner_status(scanner),
        Command::ScannerTrigger => handle_scanner_trigger(scanner, fb),
        Command::ScannerData => handle_scanner_data(scanner, fb, last_scan_data),
    }
}

fn handle_import_token(
    payload: &[u8],
    state: &mut FirmwareState,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    defmt::info!("IMPORT_TOKEN: {} bytes", payload.len());

    let token = match decode_token(payload) {
        Ok(t) => t,
        Err(_) => {
            defmt::error!("Token decode failed");
            firmware::display::render_error(fb, "Invalid token");
            return Response::new(Status::InvalidPayload);
        }
    };

    firmware::display::render_token_info(fb, &token);

    state.imported_token = Some(token);
    state.swap_state = SwapState::TokenImported;

    defmt::info!("Token imported successfully");
    Response::new(Status::Ok)
}

fn handle_get_token_info(state: &mut FirmwareState) -> Response {
    defmt::info!("GET_TOKEN_INFO");

    match &state.imported_token {
        Some(token) => {
            let mint = token.mint.as_bytes();
            let unit = token.unit.as_bytes();
            let total_len = 1 + mint.len() + 1 + unit.len() + 8 + 4;

            if total_len > firmware::usb::MAX_PAYLOAD_SIZE {
                return Response::new(Status::BufferOverflow);
            }

            let mut payload = [0u8; firmware::usb::MAX_PAYLOAD_SIZE];
            let mut offset = 0;

            payload[offset] = mint.len() as u8;
            offset += 1;
            payload[offset..offset + mint.len()].copy_from_slice(mint);
            offset += mint.len();

            payload[offset] = unit.len() as u8;
            offset += 1;
            payload[offset..offset + unit.len()].copy_from_slice(unit);
            offset += unit.len();

            let amount = token.total_amount();
            payload[offset..offset + 8].copy_from_slice(&amount.to_be_bytes());
            offset += 8;

            let count = token.proof_count() as u32;
            payload[offset..offset + 4].copy_from_slice(&count.to_be_bytes());
            offset += 4;

            Response::with_payload(Status::Ok, &payload[..offset])
                .unwrap_or_else(|| Response::new(Status::BufferOverflow))
        }
        None => {
            defmt::warn!("No token imported");
            Response::new(Status::Error)
        }
    }
}

fn handle_get_blinded(
    state: &mut FirmwareState,
    rng: &mut Rng,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    defmt::info!("GET_BLINDED");

    let token = match &state.imported_token {
        Some(t) => t,
        None => {
            defmt::warn!("No token imported");
            return Response::new(Status::Error);
        }
    };

    let mut blinded_messages: Vec<BlindedMessage> = Vec::new();
    let mut secrets: Vec<Vec<u8>> = Vec::new();
    let mut amounts: Vec<u64> = Vec::new();

    for token_part in &token.tokens {
        for proof in &token_part.proofs {
            let secret_bytes = match decode_hex(&proof.secret) {
                Some(s) => s,
                None => {
                    defmt::error!("Invalid secret hex");
                    continue;
                }
            };

            let mut blinder_bytes = [0u8; 32];
            rng.fill_bytes(&mut blinder_bytes);
            let blinder = match SecretKey::from_slice(&blinder_bytes) {
                Ok(sk) => sk,
                Err(_) => {
                    defmt::error!("Invalid blinder");
                    continue;
                }
            };

            let blinded = match blind_message(&secret_bytes, Some(blinder)) {
                Ok(b) => b,
                Err(_) => {
                    defmt::error!("Blinding failed");
                    continue;
                }
            };

            secrets.push(secret_bytes);
            amounts.push(proof.amount);
            blinded_messages.push(blinded);
        }
    }

    if blinded_messages.is_empty() {
        defmt::error!("No valid proofs to blind");
        return Response::new(Status::CryptoError);
    }

    let total_len = blinded_messages.len() * 33;
    if total_len > firmware::usb::MAX_PAYLOAD_SIZE {
        defmt::error!("Too many blinded outputs");
        return Response::new(Status::BufferOverflow);
    }

    let mut payload = [0u8; firmware::usb::MAX_PAYLOAD_SIZE];
    let mut offset = 0;

    for blinded in &blinded_messages {
        let bytes = blinded.blinded.to_encoded_point(false);
        let bytes = bytes.as_bytes();
        if bytes.len() == 65 {
            payload[offset..offset + 33].copy_from_slice(&bytes[1..34]);
        } else {
            payload[offset..offset + 33].copy_from_slice(&bytes[..33]);
        }
        offset += 33;
    }

    state.blinded_messages = Some(blinded_messages);
    state.swap_secrets = Some(secrets);
    state.swap_amounts = Some(amounts);
    state.swap_state = SwapState::BlindedGenerated;

    firmware::display::render_status(fb, "Blinded outputs ready");

    defmt::info!("Generated {} blinded outputs", offset / 33);
    Response::with_payload(Status::Ok, &payload[..offset])
        .unwrap_or_else(|| Response::new(Status::BufferOverflow))
}

fn handle_send_signatures(
    payload: &[u8],
    state: &mut FirmwareState,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    defmt::info!("SEND_SIGNATURES: {} bytes", payload.len());

    let blinded_messages = match &state.blinded_messages {
        Some(bm) => bm.clone(),
        None => {
            defmt::error!("No blinded messages stored");
            return Response::new(Status::Error);
        }
    };

    if payload.len() % 33 != 0 {
        defmt::error!("Invalid signature payload length");
        return Response::new(Status::InvalidPayload);
    }

    let sig_count = payload.len() / 33;
    if sig_count != blinded_messages.len() {
        defmt::error!(
            "Signature count mismatch: expected {}, got {}",
            blinded_messages.len(),
            sig_count
        );
        return Response::new(Status::InvalidPayload);
    }

    let mint_pubkey = match derive_demo_mint_key(&state.imported_token) {
        Ok(pk) => pk,
        Err(_) => {
            defmt::error!("Failed to derive mint key");
            return Response::new(Status::CryptoError);
        }
    };

    let mut proofs: Vec<Proof> = Vec::new();
    let keyset_id = state
        .imported_token
        .as_ref()
        .and_then(|t| t.tokens.first())
        .map(|t| t.keyset_id.clone())
        .unwrap_or_else(|| "00".to_string());

    for (i, blinded) in blinded_messages.iter().enumerate() {
        let sig_bytes = &payload[i * 33..(i + 1) * 33];
        let mut full_bytes = [0u8; 65];
        full_bytes[0] = 0x04;
        full_bytes[1..34].copy_from_slice(sig_bytes);
        full_bytes[34..].copy_from_slice(&sig_bytes[1..32]);

        let blinded_sig = match PublicKey::from_sec1_bytes(&full_bytes[..65]) {
            Ok(pk) => pk,
            Err(_) => {
                let compressed: [u8; 33] = {
                    let mut arr = [0u8; 33];
                    arr.copy_from_slice(sig_bytes);
                    arr
                };
                match PublicKey::from_sec1_bytes(&compressed) {
                    Ok(pk) => pk,
                    Err(_) => {
                        defmt::error!("Invalid signature at index {}", i);
                        continue;
                    }
                }
            }
        };

        let unblinded = match unblind_signature(&blinded_sig, &blinded.blinder, &mint_pubkey) {
            Ok(pk) => pk,
            Err(_) => {
                defmt::error!("Unblind failed at index {}", i);
                continue;
            }
        };

        let secret = &state.swap_secrets.as_ref().unwrap()[i];
        let amount = state.swap_amounts.as_ref().unwrap()[i];

        let c_bytes = unblinded.to_encoded_point(false);
        let c_vec = c_bytes.as_bytes().to_vec();

        proofs.push(Proof {
            amount,
            keyset_id: keyset_id.clone(),
            secret: encode_hex(secret),
            c: c_vec,
        });
    }

    if proofs.is_empty() {
        defmt::error!("No valid proofs generated");
        return Response::new(Status::CryptoError);
    }

    state.new_proofs = Some(proofs);
    state.swap_state = SwapState::ProofsReady;

    firmware::display::render_status(fb, "Proofs ready");

    defmt::info!("Unblinded {} signatures successfully", sig_count);
    Response::new(Status::Ok)
}

fn handle_get_proofs(state: &mut FirmwareState) -> Response {
    defmt::info!("GET_PROOFS");

    let proofs = match &state.new_proofs {
        Some(p) => p,
        None => {
            defmt::warn!("No proofs ready");
            return Response::new(Status::Error);
        }
    };

    let token = match &state.imported_token {
        Some(t) => t,
        None => {
            defmt::warn!("No token imported");
            return Response::new(Status::Error);
        }
    };

    let new_token = TokenV4 {
        mint: token.mint.clone(),
        unit: token.unit.clone(),
        memo: Some("Swapped via Micronuts".to_string()),
        tokens: alloc::vec![TokenV4Token {
            keyset_id: proofs
                .first()
                .map(|p| p.keyset_id.clone())
                .unwrap_or_else(|| "00".to_string()),
            proofs: proofs.clone(),
        }],
    };

    let encoded = match encode_token(&new_token) {
        Ok(e) => e,
        Err(_) => {
            defmt::error!("Token encoding failed");
            return Response::new(Status::Error);
        }
    };

    if encoded.len() > firmware::usb::MAX_PAYLOAD_SIZE {
        defmt::error!("Encoded token too large");
        return Response::new(Status::BufferOverflow);
    }

    defmt::info!(
        "Exporting {} proofs ({} bytes)",
        proofs.len(),
        encoded.len()
    );
    Response::with_payload(Status::Ok, &encoded)
        .unwrap_or_else(|| Response::new(Status::BufferOverflow))
}

fn handle_scanner_status(scanner: &mut Gm65Scanner<Serial6>) -> Response {
    defmt::info!("SCANNER_STATUS");
    let status = scanner.status();
    let mut payload = [0u8; firmware::usb::MAX_PAYLOAD_SIZE];
    let mut offset = 0;

    payload[offset] = if status.connected { 1 } else { 0 };
    offset += 1;
    payload[offset] = if status.initialized { 1 } else { 0 };
    offset += 1;

    let model_byte: u8 = match status.model {
        firmware::qr::ScannerModel::Gm65 => 0x01,
        firmware::qr::ScannerModel::M3Y => 0x02,
        firmware::qr::ScannerModel::Generic => 0x03,
        firmware::qr::ScannerModel::Unknown => 0x00,
    };
    payload[offset] = model_byte;
    offset += 1;

    Response::with_payload(Status::Ok, &payload[..offset])
        .unwrap_or_else(|| Response::new(Status::Error))
}

fn handle_scanner_trigger(
    scanner: &mut Gm65Scanner<Serial6>,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    defmt::info!("SCANNER_TRIGGER");
    match scanner.trigger_scan() {
        Ok(()) => {
            firmware::display::render_status(fb, "Scanning...");
            Response::new(Status::Ok)
        }
        Err(_) => {
            firmware::display::render_error(fb, "Scanner error");
            Response::new(Status::ScannerNotConnected)
        }
    }
}

fn handle_scanner_data(
    _scanner: &mut Gm65Scanner<Serial6>,
    fb: &mut LtdcFramebuffer<u16>,
    last_scan_data: &mut Option<Vec<u8>>,
) -> Response {
    defmt::info!("SCANNER_DATA");
    match last_scan_data.take() {
        Some(data) => {
            defmt::info!("Returning buffered scan data: {} bytes", data.len());
            let payload = firmware::qr::decode_qr(&data);
            firmware::display::render_decoded_scan(fb, &payload);
            let type_byte: u8 = match &payload {
                firmware::qr::QrPayload::CashuV4 { .. } => 0x01,
                firmware::qr::QrPayload::CashuV3 { .. } => 0x02,
                firmware::qr::QrPayload::UrFragment { .. } => 0x03,
                firmware::qr::QrPayload::PlainText(_) => 0x00,
                firmware::qr::QrPayload::Binary(_) => 0x04,
            };
            let max_payload = firmware::usb::MAX_PAYLOAD_SIZE;
            let total = 1 + data.len().min(max_payload - 1);
            let mut buf = alloc::vec![type_byte; total];
            buf[1..].copy_from_slice(&data[..total - 1]);
            Response::with_payload(Status::Ok, &buf)
                .unwrap_or_else(|| Response::new(Status::BufferOverflow))
        }
        None => {
            defmt::info!("No scan data available");
            Response::new(Status::NoScanData)
        }
    }
}

fn derive_demo_mint_key(token: &Option<TokenV4>) -> Result<PublicKey, ()> {
    let mint_url = token
        .as_ref()
        .map(|t| t.mint.as_str())
        .unwrap_or("demo://micronuts");

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    Digest::update(&mut hasher, mint_url.as_bytes());
    let seed = Digest::finalize(hasher);

    let sk = SecretKey::from_slice(&seed).map_err(|_| ())?;
    Ok(sk.public_key())
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut result = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        result.push(byte);
    }
    Some(result)
}

fn encode_hex(bytes: &[u8]) -> alloc::string::String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut result = alloc::string::String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX_CHARS[(*byte >> 4) as usize] as char);
        result.push(HEX_CHARS[(*byte & 0x0F) as usize] as char);
    }
    result
}

use cortex_m::peripheral::DWT;
