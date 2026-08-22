#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "defmt-log")]
use defmt_rtt as _;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

use embassy_executor::Spawner;
use embassy_stm32::interrupt::InterruptExt;
use embassy_stm32::{bind_interrupts, peripherals, usart, usb};
use embassy_time::{Duration, Ticker};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, UsbDevice};

use embassy_stm32f469i_disco::display::{DisplayCtrl, FB_HEIGHT, FB_WIDTH};
use embassy_stm32f469i_disco::BoardHint;
use embassy_stm32f469i_disco::{BootTestResults, TestResult};

use embedded_graphics::{mono_font::MonoTextStyle, pixelcolor::Rgb888, prelude::*, text::Text};

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
    let p = embassy_stm32::init(embassy_stm32f469i_disco::config_180());

    let mut sdram = embassy_stm32f469i_disco::sdram_init!(p);
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
    let (_display_bytes, rest) = sdram_bytes.split_at_mut(fb_size_bytes);
    let (_fb1_bytes, _) = rest.split_at_mut(fb_size_bytes);

    crate::log_info!("Micronuts firmware starting (embassy)...");
    crate::log_info!("SDRAM initialized");

    let rng = embassy_stm32::rng::Rng::new(p.RNG, Irqs);

    unsafe {
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
    crate::log_info!("Heap: {} bytes from SDRAM", HEAP_SIZE);

    crate::log_info!("Initializing display...");
    let fb_size_bytes = FB_SIZE * core::mem::size_of::<u32>();
    // SAFETY: sdram_base points to valid 16MB SDRAM. DisplayCtrl uses the first
    // fb_size_bytes for the framebuffer. After forget(display), LTDC continues scanning.
    // fb0/fb1 alias the same memory for double-buffering.
    let framebuffer: &'static mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(sdram_base as *mut u8, fb_size_bytes) };
    #[allow(unexpected_cfgs)]
    #[cfg(not(rust_analyzer))]
    let display = DisplayCtrl::new(
        framebuffer,
        p.LTDC,
        p.DSIHOST,
        p.PJ2,
        p.PH7,
        BoardHint::ForceNt35510,
    );
    #[allow(unexpected_cfgs)]
    #[cfg(rust_analyzer)]
    let display: DisplayCtrl = loop {};
    crate::log_info!("Display initialized");

    // SAFETY: Same SDRAM region as framebuffer above, reinterpreted as u32 for
    // double-buffered pixel access. DisplayCtrl is forgotten below — LTDC keeps scanning.
    let fb0: &'static mut [u32] =
        unsafe { core::slice::from_raw_parts_mut(sdram_base as *mut u32, FB_SIZE) };
    let fb1: &'static mut [u32] = unsafe {
        core::slice::from_raw_parts_mut((sdram_base + fb_size_bytes) as *mut u32, FB_SIZE)
    };

    {
        use embedded_graphics::Drawable;

        let bist = BootTestResults {
            sdram: if sdram_ok {
                TestResult::Pass
            } else {
                TestResult::Fail
            },
            display: TestResult::Pass,
            touch_i2c: TestResult::Skip,
            touch_vendor_id: TestResult::Skip,
            touch_chip_model: TestResult::Skip,
            touch_idle: TestResult::Skip,
            leds: TestResult::Skip,
            user_button: TestResult::Skip,
        };

        const BG_DARK: u32 = 0xFF181818;

        for pixel in fb0.iter_mut() {
            *pixel = BG_DARK;
        }

        struct Argb8888Target<'a> {
            fb: &'a mut [u32],
            width: u32,
            height: u32,
        }

        impl<'a> Argb8888Target<'a> {
            fn new(fb: &'a mut [u32], width: u32, height: u32) -> Self {
                Self { fb, width, height }
            }
        }

        impl<'a> embedded_graphics::geometry::OriginDimensions for Argb8888Target<'a> {
            fn size(&self) -> Size {
                Size::new(self.width, self.height)
            }
        }

        impl<'a> DrawTarget for Argb8888Target<'a> {
            type Color = Rgb888;
            type Error = core::convert::Infallible;

            fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
            where
                I: IntoIterator<Item = Pixel<Rgb888>>,
            {
                for Pixel(pos, color) in pixels {
                    if pos.x >= 0
                        && pos.x < FB_WIDTH as i32
                        && pos.y >= 0
                        && pos.y < FB_HEIGHT as i32
                    {
                        let idx = (pos.y as usize) * FB_WIDTH as usize + (pos.x as usize);
                        if idx < self.fb.len() {
                            self.fb[idx] = ((color.r() as u32) << 16)
                                | ((color.g() as u32) << 8)
                                | (color.b() as u32)
                                | 0xFF000000;
                        }
                    }
                }
                Ok(())
            }
        }

        let font = &embedded_graphics::mono_font::ascii::FONT_10X20;

        let mut target = Argb8888Target::new(fb0, FB_WIDTH as u32, FB_HEIGHT as u32);
        let title_style = MonoTextStyle::new(font, Rgb888::WHITE);
        Text::new("BOOT SELF-TEST", Point::new(160, 60), title_style)
            .draw(&mut target)
            .unwrap();

        let entries = bist.entries();
        for (i, entry) in entries.iter().enumerate() {
            let y_offset = 120 + (i as i32) * 40;

            let name_style = MonoTextStyle::new(font, Rgb888::WHITE);
            Text::new(entry.name, Point::new(30, y_offset), name_style)
                .draw(&mut target)
                .unwrap();

            let result_text = match entry.result {
                TestResult::Pass => "PASS",
                TestResult::Fail => "FAIL",
                TestResult::Skip => "SKIP",
            };
            let result_color = match entry.result {
                TestResult::Pass => Rgb888::new(0, 255, 0),
                TestResult::Fail => Rgb888::new(255, 0, 0),
                TestResult::Skip => Rgb888::new(0, 255, 255),
            };
            let result_style = MonoTextStyle::new(font, result_color);
            Text::new(result_text, Point::new(400, y_offset), result_style)
                .draw(&mut target)
                .unwrap();
        }

        let summary = alloc::format!("{}/{} PASSED", bist.passed_count(), bist.total());
        let summary_style = MonoTextStyle::new(font, Rgb888::WHITE);
        Text::new(&summary, Point::new(190, 600), summary_style)
            .draw(&mut target)
            .unwrap();
    }

    embassy_time::Timer::after(embassy_time::Duration::from_millis(5000)).await;

    core::mem::forget(display);
    let mut fb = RawFramebuffer::new_double(fb0, fb1);

    crate::log_info!("Initializing touch...");
    let mut touch_i2c = embassy_stm32::i2c::I2c::new_blocking(
        p.I2C1,
        p.PB8,
        p.PB9,
        embassy_stm32::i2c::Config::default(),
    );
    let mut vendor_buf = [0u8; 1];
    let touch_available = touch_i2c
        .blocking_write_read(0x38, &[0xA8], &mut vendor_buf)
        .is_ok();
    if touch_available {
        let _ = touch_i2c.blocking_write(0x38, &[0xA4, 0x01]);
        crate::log_info!("Touch controller ready (G_MODE=interrupt trigger)");
    } else {
        crate::log_warn!("Touch controller not found");
    }
    let mut touch_ctrl = embassy_stm32f469i_disco::touch::TouchCtrl::new(touch_i2c);

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
    embassy_stm32f469i_disco::reset_usb_phy();

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
    let async_uart = firmware::hardware_impl::AsyncUart {
        inner: uart,
        uart_error_count: 0,
    };

    embassy_time::Timer::after(embassy_time::Duration::from_millis(500)).await;
    crate::log_info!("Scanner UART ready (115200 baud, USART6 PG14=TX PG9=RX)");

    #[cfg(feature = "uart-log")]
    {
        let mut dbg_config = embassy_stm32::usart::Config::default();
        dbg_config.baudrate = 115200;
        let dbg_uart =
            embassy_stm32::usart::Uart::new_blocking(p.USART3, p.PD9, p.PD8, dbg_config).unwrap();
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
