use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb888;

use crate::protocol::{Frame, Response};

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
    pub detected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    NotConnected,
    NotReady,
    IoError,
}

pub trait Scanner {
    fn trigger(&mut self) -> impl core::future::Future<Output = Result<(), ScanError>>;
    fn read_scan(&mut self) -> impl core::future::Future<Output = Option<alloc::vec::Vec<u8>>>;
    fn stop(&mut self) -> impl core::future::Future<Output = ()>;
    fn is_connected(&self) -> bool;
    fn set_aim(
        &mut self,
        enabled: bool,
    ) -> impl core::future::Future<Output = Result<(), ScanError>>;
    fn debug_dump_settings(&mut self);
    /// Deep-sleep module reboot (crate-owned Tier-A heal): keeps settings
    /// and baud; the module wakes on the next UART activity.
    fn deep_sleep_reboot(&mut self) -> impl core::future::Future<Output = bool>;
    /// Re-init + re-apply the scan policy after a heal.
    fn reinit_scanner(&mut self) -> impl core::future::Future<Output = Result<(), ScanError>>;
    /// Last-resort heal: factory reset (module returns to 9600 baud —
    /// the host UART follows), re-init (restores 115200), policy restart.
    fn factory_heal(&mut self) -> impl core::future::Future<Output = Result<(), ScanError>>;
}

/// Contactless-reader capability mirroring [`Scanner`] for NFC tag
/// capture. The PN7160-class transport lives behind this seam: the
/// app-level commands (NfcPoll/NfcData/NfcHeal) and the token pipeline
/// are transport-agnostic, so a board only implements these four
/// methods to gain NFC token entry alongside the QR scanner.
pub trait NfcReader {
    fn nfc_is_connected(&self) -> bool;
    /// Turn the field on and report whether a tag answered.
    fn nfc_poll(&mut self) -> impl core::future::Future<Output = Result<bool, ScanError>>;
    /// Read the tag's NDEF payload bytes (None: no tag in field).
    fn nfc_read_ndef(&mut self) -> impl core::future::Future<Output = Option<alloc::vec::Vec<u8>>>;
    /// Reader re-init after a wedge.
    fn nfc_heal(&mut self) -> impl core::future::Future<Output = Result<(), ScanError>>;
}

pub trait MicronutsHardware: Scanner + NfcReader {
    type Display: DrawTarget<Color = Rgb888>;

    fn display(&mut self) -> &mut Self::Display;
    fn swap_buffers(&mut self) {}
    fn rng_fill_bytes(&mut self, dest: &mut [u8]);
    fn transport_recv_frame(&mut self) -> impl core::future::Future<Output = Option<Frame>>;
    fn transport_send(&mut self, response: &Response) -> impl core::future::Future<Output = ()>;
    fn touch_get(&mut self) -> Option<TouchPoint>;
    fn delay_ms(&mut self, ms: u32) -> impl core::future::Future<Output = ()>;
}
