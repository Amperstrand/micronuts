use alloc::vec::Vec;

use embassy_stm32::peripherals;
use embassy_stm32::rng::Rng;
use embassy_stm32f469i_disco::display::{FB_HEIGHT, FB_WIDTH};
use embassy_stm32f469i_disco::touch::{TouchCtrl, TouchPoint as BspTouchPoint};
use embassy_time::Duration;
use embassy_usb::class::cdc_acm::{Receiver, Sender};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
    Pixel,
};
use embedded_graphics::pixelcolor::RgbColor;
use embedded_graphics::prelude::IntoStorage;
use embedded_hal_02::blocking::serial::Write as _;

use gm65_scanner::ScannerDriver;

use micronuts_app::hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};
use micronuts_app::protocol::{Frame, FrameDecoder, Response, MAX_PAYLOAD_SIZE};

use crate::qr::Gm65ScannerAsync;

pub type UsbDriverType = embassy_stm32::usb::Driver<'static, peripherals::USB_OTG_FS>;

pub struct RawFramebuffer {
    buffer: &'static mut [u16],
}

impl RawFramebuffer {
    pub fn new(buffer: &'static mut [u16]) -> Self {
        Self { buffer }
    }

    pub fn as_raw(&mut self) -> &mut [u16] {
        self.buffer
    }
}

impl DrawTarget for RawFramebuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let x = coord.x as usize;
            let y = coord.y as usize;
            if x < FB_WIDTH as usize && y < FB_HEIGHT as usize {
                self.buffer[y * FB_WIDTH as usize + x] = color.into_storage();
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &embedded_graphics::primitives::Rectangle, color: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let top = area.top_left.y.max(0) as usize;
        let bottom = (area.top_left.y + area.size.height as i32).min(FB_HEIGHT as i32) as usize;
        let left = area.top_left.x.max(0) as usize;
        let right = (area.top_left.x + area.size.width as i32).min(FB_WIDTH as i32) as usize;

        let flat_color = color.into_iter().next().unwrap_or(Rgb565::BLACK);
        let raw = flat_color.into_storage();

        for y in top..bottom {
            for x in left..right {
                self.buffer[y * FB_WIDTH as usize + x] = raw;
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let raw = color.into_storage();
        for px in self.buffer.iter_mut() {
            *px = raw;
        }
        Ok(())
    }
}

impl OriginDimensions for RawFramebuffer {
    fn size(&self) -> Size {
        Size::new(FB_WIDTH as u32, FB_HEIGHT as u32)
    }
}

pub struct FirmwareHardware {
    pub fb: RawFramebuffer,
    pub scanner: Gm65ScannerAsync<AsyncUart<'static>>,
    pub usb_receiver: Receiver<'static, UsbDriverType>,
    pub usb_sender: Sender<'static, UsbDriverType>,
    pub decoder: FrameDecoder,
    pub encoder_buf: [u8; MAX_PAYLOAD_SIZE + 3],
    pub touch_ctrl: TouchCtrl,
    pub touch_i2c: embassy_stm32::i2c::I2c<'static, embassy_stm32::mode::Blocking, embassy_stm32::i2c::Master>,
    pub touch_available: bool,
    pub rng: Rng<'static, peripherals::RNG>,
    pub scanner_connected: bool,
}

impl FirmwareHardware {
    pub fn new(
        fb: RawFramebuffer,
        scanner: Gm65ScannerAsync<AsyncUart<'static>>,
        usb_receiver: Receiver<'static, UsbDriverType>,
        usb_sender: Sender<'static, UsbDriverType>,
        touch_ctrl: TouchCtrl,
        touch_i2c: embassy_stm32::i2c::I2c<'static, embassy_stm32::mode::Blocking, embassy_stm32::i2c::Master>,
        touch_available: bool,
        rng: Rng<'static, peripherals::RNG>,
        scanner_connected: bool,
    ) -> Self {
        Self {
            fb,
            scanner,
            usb_receiver,
            usb_sender,
            decoder: FrameDecoder::new(),
            encoder_buf: [0; MAX_PAYLOAD_SIZE + 3],
            touch_ctrl,
            touch_i2c,
            touch_available,
            rng,
            scanner_connected,
        }
    }
}

impl Scanner for FirmwareHardware {
    async fn trigger(&mut self) -> Result<(), ScanError> {
        self.scanner.trigger_scan().await.map_err(|_| ScanError::IoError)
    }

    fn try_read(&mut self) -> Option<Vec<u8>> {
        None
    }

    async fn read_scan(&mut self) -> Option<Vec<u8>> {
        self.scanner.read_scan().await
    }

    async fn stop(&mut self) {
        let _ = self.scanner.stop_scan().await;
        self.scanner.cancel_scan();
    }

    fn is_connected(&self) -> bool {
        self.scanner.status().connected
    }

    async fn set_aim(&mut self, enabled: bool) -> Result<(), ScanError> {
        use gm65_scanner::ScannerSettings;
        let settings = self
            .scanner
            .get_scanner_settings()
            .await
            .ok_or(ScanError::NotReady)?;
        let new_settings = if enabled {
            settings | ScannerSettings::AIM
        } else {
            settings & !(ScannerSettings::AIM)
        };
        if self.scanner.set_scanner_settings(new_settings).await {
            defmt::info!("Scanner aim: {}", if enabled { "ON" } else { "OFF" });
            Ok(())
        } else {
            Err(ScanError::IoError)
        }
    }

    fn debug_dump_settings(&mut self) {
        defmt::info!("Scanner connected: {}", self.scanner.status().connected);
        defmt::info!("Scanner model: {}", self.scanner.status().model);
    }
}

impl MicronutsHardware for FirmwareHardware {
    type Display = RawFramebuffer;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.fb
    }

    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest);
    }

    async fn transport_recv_frame(&mut self) -> Option<Frame> {
        let mut rx_buf = [0u8; 64];
        match self.usb_receiver.read_packet(&mut rx_buf).await {
            Ok(count) if count > 0 => self.decoder.decode(&rx_buf[..count]),
            _ => None,
        }
    }

    async fn transport_send(&mut self, response: &Response) {
        let len = response.encode(&mut self.encoder_buf);
        if len == 0 {
            return;
        }
        let _ = embedded_io_async::Write::write_all(&mut self.usb_sender, &self.encoder_buf[..len]).await;
        let _ = embedded_io_async::Write::flush(&mut self.usb_sender).await;
    }

    fn touch_get(&mut self) -> Option<TouchPoint> {
        if !self.touch_available {
            return None;
        }
        if let Ok(status) = self.touch_ctrl.td_status(&mut self.touch_i2c) {
            if status > 0 {
                if let Ok(BspTouchPoint { x, y }) = self.touch_ctrl.get_touch(&mut self.touch_i2c) {
                    defmt::info!("Touch: x={}, y={}", x, y);
                    return Some(TouchPoint {
                        x,
                        y,
                        detected: true,
                    });
                }
            }
        }
        None
    }

    async fn delay_ms(&mut self, ms: u32) {
        embassy_time::Timer::after(Duration::from_millis(ms as u64)).await;
    }
}

