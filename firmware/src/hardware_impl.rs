use alloc::vec::Vec;

use embassy_stm32::peripherals;
use embassy_stm32::rng::Rng;
use embassy_stm32f469i_disco::touch::{TouchCtrl, TouchPoint as BspTouchPoint};
use embassy_stm32f469i_disco::{FB_HEIGHT, FB_WIDTH};
use embassy_time::Duration;
use embassy_usb::class::cdc_acm::{Receiver, Sender};
use embedded_graphics::pixelcolor::RgbColor;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb888,
    Pixel,
};
use embedded_hal_02::blocking::serial::Write as _;
use sha2::Digest;

use gm65_scanner::ScannerDriver;

use micronuts_app::hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};
use micronuts_app::protocol::{Frame, FrameDecoder, Response, MAX_PAYLOAD_SIZE};

use crate::qr::Gm65ScannerAsync;

pub type UsbDriverType = embassy_stm32::usb::Driver<'static, peripherals::USB_OTG_FS>;

pub struct AsyncUart<'d> {
    pub inner: embassy_stm32::usart::Uart<'d, embassy_stm32::mode::Blocking>,
    pub uart_error_count: u32,
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
        for slot in buf.iter_mut() {
            loop {
                match embedded_hal_02::serial::Read::read(&mut self.inner) {
                    Ok(byte) => {
                        *slot = byte;
                        total += 1;
                        break;
                    }
                    Err(nb::Error::WouldBlock) => {
                        embassy_time::Timer::after_micros(10).await;
                    }
                    Err(nb::Error::Other(_e)) => {
                        self.uart_error_count = self.uart_error_count.saturating_add(1);
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

pub struct RawFramebuffer {
    buf0: &'static mut [u32],
    buf1: &'static mut [u32],
    front_is_0: bool,
}

impl RawFramebuffer {
    pub fn new_double(buf0: &'static mut [u32], buf1: &'static mut [u32]) -> Self {
        debug_assert_eq!(buf0.len(), buf1.len());
        buf1.copy_from_slice(buf0);
        Self {
            buf0,
            buf1,
            front_is_0: true,
        }
    }

    fn back_buffer(&mut self) -> &mut [u32] {
        if self.front_is_0 {
            self.buf1
        } else {
            self.buf0
        }
    }

    fn sync_back_buffer(&mut self) {
        if self.front_is_0 {
            self.buf1.copy_from_slice(self.buf0);
        } else {
            self.buf0.copy_from_slice(self.buf1);
        }
    }

    pub fn as_raw(&mut self) -> &mut [u32] {
        self.back_buffer()
    }

    pub fn present(&mut self) {
        self.front_is_0 = !self.front_is_0;
        let front_addr = if self.front_is_0 {
            self.buf0.as_ptr()
        } else {
            self.buf1.as_ptr()
        };

        stm32_metapac::LTDC
            .layer(0)
            .cfbar()
            .write(|w| w.set_cfbadd(front_addr as u32));
        stm32_metapac::LTDC
            .srcr()
            .write(|w| w.set_vbr(stm32_metapac::ltdc::vals::Vbr::RELOAD));

        self.sync_back_buffer();
    }
}

impl DrawTarget for RawFramebuffer {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let x = coord.x as usize;
            let y = coord.y as usize;
            if x < FB_WIDTH as usize && y < FB_HEIGHT as usize {
                let raw = 0xFF000000
                    | ((color.r() as u32) << 16)
                    | ((color.g() as u32) << 8)
                    | (color.b() as u32);
                self.back_buffer()[y * FB_WIDTH as usize + x] = raw;
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        color: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let top = area.top_left.y.max(0) as usize;
        let bottom = (area.top_left.y + area.size.height as i32).min(FB_HEIGHT as i32) as usize;
        let left = area.top_left.x.max(0) as usize;
        let right = (area.top_left.x + area.size.width as i32).min(FB_WIDTH as i32) as usize;

        let flat_color = color.into_iter().next().unwrap_or(Rgb888::BLACK);
        let raw = 0xFF000000
            | ((flat_color.r() as u32) << 16)
            | ((flat_color.g() as u32) << 8)
            | (flat_color.b() as u32);
        let buffer = self.back_buffer();

        for y in top..bottom {
            let row = &mut buffer[y * FB_WIDTH as usize + left..y * FB_WIDTH as usize + right];
            row.fill(raw);
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let raw = 0xFF000000
            | ((color.r() as u32) << 16)
            | ((color.g() as u32) << 8)
            | (color.b() as u32);
        for px in self.back_buffer().iter_mut() {
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
            touch_available,
            rng,
            scanner_connected,
        }
    }
}

impl Scanner for FirmwareHardware {
    async fn trigger(&mut self) -> Result<(), ScanError> {
        self.scanner
            .trigger_scan()
            .await
            .map_err(|_| ScanError::IoError)
    }

    async fn read_scan(&mut self) -> Option<Vec<u8>> {
        let data = self.scanner.read_scan().await?;
        crate::log_info!("SCAN: {} bytes", data.len());
        let preview_len = data.len().min(40);
        crate::log_info!("SCAN head: {:?}", &data[..preview_len]);
        Some(data)
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
            crate::log_info!("Scanner aim: {}", if enabled { "ON" } else { "OFF" });
            Ok(())
        } else {
            Err(ScanError::IoError)
        }
    }

    fn debug_dump_settings(&mut self) {
        crate::log_info!("Scanner connected: {}", self.scanner.status().connected);
        crate::log_info!("Scanner model: {}", self.scanner.status().model);
    }
}

impl MicronutsHardware for FirmwareHardware {
    type Display = RawFramebuffer;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.fb
    }

    fn swap_buffers(&mut self) {
        self.fb.present();
    }

    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        if dest.is_empty() {
            return;
        }
        let mut hasher = sha2::Sha256::new();
        let mut offset = 0;
        while offset < dest.len() {
            let mut raw = [0u8; 32];
            self.rng.fill_bytes(&mut raw);
            hasher.update(&raw);
            let hash = hasher.finalize_reset();
            let to_copy = core::cmp::min(32, dest.len() - offset);
            dest[offset..offset + to_copy].copy_from_slice(&hash[..to_copy]);
            offset += to_copy;
        }
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
        let _ =
            embassy_stm32f469i_disco::send_with_zlp(&mut self.usb_sender, &self.encoder_buf[..len])
                .await;
    }

    fn touch_get(&mut self) -> Option<TouchPoint> {
        if !self.touch_available {
            return None;
        }
        // get_touch() internally checks td_status(); returns Ok(None) if no touch.
        if let Ok(Some(BspTouchPoint { x, y })) = self.touch_ctrl.get_touch() {
            crate::log_info!("Touch: x={}, y={}", x, y);
            return Some(TouchPoint {
                x,
                y,
                detected: true,
            });
        }
        None
    }

    async fn delay_ms(&mut self, ms: u32) {
        embassy_time::Timer::after(Duration::from_millis(ms as u64)).await;
    }
}
