//! SK6812 5×5 LED matrix for the M5 Atom (ESP32-PICO, data on GPIO27).
//!
//! Status vocabulary ( TollGate client spec ):
//! - Scanning   red blink    — no TollGate network in range
//! - Validating yellow pulse — TollGate found, joining/parsing the ad
//! - Active     green solid  — session live; the 5×5 doubles as a data
//!                             meter (pixels drain as allotment depletes)
//! - Paying     blue breathe — composing/submitting the token
//! - LowBalance purple       — wallet can't fund the next renewal
//!
//! Timing follows the bolty NeoPixel lane (10 MHz RMT ticks, WS2812
//! symbol widths — SK6812 accepts them; bench-verified there). Colors
//! are held far below full scale: a 5×5 at 255 saturates and the Atom
//! runs from a 500 mA USB budget.

use core::time::Duration;

use esp_idf_hal::gpio::Gpio27;
use esp_idf_hal::rmt::config::{TransmitConfig, TxChannelConfig};
use esp_idf_hal::rmt::encoder::{BytesEncoder, BytesEncoderConfig};
use esp_idf_hal::rmt::{PinState, Pulse, Symbol, TxChannelDriver};
use esp_idf_hal::units::FromValueType as _;

pub const LED_COUNT: usize = 25;

const BLINK_PERIOD_MS: u64 = 600;
const PULSE_PERIOD_MS: u64 = 900;
const BREATHE_PERIOD_MS: u64 = 1800;

type Rgb = (u8, u8, u8);

const RED: Rgb = (36, 0, 0);
const YELLOW: Rgb = (30, 22, 0);
const GREEN: Rgb = (0, 36, 0);
const BLUE: Rgb = (0, 0, 36);
const PURPLE: Rgb = (24, 0, 36);
const BLACK: Rgb = (0, 0, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomState {
    /// Red blink — scanning, no TollGate in range.
    Scanning,
    /// Yellow pulse — found, validating the kind-10021 ad.
    Validating,
    /// Green solid + data meter — session active.
    Active,
    /// Blue breathing — paying / renewing.
    Paying,
    /// Purple — balance below two renewals.
    LowBalance,
}

pub struct LedMatrix {
    tx: TxChannelDriver<'static>,
    bit0: Symbol,
    bit1: Symbol,
    state: AtomState,
    /// (remaining, total) allotment units — rendered as lit pixels.
    meter: Option<(u64, u64)>,
    now_ms: u64,
}

impl LedMatrix {
    pub fn new(pin: Gpio27<'static>) -> Result<Self, esp_idf_sys::EspError> {
        let config = TxChannelConfig {
            resolution: 10.MHz().into(),
            ..Default::default()
        };
        let tx = TxChannelDriver::new(pin, &config)?;

        let t0h =
            Pulse::new_with_duration(10.MHz().into(), PinState::High, Duration::from_nanos(350))?;
        let t0l =
            Pulse::new_with_duration(10.MHz().into(), PinState::Low, Duration::from_nanos(800))?;
        let t1h =
            Pulse::new_with_duration(10.MHz().into(), PinState::High, Duration::from_nanos(700))?;
        let t1l =
            Pulse::new_with_duration(10.MHz().into(), PinState::Low, Duration::from_nanos(600))?;
        Ok(Self {
            tx,
            bit0: Symbol::new(t0h, t0l),
            bit1: Symbol::new(t1h, t1l),
            state: AtomState::Scanning,
            meter: None,
            now_ms: 0,
        })
    }

    pub fn set_state(&mut self, state: AtomState) {
        if self.state != state {
            self.state = state;
        }
    }

    pub fn state(&self) -> AtomState {
        self.state
    }

    pub fn set_meter(&mut self, remaining: u64, total: u64) {
        self.meter = if total == 0 { None } else { Some((remaining, total)) };
    }

    pub fn clear_meter(&mut self) {
        self.meter = None;
    }

    /// Advance animations. `dt_ms` is elapsed since the previous tick.
    pub fn tick(&mut self, dt_ms: u64) {
        self.now_ms = self.now_ms.wrapping_add(dt_ms);
    }

    fn phase(&self, period_ms: u64) -> u64 {
        self.now_ms % period_ms
    }

    fn fill(&self, color: Rgb) -> [Rgb; LED_COUNT] {
        [color; LED_COUNT]
    }

    fn dim(color: Rgb, scale_percent: u64) -> Rgb {
        (
            (color.0 as u64 * scale_percent / 100) as u8,
            (color.1 as u64 * scale_percent / 100) as u8,
            (color.2 as u64 * scale_percent / 100) as u8,
        )
    }

    fn frame(&self) -> [Rgb; LED_COUNT] {
        match self.state {
            AtomState::Scanning => {
                let on = self.phase(BLINK_PERIOD_MS) < BLINK_PERIOD_MS / 2;
                self.fill(if on { RED } else { BLACK })
            }
            AtomState::Validating => {
                // triangle wave 0..100..0
                let p = self.phase(PULSE_PERIOD_MS) * 200 / PULSE_PERIOD_MS;
                let level = if p <= 100 { p } else { 200 - p };
                self.fill(Self::dim(YELLOW, 25 + level))
            }
            AtomState::Active => {
                let mut frame = self.fill(GREEN);
                if let Some((remaining, total)) = self.meter {
                    let lit = ((remaining.min(total) as u128 * LED_COUNT as u128 / total as u128)
                        as usize)
                        .min(LED_COUNT);
                    for pixel in frame.iter_mut().skip(lit) {
                        *pixel = BLACK;
                    }
                }
                frame
            }
            AtomState::Paying => {
                let p = self.phase(BREATHE_PERIOD_MS) * 200 / BREATHE_PERIOD_MS;
                let level = if p <= 100 { p } else { 200 - p };
                self.fill(Self::dim(BLUE, 15 + level))
            }
            AtomState::LowBalance => self.fill(PURPLE),
        }
    }

    /// Push the current frame to the strip (GRB byte order).
    pub fn render(&mut self) -> Result<(), esp_idf_sys::EspError> {
        let frame = self.frame();
        let mut buf = [0u8; LED_COUNT * 3];
        for (i, (r, g, b)) in frame.iter().enumerate() {
            buf[i * 3] = *g;
            buf[i * 3 + 1] = *r;
            buf[i * 3 + 2] = *b;
        }
        let encoder = BytesEncoder::with_config(&BytesEncoderConfig {
            bit0: self.bit0,
            bit1: self.bit1,
            msb_first: true,
            ..Default::default()
        })?;
        self.tx
            .send_and_wait(encoder, &buf, &TransmitConfig::default())
    }
}
