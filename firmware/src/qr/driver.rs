//! QR Scanner Hardware Driver
//!
//! Low-level driver for GM65/M3Y QR scanner modules connected via UART.
//! Protocol modeled after specter-diy's qr.py (https://github.com/cryptoadvance/specter-diy).
//!
//! # Hardware Configuration
//!
//! - UART: USART6 (PG14=TX, PG9=RX via shield-lite Arduino D0/D1)
//! - Baud: 9600 (default GM65) -> 115200 (after config)
//!
//! # Protocol
//!
//! Command format: `7E 00 [type:1] [len:1] [addr_lo] [addr_hi] [value:N] AB CD`
//! Response format: `02 00 00 01 [value_byte] 33 31` (7 bytes for get_setting)

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use embedded_hal_02::blocking::serial::Write as BlockingWrite;
use embedded_hal_02::serial::{Read, Write};

pub const MAX_SCAN_SIZE: usize = 2048;

const GM65_HEADER: [u8; 2] = [0x7E, 0x00];
const GM65_CRC_NO_CHECKSUM: [u8; 2] = [0xAB, 0xCD];
const GM65_SUCCESS_PREFIX: [u8; 4] = [0x02, 0x00, 0x00, 0x01];
const GM65_SUCCESS_LEN: usize = 7;

const SERIAL_ADDR: [u8; 2] = [0x00, 0x0D];
const SETTINGS_ADDR: [u8; 2] = [0x00, 0x00];
const BAUD_RATE_ADDR: [u8; 2] = [0x00, 0x2A];
const BAUD_RATE_115200: [u8; 2] = [0x1A, 0x00];
const SCAN_ADDR: [u8; 2] = [0x00, 0x02];
const TIMEOUT_ADDR: [u8; 2] = [0x00, 0x06];
const SCAN_INTERVAL_ADDR: [u8; 2] = [0x00, 0x05];
const SAME_BARCODE_DELAY_ADDR: [u8; 2] = [0x00, 0x13];
const VERSION_ADDR: [u8; 2] = [0x00, 0xE2];
const VERSION_NEEDS_RAW: u8 = 0x69;
const RAW_MODE_ADDR: [u8; 2] = [0x00, 0xBC];
const RAW_MODE_VALUE: u8 = 0x08;
const BAR_TYPE_ADDR: [u8; 2] = [0x00, 0x2C];
const QR_ADDR: [u8; 2] = [0x00, 0x3F];

const SCAN_INTERVAL_MS: u8 = 0x01;
const SAME_BARCODE_DELAY: u8 = 0x85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScannerModel {
    Gm65,
    M3Y,
    Generic,
    Unknown,
}

impl fmt::Display for ScannerModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScannerModel::Gm65 => write!(f, "GM65"),
            ScannerModel::M3Y => write!(f, "M3Y"),
            ScannerModel::Generic => write!(f, "Generic"),
            ScannerModel::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScannerError {
    NotDetected,
    Timeout,
    InvalidResponse,
    BufferOverflow,
    ConfigFailed,
    NotInitialized,
    UartError,
}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScannerError::NotDetected => write!(f, "Scanner not detected"),
            ScannerError::Timeout => write!(f, "Communication timeout"),
            ScannerError::InvalidResponse => write!(f, "Invalid response"),
            ScannerError::BufferOverflow => write!(f, "Buffer overflow"),
            ScannerError::ConfigFailed => write!(f, "Configuration failed"),
            ScannerError::NotInitialized => write!(f, "Not initialized"),
            ScannerError::UartError => write!(f, "UART error"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Continuous,
    CommandTriggered,
    HardwareTriggered,
}

