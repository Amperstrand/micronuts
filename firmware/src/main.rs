#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "defmt-log")]
use defmt_rtt as _;
#[cfg(feature = "defmt-log")]
use panic_probe as _;
#[cfg(feature = "uart-log")]
use panic_halt as _;

use embassy_executor::Spawner;
use embassy_stm32::interrupt::InterruptExt;
use embassy_stm32::{bind_interrupts, peripherals, usb, usart};
use embassy_time::{Duration, Ticker};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, UsbDevice};

use embassy_stm32f469i_disco::{BoardHint, DisplayCtrl, SdramCtrl, FB_HEIGHT, FB_WIDTH};

use firmware::boot_splash;
use firmware::hardware_impl::{FirmwareHardware, RawFramebuffer, UsbDriverType};
use firmware::self_test;
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver};
use linked_list_allocator::LockedHeap;

use static_cell::StaticCell;

pub use firmware::{log_error, log_info, log_warn};

const HEAP_SIZE: usize = 128 * 1024;
const FB_SIZE: usize = (FB_WIDTH as usize) * (FB_HEIGHT as usize);

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[inline(always)]
unsafe fn clear_ltdc_irq_flags() {
    stm32_metapac::LTDC.icr().write(|w| {
        w.set_clif(stm32_metapac::ltdc::vals::Clif::CLEAR);
        w.set_cfuif(stm32_metapac::ltdc::vals::Cfuif::CLEAR);
        w.set_cterrif(stm32_metapac::ltdc::vals::Cterrif::CLEAR);
        w.set_crrif(stm32_metapac::ltdc::vals::Crrif::CLEAR);
    });
}