pub struct AsyncUart<'d> {
    pub inner: embassy_stm32::usart::Uart<'d, embassy_stm32::mode::Blocking>,
}

impl<'d> embedded_io::ErrorType for AsyncUart<'d> {
    type Error = embassy_stm32::usart::Error;
}

impl<'d> embedded_io_async::Read for AsyncUart<'d> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut total = 0usize;
        let yield_threshold = if buf.len() <= 8 { 2_000_000 } else { 100_000 };
        for slot in buf.iter_mut() {
            let mut spins = 0u32;
            loop {
                match embedded_hal_02::serial::Read::read(&mut self.inner) {
                    Ok(byte) => {
                        *slot = byte;
                        total += 1;
                        break;
                    }
                    Err(nb::Error::WouldBlock) => {
                        spins += 1;
                        if spins < yield_threshold {
                            continue;
                        }
                        embassy_time::Timer::after_micros(100).await;
                    }
                    Err(nb::Error::Other(_e)) => {
                        unsafe {
                            const USART6_BASE: usize = 0x4001_1400;
                            let _sr = core::ptr::read_volatile(USART6_BASE as *const u32);
                            let _dr = core::ptr::read_volatile((USART6_BASE + 0x04) as *const u32);
                        }
                        embassy_time::Timer::after_micros(10).await;
                    }
                }
            }
        }
        Ok(total)
    }
}

impl<'d> embedded_io_async::Write for AsyncUart<'d> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner.bwrite_all(buf)?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.bflush()
    }
}
