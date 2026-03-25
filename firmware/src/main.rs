#![no_std]
#![no_main]

extern crate alloc;

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use hal::otg_fs::UsbBus;
use hal::otg_fs::UsbBusType;
use hal::serial::Serial6;
use static_cell::ConstStaticCell;
use stm32f469i_disc::{
    hal,
    hal::ltdc::LtdcFramebuffer,
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    hal::rng::RngExt,
    lcd, sdram, touch, usb,
};
use usb_device::prelude::*;
use usbd_serial::SerialPort;

use firmware::boot_splash;
use firmware::hardware_impl::FirmwareHardware;
use firmware::qr::Gm65Scanner;
use gm65_scanner::ScannerDriverSync;

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

    let rng = dp.RNG.constrain(&mut rcc);

    let gpioa = dp.GPIOA.split(&mut rcc);
    let gpiob = dp.GPIOB.split(&mut rcc);
    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let gpiog = dp.GPIOG.split(&mut rcc);
    let gpioh = dp.GPIOH.split(&mut rcc);
    let gpioi = dp.GPIOI.split(&mut rcc);

    defmt::info!("Initializing SDRAM...");

    let (sdram_pins, remainders, ph7) =
        sdram::split_sdram_pins(gpioc, gpiod, gpioe, gpiof, gpiog, gpioh, gpioi);

    let ts_int = remainders.pc1.into_pull_down_input();
    let scanner_tx = remainders.pg14;
    let scanner_rx = remainders.pg9;

    let mut lcd_reset = ph7.into_push_pull_output();
    lcd_reset.set_low();
    delay.delay_ms(20u32);
    lcd_reset.set_high();
    delay.delay_ms(10u32);

    let mut sdram = sdram::Sdram::new(dp.FMC, sdram_pins, &rcc.clocks, &mut delay);

    let orientation = lcd::DisplayOrientation::Portrait;

    {
        const HEAP_SIZE: usize = 128 * 1024;
        let heap_start = sdram.mem as *mut u8;
        unsafe {
            let heap_ptr = heap_start.add(orientation.fb_size() * 4);
            ALLOCATOR.lock().init(heap_ptr as *mut u8, HEAP_SIZE);
        }
    }

    defmt::info!("Initializing display...");

    let (display_ctrl, _controller, _orientation) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::ForceNt35510,
        orientation,
    );

    let mut dbl_fb = lcd::DoubleFramebuffer::new(&mut sdram, display_ctrl, orientation);

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

    {
        defmt::info!("Running boot splash...");
        let mut splash_state = boot_splash::SplashState::new();
        let mut splash_done = false;
        const MAX_SPLASH_FRAMES: u32 = 2 * 3 * 90;
        while !splash_done {
            boot_splash::render_frame(
                dbl_fb.back_buffer(),
                orientation.width() as u32,
                orientation.height() as u32,
                &mut splash_state,
            );
            dbl_fb.swap();

            delay.delay_ms(33u32);

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

    let fb = LtdcFramebuffer::new(
        dbl_fb.into_front_buffer(),
        orientation.width(),
        orientation.height(),
    );

    defmt::info!("Initializing USB...");
    let usb_periph = usb::init(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        gpioa.pa11,
        gpioa.pa12,
        &rcc.clocks,
    );

    let usb_bus = UsbBus::new(usb_periph, EP_MEMORY.take());

    let serial: SerialPort<'static, UsbBusType> =
        unsafe { core::mem::transmute(usbd_serial::SerialPort::new(&usb_bus)) };

    let usb_dev: UsbDevice<'static, UsbBusType> = unsafe {
        core::mem::transmute(
            UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
                .device_class(usbd_serial::USB_CLASS_CDC)
                .strings(&[StringDescriptors::default()
                    .manufacturer("Micronuts")
                    .product("Cashu Hardware Wallet")
                    .serial_number("F4691")])
                .unwrap()
                .build(),
        )
    };

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
            let s = Gm65Scanner::with_default_config(uart);
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
                match scanner.init() {
                    Ok(_) => defmt::info!("UART re-init at 115200 bps complete"),
                    Err(e) => defmt::warn!("UART re-init at 115200 bps failed: {}", e),
                }
            }
        }
        Err(e) => defmt::warn!("QR scanner init failed: {}", e),
    }

    defmt::info!("USB initialized, entering main loop");

    let scanner_connected = scanner.state() == gm65_scanner::ScannerState::Ready;

    let mut hw = FirmwareHardware::new(
        fb,
        scanner,
        usb_dev,
        serial,
        touch_ctrl,
        touch_i2c,
        rng,
        delay,
        scanner_connected,
    );

    micronuts_app::run(&mut hw);
}