#[derive(Debug, Clone)]
pub struct ScannerConfig {
    pub model: ScannerModel,
    pub baud_rate: u32,
    pub mode: ScanMode,
    pub raw_mode: bool,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            model: ScannerModel::Unknown,
            baud_rate: 9600,
            mode: ScanMode::CommandTriggered,
            raw_mode: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannerStatus {
    pub model: ScannerModel,
    pub connected: bool,
    pub initialized: bool,
    pub config: ScannerConfig,
    pub last_scan_len: Option<usize>,
}

pub struct ScanBuffer {
    data: [u8; MAX_SCAN_SIZE],
    len: usize,
}

impl ScanBuffer {
    pub const fn new() -> Self {
        Self {
            data: [0u8; MAX_SCAN_SIZE],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push(&mut self, byte: u8) -> bool {
        if self.len >= MAX_SCAN_SIZE {
            return false;
        }
        self.data[self.len] = byte;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn has_eol(&self) -> bool {
        if self.len == 0 {
            return false;
        }
        if self.len >= 2 && self.data[self.len - 2] == b'\r' && self.data[self.len - 1] == b'\n' {
            return true;
        }
        if self.data[self.len - 1] == b'\r' || self.data[self.len - 1] == b'\n' {
            return true;
        }
        false
    }

    pub fn data_without_eol(&self) -> &[u8] {
        let mut end = self.len;
        if end > 0 && self.data[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && self.data[end - 1] == b'\r' {
            end -= 1;
        }
        &self.data[..end]
    }
}

impl Default for ScanBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerState {
    Uninitialized,
    Detecting,
    Configuring,
    Ready,
    Scanning,
    ScanComplete,
    Error(ScannerError),
}

pub trait ScannerDriver {
    fn init(&mut self) -> Result<ScannerModel, ScannerError>;
    fn ping(&mut self) -> bool;
    fn trigger_scan(&mut self) -> Result<(), ScannerError>;
    fn read_scan(&mut self) -> Option<Vec<u8>>;
    fn state(&self) -> ScannerState;
    fn status(&self) -> ScannerStatus;
    fn data_ready(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudRate {
    Bps9600 = 9600,
    Bps115200 = 115200,
}

impl BaudRate {
    pub fn as_u32(&self) -> u32 {
        *self as u32
    }
}

pub struct Gm65Scanner<UART> {
    uart: UART,
    config: ScannerConfig,
    state: ScannerState,
    buffer: ScanBuffer,
    initialized: bool,
    detected_model: ScannerModel,
    last_scan_len: Option<usize>,
}

impl<UART> Gm65Scanner<UART> {
    pub fn new(uart: UART, config: ScannerConfig) -> Self {
        Self {
            uart,
            config,
            state: ScannerState::Uninitialized,
            buffer: ScanBuffer::new(),
            initialized: false,
            detected_model: ScannerModel::Unknown,
            last_scan_len: None,
        }
    }

    pub fn with_default_config(uart: UART) -> Self {
        Self::new(uart, ScannerConfig::default())
    }

    pub fn release(self) -> UART {
        self.uart
    }
}

impl<UART> Gm65Scanner<UART>
where
    UART: Write<u8> + Read<u8> + BlockingWrite<u8>,
{
    fn uart_write_all(&mut self, data: &[u8]) -> Result<(), ()> {
        self.uart.bwrite_all(data).map_err(|_| ())
    }

    fn send_command(&mut self, cmd: &[u8]) -> Option<Vec<u8>> {
        if self.uart_write_all(cmd).is_err() {
            return None;
        }

        let mut resp = Vec::with_capacity(GM65_SUCCESS_LEN);
        let mut total_attempts = 0u32;
        let max_attempts = 200_000u32;

        while resp.len() < GM65_SUCCESS_LEN && total_attempts < max_attempts {
            match self.uart.read() {
                Ok(byte) => {
                    resp.push(byte);
                    total_attempts = 0;
                }
                Err(nb::Error::WouldBlock) => {
                    total_attempts += 1;
                }
                Err(_) => {
                    return None;
                }
            }
        }

        if resp.len() != GM65_SUCCESS_LEN {
            return None;
        }

        Some(resp)
    }

    fn drain_uart(&mut self) {
        let mut attempts = 0u32;
        loop {
            match self.uart.read() {
                Ok(_) => attempts = 0,
                Err(nb::Error::WouldBlock) => {
                    attempts += 1;
                    if attempts > 1000 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    fn get_setting(&mut self, addr: [u8; 2]) -> Option<u8> {
        let cmd = build_get_setting(addr);
        let resp = self.send_command(&cmd)?;

        if resp.len() != GM65_SUCCESS_LEN {
            return None;
        }

        if resp[0..4] != GM65_SUCCESS_PREFIX {
            return None;
        }

        Some(resp[4])
    }

    fn set_setting(&mut self, addr: [u8; 2], value: u8) -> bool {
        let cmd = build_set_setting(addr, value);
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn set_setting_2byte(&mut self, addr: [u8; 2], value: [u8; 2]) -> bool {
        let cmd = build_set_setting_2byte(addr, value);
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn save_settings(&mut self) -> bool {
        let cmd = build_save_settings();
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn probe_gm65(&mut self) -> bool {
        self.drain_uart();
        self.get_setting(SERIAL_ADDR).is_some()
    }
}

impl<UART> ScannerDriver for Gm65Scanner<UART>
where
    UART: Write<u8> + Read<u8> + BlockingWrite<u8>,
{
    fn init(&mut self) -> Result<ScannerModel, ScannerError> {
        self.state = ScannerState::Detecting;

        if !self.probe_gm65() {
            defmt::warn!("Scanner not detected on UART (GM65 probe failed)");
            self.state = ScannerState::Error(ScannerError::NotDetected);
            return Err(ScannerError::NotDetected);
        }

        self.detected_model = ScannerModel::Gm65;
        defmt::info!("Scanner detected: GM65");
        self.state = ScannerState::Configuring;

        let serial_val = match self.get_setting(SERIAL_ADDR) {
            Some(v) => v,
            None => {
                defmt::warn!("Scanner: failed to read serial mode");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        };

        if serial_val & 0x03 != 0 {
            if !self.set_setting(SERIAL_ADDR, serial_val & 0xFC) {
                defmt::warn!("Scanner: failed to set serial mode");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        let cmd_mode: u8 = 0xD1;
        let scanner_settings: [([u8; 2], u8); 6] = [
            (SETTINGS_ADDR, cmd_mode),
            (TIMEOUT_ADDR, 0x00),
            (SCAN_INTERVAL_ADDR, SCAN_INTERVAL_MS),
            (SAME_BARCODE_DELAY_ADDR, SAME_BARCODE_DELAY),
            (BAR_TYPE_ADDR, 0x01),
            (QR_ADDR, 0x01),
        ];

        for (addr, set_val) in scanner_settings.iter() {
            match self.get_setting(*addr) {
                Some(val) => {
                    if val != *set_val {
                        if !self.set_setting(*addr, *set_val) {
                            defmt::warn!(
                                "Scanner: failed to set setting at {:02x}{:02x}",
                                addr[0],
                                addr[1]
                            );
                            self.state = ScannerState::Error(ScannerError::ConfigFailed);
                            return Err(ScannerError::ConfigFailed);
                        }
                    }
                }
                None => {
                    defmt::warn!(
                        "Scanner: failed to read setting at {:02x}{:02x}",
                        addr[0],
                        addr[1]
                    );
                    self.state = ScannerState::Error(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        }

        if let Some(version) = self.get_setting(VERSION_ADDR) {
            defmt::info!("Scanner firmware version: {}", version);
            if version == VERSION_NEEDS_RAW {
                match self.get_setting(RAW_MODE_ADDR) {
                    Some(val) => {
                        if val != RAW_MODE_VALUE {
                            self.set_setting(RAW_MODE_ADDR, RAW_MODE_VALUE);
                        }
                    }
                    None => {
                        defmt::warn!("Scanner: failed to read RAW mode");
                    }
                }
            }
        }

        if !self.save_settings() {
            defmt::warn!("Scanner: failed to save settings to EEPROM");
        }

        if self.set_setting_2byte(BAUD_RATE_ADDR, BAUD_RATE_115200) {
            defmt::info!("Scanner: baud rate set to 115200 (re-init UART on host side)");
        } else {
            defmt::warn!("Scanner: failed to set baud rate");
        }

        self.initialized = true;
        self.state = ScannerState::Ready;
        self.config.model = self.detected_model;
        defmt::info!("Scanner init complete");
        Ok(self.detected_model)
    }

    fn ping(&mut self) -> bool {
        self.get_setting(SERIAL_ADDR).is_some()
    }

    fn trigger_scan(&mut self) -> Result<(), ScannerError> {
        if !self.initialized {
            return Err(ScannerError::NotInitialized);
        }
        self.state = ScannerState::Scanning;
        self.buffer.clear();
        self.drain_uart();
        let cmd = build_set_setting(SCAN_ADDR, 0x01);
        self.uart_write_all(&cmd).ok();
        Ok(())
    }

    fn read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.initialized {
            return None;
        }

        let mut attempts = 0u32;
        let max_attempts = 500_000u32;

        while attempts < max_attempts {
            match self.uart.read() {
                Ok(b) => {
                    if !self.buffer.push(b) {
                        self.state = ScannerState::Error(ScannerError::BufferOverflow);
                        return None;
                    }
                    if self.buffer.has_eol() {
                        let data = self.buffer.data_without_eol();
                        if data.is_empty() {
                            self.buffer.clear();
                            return None;
                        }
                        self.last_scan_len = Some(data.len());
                        self.state = ScannerState::ScanComplete;
                        let result = data.to_vec();
                        self.buffer.clear();
                        return Some(result);
                    }
                    attempts = 0;
                }
                Err(nb::Error::WouldBlock) => {
                    attempts += 1;
                }
                Err(_) => {
                    self.state = ScannerState::Error(ScannerError::UartError);
                    return None;
                }
            }
        }

        self.state = ScannerState::Error(ScannerError::Timeout);
        None
    }

    fn state(&self) -> ScannerState {
        self.state
    }

    fn status(&self) -> ScannerStatus {
        ScannerStatus {
            model: self.detected_model,
            connected: self.initialized,
            initialized: self.initialized,
            config: self.config.clone(),
            last_scan_len: self.last_scan_len,
        }
    }

    fn data_ready(&self) -> bool {
        self.state == ScannerState::ScanComplete
    }
}

fn build_get_setting(addr: [u8; 2]) -> [u8; 9] {
    [
        GM65_HEADER[0],
        GM65_HEADER[1],
        0x07,
        0x01,
        addr[0],
        addr[1],
        0x01,
        GM65_CRC_NO_CHECKSUM[0],
        GM65_CRC_NO_CHECKSUM[1],
    ]
}

fn build_set_setting(addr: [u8; 2], value: u8) -> [u8; 9] {
    [
        GM65_HEADER[0],
        GM65_HEADER[1],
        0x08,
        0x01,
        addr[0],
        addr[1],
        value,
        GM65_CRC_NO_CHECKSUM[0],
        GM65_CRC_NO_CHECKSUM[1],
    ]
}

fn build_set_setting_2byte(addr: [u8; 2], value: [u8; 2]) -> [u8; 10] {
    [
        GM65_HEADER[0],
        GM65_HEADER[1],
        0x08,
        0x02,
        addr[0],
        addr[1],
        value[0],
        value[1],
        GM65_CRC_NO_CHECKSUM[0],
        GM65_CRC_NO_CHECKSUM[1],
    ]
}

fn build_save_settings() -> [u8; 9] {
    [0x7E, 0x00, 0x09, 0x01, 0x00, 0x00, 0x00, 0xDE, 0xC8]
}

fn build_factory_reset() -> [u8; 9] {
    [0x7E, 0x00, 0x08, 0x01, 0x00, 0xD9, 0x55, 0xAB, 0xCD]
}
