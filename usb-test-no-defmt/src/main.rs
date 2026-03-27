#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::{bind_interrupts, peripherals, rcc::*, time::Hertz, usb, Config};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::Builder;

use static_cell::StaticCell;

type UsbDriverType = embassy_stm32::usb::Driver<'static, peripherals::USB_OTG_FS>;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[embassy_executor::task]
async fn usb_task(mut usb_dev: embassy_usb::UsbDevice<'static, UsbDriverType>) {
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
    let p = embassy_stm32::init(config);

    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUFFER.init([0u8; 1024]);
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
    usb_config_desc.product = Some("USB Test (no-defmt)");
    usb_config_desc.serial_number = Some("NODEFMT");

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config_desc,
        CONFIG_DESCRIPTOR.init([0u8; 256]),
        BOS_DESCRIPTOR.init([0u8; 256]),
        MSOS_DESCRIPTOR.init([0u8; 256]),
        CONTROL_BUF.init([0u8; 64]),
    );
    static USB_STATE: StaticCell<State<'static>> = StaticCell::new();
    let usb_state = USB_STATE.init(State::new());

    let cdc = CdcAcmClass::new(&mut usb_builder, usb_state, 64);
    let usb_dev = usb_builder.build();

    let (mut sender, mut receiver) = cdc.split();

    let token = usb_task(usb_dev);
    match token {
        Ok(t) => spawner.spawn(t),
        Err(_) => {}
    }

    let mut rx_buf = [0u8; 256];
    loop {
        match receiver.read_packet(&mut rx_buf).await {
            Ok(n) => {
                let _ = sender.write_packet(&rx_buf[..n]).await;
            }
            Err(_) => {}
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        cortex_m::asm::nop();
    }
}
