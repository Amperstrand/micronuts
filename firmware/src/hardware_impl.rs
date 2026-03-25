use alloc::vec::Vec;
use embedded_hal::delay::DelayNs;
use rand_core::RngCore;

use hal::i2c::I2c;
use hal::ltdc::LtdcFramebuffer;
use hal::pac::I2C1;
use hal::rng::Rng;
use hal::serial::Serial6;
use hal::timer::SysDelay;
use stm32f469i_disc::hal;
use usb_device::device::UsbDevice;
use usbd_serial::SerialPort;

use micronuts_app::hardware::{MicronutsHardware, TouchPoint};
use micronuts_app::protocol::{Frame, FrameDecoder, Response, MAX_PAYLOAD_SIZE};
use micronuts_app::qr::{ScannerModel, ScannerState};
use micronuts_app::state::ScannerInfo;

use crate::qr::Gm65Scanner;
use gm65_scanner::ScannerDriverSync;
use hal::otg_fs::UsbBusType;

pub struct FirmwareHardware<T> {
    pub fb: LtdcFramebuffer<u16>,
    pub scanner: Gm65Scanner<Serial6>,
    pub usb_dev: UsbDevice<'static, UsbBusType>,
    pub serial: SerialPort<'static, UsbBusType>,
    pub decoder: FrameDecoder,
    pub encoder_buf: [u8; MAX_PAYLOAD_SIZE + 3],
    pub touch_ctrl: Option<ft6x06::Ft6X06<I2c<I2C1>, T>>,
    pub touch_i2c: I2c<I2C1>,
    pub rng: Rng,
    pub delay: SysDelay,
    pub scanner_connected: bool,
}

impl<T> FirmwareHardware<T>
where
    T: embedded_hal_02::digital::v2::InputPin,
{
    pub fn new(
        fb: LtdcFramebuffer<u16>,
        scanner: Gm65Scanner<Serial6>,
        usb_dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
        touch_ctrl: Option<ft6x06::Ft6X06<I2c<I2C1>, T>>,
        touch_i2c: I2c<I2C1>,
        rng: Rng,
        delay: SysDelay,
        scanner_connected: bool,
    ) -> Self {
        Self {
            fb,
            scanner,
            usb_dev,
            serial,
            decoder: FrameDecoder::new(),
            encoder_buf: [0; MAX_PAYLOAD_SIZE + 3],
            touch_ctrl,
            touch_i2c,
            rng,
            delay,
            scanner_connected,
        }
    }
}

impl<T> MicronutsHardware for FirmwareHardware<T>
where
    T: embedded_hal_02::digital::v2::InputPin,
{
    type Display = LtdcFramebuffer<u16>;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.fb
    }

    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest);
    }

    fn scanner_trigger(&mut self) -> Result<(), ()> {
        self.scanner.trigger_scan().map_err(|_| ())
    }

    fn scanner_try_read(&mut self) -> Option<Vec<u8>> {
        self.scanner.try_read_scan()
    }

    fn scanner_status(&self) -> ScannerInfo {
        let status = self.scanner.status();
        ScannerInfo {
            model: match status.model {
                gm65_scanner::ScannerModel::Gm65 => ScannerModel::Gm65,
                gm65_scanner::ScannerModel::M3Y => ScannerModel::M3Y,
                gm65_scanner::ScannerModel::Generic => ScannerModel::Generic,
                gm65_scanner::ScannerModel::Unknown => ScannerModel::Unknown,
            },
            state: if status.connected {
                ScannerState::Ready
            } else {
                ScannerState::Uninitialized
            },
            last_scan_len: None,
            connected: status.connected,
        }
    }

    fn transport_poll(&mut self) -> Option<Frame> {
        self.usb_dev.poll(&mut [&mut self.serial]);
        let mut rx_buf = [0u8; 64];
        match self.serial.read(&mut rx_buf) {
            Ok(count) if count > 0 => self.decoder.decode(&rx_buf[..count]),
            _ => None,
        }
    }

    fn transport_send(&mut self, response: &Response) {
        let len = response.encode(&mut self.encoder_buf);
        if len == 0 {
            return;
        }
        let mut offset = 0;
        while offset < len {
            match self.serial.write(&self.encoder_buf[offset..len]) {
                Ok(written) if written > 0 => {
                    offset += written;
                }
                _ => {
                    let _ = self.serial.flush();
                }
            }
        }
        let _ = self.serial.flush();
    }

    fn touch_get(&mut self) -> Option<TouchPoint> {
        if let Some(ref mut t) = self.touch_ctrl {
            if let Ok(status) = t.td_status(&mut self.touch_i2c) {
                if status > 0 {
                    if let Ok(tp) = t.get_touch(&mut self.touch_i2c, 1) {
                        if tp.detected {
                            return Some(TouchPoint {
                                x: tp.x,
                                y: tp.y,
                                detected: true,
                            });
                        }
                    }
                }
            }
        }
        None
    }

    fn delay_ms(&mut self, ms: u32) {
        self.delay.delay_ms(ms);
    }
}
