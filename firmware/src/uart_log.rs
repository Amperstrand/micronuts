use core::fmt::{self, Write as _};

use embassy_stm32::{mode::Blocking, usart::Uart};
use embedded_io::Write as EmbeddedWrite;

pub struct UartLogger<U> {
    uart: U,
}

impl<U> UartLogger<U>
where
    U: EmbeddedWrite,
{
    pub fn new(uart: U) -> Self {
        Self { uart }
    }
}

impl<U> fmt::Write for UartLogger<U>
where
    U: EmbeddedWrite,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.uart.write_all(s.as_bytes()).ok();
        Ok(())
    }
}

static mut LOGGER: Option<UartLogger<Uart<'static, Blocking>>> = None;

#[defmt::global_logger]
struct DefmtFallbackLogger;

unsafe impl defmt::Logger for DefmtFallbackLogger {
    fn acquire() {}

    unsafe fn flush() {}

    unsafe fn release() {}

    unsafe fn write(_bytes: &[u8]) {}
}

#[defmt::panic_handler]
fn defmt_panic() -> ! {
    loop {}
}

defmt::timestamp!("{=u8}", 0);

pub fn init(uart: Uart<'static, Blocking>) {
    unsafe {
        LOGGER = Some(UartLogger::new(uart));
    }
}

pub fn write_log(args: fmt::Arguments<'_>) {
    unsafe {
        if let Some(ref mut logger) = LOGGER {
            let _ = write!(logger, "{}\r\n", args);
        }
    }
}
