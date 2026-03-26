use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

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
    fn trigger(&mut self) -> Result<(), ScanError>;
    fn try_read(&mut self) -> Option<alloc::vec::Vec<u8>>;
    fn stop(&mut self);
    fn is_connected(&self) -> bool;
    fn set_aim(&mut self, enabled: bool) -> Result<(), ScanError>;
    fn debug_dump_settings(&mut self);
}

pub trait MicronutsHardware: Scanner {
    type Display: DrawTarget<Color = Rgb565>;

    fn display(&mut self) -> &mut Self::Display;
    fn rng_fill_bytes(&mut self, dest: &mut [u8]);
    fn transport_poll(&mut self) -> Option<Frame>;
    fn transport_send(&mut self, response: &Response);
    fn touch_get(&mut self) -> Option<TouchPoint>;
    fn delay_ms(&mut self, ms: u32);
}