#[inline(always)]
unsafe fn nop_irq() {
    cortex_m::asm::nop();
}

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
    HASH_RNG => embassy_stm32::rng::InterruptHandler<peripherals::RNG>;
});

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() {
    clear_ltdc_irq_flags();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC_ER() {
    clear_ltdc_irq_flags();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSI() {
    nop_irq();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSIHOST() {
    nop_irq();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() {
    nop_irq();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() {
    nop_irq();
}

#[embassy_executor::task]
async fn usb_task(mut usb_dev: UsbDevice<'static, UsbDriverType>) {
    usb_dev.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut p = embassy_stm32::init(embassy_stm32f469i_disco::config_180());

    let sdram = SdramCtrl::new(&mut p, embassy_stm32f469i_disco::SYSCLK_HZ_180);
    let sdram_base = sdram.base_address();
    let heap_start = sdram_base + (FB_SIZE * 2 * core::mem::size_of::<u32>());

    crate::log_info!("SDRAM quick test...");
    let sdram_ok = sdram.test_quick();
    if sdram_ok {
        crate::log_info!("SDRAM quick test: PASS");
    } else {
        crate::log_error!("SDRAM quick test: FAIL — display may be unreliable");
    }

    let sdram_bytes = sdram.into_bytes();
    let fb_size_bytes = FB_SIZE * core::mem::size_of::<u32>();
    let (display_bytes, rest) = sdram_bytes.split_at_mut(fb_size_bytes);
    let (fb1_bytes, _) = rest.split_at_mut(fb_size_bytes);

    crate::log_info!("Micronuts firmware starting (embassy)...");
    crate::log_info!("SDRAM initialized");

    let rng = embassy_stm32::rng::Rng::new(p.RNG, Irqs);

    unsafe {
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
    crate::log_info!("Heap: {} bytes from SDRAM", HEAP_SIZE);

    crate::log_info!("Initializing display...");
    #[allow(unexpected_cfgs)]
    #[cfg(not(rust_analyzer))]
    let display = DisplayCtrl::new(display_bytes, p.LTDC, p.DSIHOST, p.PJ2, p.PH7, BoardHint::ForceNt35510);
    #[allow(unexpected_cfgs)]
    #[cfg(rust_analyzer)]
    let display: DisplayCtrl = loop {};
    crate::log_info!("Display initialized");

    let fb0: &'static mut [u32] = unsafe { core::slice::from_raw_parts_mut(sdram_base as *mut u32, FB_SIZE) };
    let fb1: &'static mut [u32] = unsafe {
        core::slice::from_raw_parts_mut(fb1_bytes.as_mut_ptr() as *mut u32, FB_SIZE)
    };
    // Keep the display controller alive so DSI/LTDC keep scanning out of SDRAM while the app
    // writes the framebuffer directly instead of going through DisplayCtrl.
    core::mem::forget(display);
    let mut fb = RawFramebuffer::new_double(fb0, fb1);

    crate::log_info!("Initializing touch...");
    let mut touch_i2c = embassy_stm32::i2c::I2c::new_blocking(
        p.I2C1,
        p.PB8,
        p.PB9,
        embassy_stm32::i2c::Config::default(),
    );
    let _ = touch_i2c.blocking_write(0x38, &[0xA4, 0x01]);
    let mut touch_ctrl = embassy_stm32f469i_disco::touch::TouchCtrl::new(touch_i2c);
    let touch_available = touch_ctrl.read_vendor_id().is_ok();
    if touch_available {
        crate::log_info!("Touch controller ready (G_MODE=interrupt trigger)");
    } else {
        crate::log_warn!("Touch controller not found");
    }

    {
        crate::log_info!("Running boot splash...");
        let mut splash_state = boot_splash::SplashState::new();
        let mut splash_done = false;
        const MAX_SPLASH_FRAMES: u32 = 2 * 3 * 90;
        let mut ticker = Ticker::every(Duration::from_millis(33));
        while !splash_done {
            boot_splash::render_frame(
                fb.as_raw(),
                embassy_stm32f469i_disco::FB_WIDTH as u32,
                embassy_stm32f469i_disco::FB_HEIGHT as u32,
                &mut splash_state,
            );
            fb.present();

            ticker.next().await;

            if touch_available {
                if let Ok(status) = touch_ctrl.td_status() {
                    if status > 0 {
                        crate::log_info!("Touch detected, exiting splash");
                        splash_done = true;
                    }
                }
            }

            if splash_state.global_frame >= MAX_SPLASH_FRAMES {
                crate::log_info!("Splash timeout, continuing boot");
                splash_done = true;
            }
        }
        crate::log_info!("Boot splash complete");
    }

    crate::log_info!("Initializing USB...");

    // After a soft reset (SYSRESETREQ from st-flash), the USB OTG FS peripheral
    // can be left in an inconsistent state where the PHY doesn't re-enumerate.
    // Cycling the RCC clock + core soft reset + PHY power cycle ensures a clean
    // start regardless of how we got here. See gm65-scanner #56, ccid-firmware-rs #15.
    {
        let rcc = stm32_metapac::RCC;

        // SAFETY: RCC register writes are side-effect-only, no aliasing with other refs.
        rcc.ahb2enr().modify(|w| w.set_usb_otg_fsen(false));
        cortex_m::asm::delay(100);
        rcc.ahb2enr().modify(|w| w.set_usb_otg_fsen(true));

        rcc.ahb2rstr().modify(|w| w.set_usb_otg_fsrst(true));
        cortex_m::asm::delay(100);
        rcc.ahb2rstr().modify(|w| w.set_usb_otg_fsrst(false));
        cortex_m::asm::delay(100);

        // SAFETY: USB_OTG_FS_GLOBAL base address 0x5000_0000 is fixed by silicon.
        // No other reference to this peripheral exists yet (driver created below).
        let otg_global = 0x5000_0000usize as *mut u32;
        unsafe {
            // GRSTCTL.AHBIDL (bit 31) — wait for AHB idle before reset
            let mut timeout = 100_000u32;
            while otg_global.add(0x010 / 4).read_volatile() & (1 << 31) == 0 {
                timeout -= 1;
                if timeout == 0 {
                    break;
                }
            }

            // GRSTCTL.CSRST (bit 0) — core soft reset, self-clearing
            otg_global.add(0x010 / 4).write_volatile(1);
            timeout = 100_000u32;
            while otg_global.add(0x010 / 4).read_volatile() & 1 != 0 {
                timeout -= 1;
                if timeout == 0 {
                    break;
                }
            }

            // GCCFG.PWRDWN (bit 16) — PHY power cycle
            otg_global.add(0x038 / 4).write_volatile(0);
            cortex_m::asm::delay(100);
            otg_global.add(0x038 / 4).write_volatile(1 << 16);
        }
    }

    static EP_OUT_BUFFER: StaticCell<[u8; 512]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUFFER.init([0u8; 512]);
    let mut usb_config = usb::Config::default();
    usb_config.vbus_detection = false;
    let usb_driver = usb::Driver::new_fs(
        p.USB_OTG_FS,
        Irqs,
        p.PA12,
        p.PA11,
        ep_out_buffer,
        usb_config,
    );

    let mut usb_config_desc = embassy_usb::Config::new(0x16c0, 0x27dd);
    usb_config_desc.manufacturer = Some("Micronuts");
    usb_config_desc.product = Some("Cashu Hardware Wallet");
    usb_config_desc.serial_number = Some("F4691");

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config_desc,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        MSOS_DESCRIPTOR.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    );
    static USB_STATE: StaticCell<State<'static>> = StaticCell::new();
    let usb_state = USB_STATE.init(State::new());

    let cdc = CdcAcmClass::new(&mut usb_builder, usb_state, 64);
    let usb_dev = usb_builder.build();

    let (usb_sender, usb_receiver) = cdc.split();

    spawner.spawn(usb_task(usb_dev).expect("usb task token"));

    crate::log_info!("USB CDC initialized");

    crate::log_info!("Initializing QR scanner (USART6)...");
    embassy_stm32::interrupt::USART6.disable();
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    let async_uart = firmware::hardware_impl::AsyncUart { inner: uart, uart_error_count: 0 };

    embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;
    crate::log_info!("Scanner UART ready (115200 baud, USART6 PG14=TX PG9=RX)");

    #[cfg(feature = "uart-log")]
    {
        let mut dbg_config = embassy_stm32::usart::Config::default();
        dbg_config.baudrate = 115200;
        let dbg_uart = embassy_stm32::usart::Uart::new_blocking(p.USART3, p.PD9, p.PD8, dbg_config)
            .unwrap();
        firmware::uart_log::init(dbg_uart);
    }

    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let scanner_connected = match scanner.init().await {
        Ok(model) => {
            crate::log_info!("QR scanner ready: {}", model);
            true
        }
        Err(e) => {
            crate::log_warn!("QR scanner init failed: {}", e);
            false
        }
    };

    crate::log_info!("Scanner state after init: connected={}", scanner_connected);

    let mut hw = FirmwareHardware::new(
        fb,
        scanner,
        usb_receiver,
        usb_sender,
        touch_ctrl,
        touch_available,
        rng,
        scanner_connected,
    );

    use micronuts_app::hardware::Scanner;
    crate::log_info!("--- Scanner register dump ---");
    hw.debug_dump_settings();
    crate::log_info!("--- End dump ---");

    self_test::run_all(&mut hw).await;

    crate::log_info!("Self-test complete, starting app...");
    let raw_buf = hw.fb.as_raw();
    for px in raw_buf.iter_mut() {
        *px = 0x00000000;
    }

    micronuts_app::run(&mut hw).await;
}
