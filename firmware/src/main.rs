#![no_std]
#![no_main]

extern crate alloc;

use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::{bind_interrupts, interrupt::InterruptExt, peripherals, rcc::*, time::Hertz, usb, usart, Config};
use embassy_time::{Duration, Ticker};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, UsbDevice};

use embassy_stm32f469i_disco::display::{DisplayCtrl, SdramCtrl, FB_SIZE};

use firmware::boot_splash;
use firmware::hardware_impl::{AsyncUart, FirmwareHardware, RawFramebuffer, UsbDriverType};
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver};
use linked_list_allocator::LockedHeap;

use static_cell::StaticCell;

const HEAP_SIZE: usize = 128 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
    HASH_RNG => embassy_stm32::rng::InterruptHandler<peripherals::RNG>;
});

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() {
    cortex_m::asm::nop();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC_ER() {
    cortex_m::asm::nop();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSI() {
    cortex_m::asm::nop();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSIHOST() {
    cortex_m::asm::nop();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() {
    cortex_m::asm::nop();
}
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() {
    cortex_m::asm::nop();
}

#[embassy_executor::task]
async fn usb_task(mut usb_dev: UsbDevice<'static, UsbDriverType>) {
    usb_dev.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.mux.clk48sel = mux::Clk48sel::PLL1_Q;
    }
    let mut p = embassy_stm32::init(config);

    defmt::info!("Micronuts firmware starting (embassy)...");

    defmt::info!("Initializing SDRAM...");
    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    defmt::info!("SDRAM initialized");

    let rng = embassy_stm32::rng::Rng::new(p.RNG, Irqs);

    unsafe {
        let heap_start = sdram.base_address() + FB_SIZE * 4;
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
    defmt::info!("Heap: {} bytes from SDRAM", HEAP_SIZE);

    defmt::info!("Initializing display...");
    let display = DisplayCtrl::new(&sdram, p.PH7);
    defmt::info!("Display initialized");

    let fb_buffer: &'static mut [u16] = sdram.subslice_mut(0, FB_SIZE);
    core::mem::forget(display);

    defmt::info!("Initializing touch...");
    let mut touch_i2c = embassy_stm32::i2c::I2c::new_blocking(
        p.I2C1,
        p.PB8,
        p.PB9,
        embassy_stm32::i2c::Config::default(),
    );
    let touch_ctrl = embassy_stm32f469i_disco::touch::TouchCtrl::new();
    let touch_available = touch_ctrl
        .read_chip_id(&mut touch_i2c)
        .is_ok();
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
        let mut ticker = Ticker::every(Duration::from_millis(33));
        while !splash_done {
            boot_splash::render_frame(
                fb_buffer,
                embassy_stm32f469i_disco::FB_WIDTH as u32,
                embassy_stm32f469i_disco::FB_HEIGHT as u32,
                &mut splash_state,
            );

            ticker.next().await;

            if touch_available {
                if let Ok(status) = touch_ctrl.td_status(&mut touch_i2c) {
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

    defmt::info!("Initializing USB...");
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

    defmt::info!("USB CDC initialized");

    defmt::info!("Initializing QR scanner (USART6)...");
    embassy_stm32::interrupt::USART6.disable();

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();

    let async_uart = AsyncUart { inner: uart };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let scanner_connected = match scanner.init().await {
        Ok(model) => {
            defmt::info!("QR scanner ready: {}", model);
            true
        }
        Err(e) => {
            defmt::warn!("QR scanner init failed: {}", e);
            false
        }
    };

    defmt::info!("Scanner state after init: connected={}", scanner_connected);

    let mut hw = FirmwareHardware::new(
        RawFramebuffer::new(fb_buffer),
        scanner,
        usb_receiver,
        usb_sender,
        touch_ctrl,
        touch_i2c,
        touch_available,
        rng,
        scanner_connected,
    );

    use micronuts_app::hardware::Scanner;
    defmt::info!("--- Scanner register dump ---");
    hw.debug_dump_settings();
    defmt::info!("--- End dump ---");

    micronuts_app::run(&mut hw).await;
}
