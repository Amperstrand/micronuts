use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::protocol::{Frame, Response};
use crate::state::ScannerInfo;

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
    pub detected: bool,
}

pub trait MicronutsHardware {
    type Display: DrawTarget<Color = Rgb565>;

    fn display(&mut self) -> &mut Self::Display;
    fn rng_fill_bytes(&mut self, dest: &mut [u8]);
    fn scanner_trigger(&mut self) -> Result<(), ()>;
    fn scanner_try_read(&mut self) -> Option<alloc::vec::Vec<u8>>;
    fn scanner_status(&self) -> ScannerInfo;
    fn transport_poll(&mut self) -> Option<Frame>;
    fn transport_send(&mut self, response: &Response);
    fn touch_get(&mut self) -> Option<TouchPoint>;
    fn delay_ms(&mut self, ms: u32);
}
