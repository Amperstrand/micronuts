//! GM65 Scanner Protocol
//!
//! Command and response handling for GM65 QR scanner modules.
//!
//! # Protocol Format
//!
//! All commands follow this structure:
//! `[Header:2][Type:1][Length:1][Address:2][Value:N][CRC:1][Footer:1]`
//!
//! - Header: `7E 00`
//! - Type: Command type (usually `08`)
//! - Length: Payload length
//! - Address: 2-byte register address
//! - Value: Data to write (if any)
//! - CRC: XOR checksum
//! - Footer: `55`

extern crate alloc;

use alloc::vec::Vec;

/// GM65 command header bytes
pub const HEADER: [u8; 2] = [0x7E, 0x00];

/// GM65 command footer byte
pub const FOOTER: u8 = 0x55;

/// GM65 command type for setting parameters
pub const CMD_SET_PARAM: u8 = 0x08;

/// GM65 command type for reading parameters
pub const CMD_GET_PARAM: u8 = 0x07;

/// GM65 command type for querying version
pub const CMD_QUERY_VERSION: u8 = 0x01;

/// Register addresses for GM65 configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    /// Serial output enable (0x0000)
    SerialOutput = 0x0000,
    /// Baud rate setting (0x002A)
    BaudRate = 0x002A,
    /// RAW mode enable (0x00BC) - critical for binary data
    RawMode = 0x00BC,
    /// Factory reset (0x00D9)
    FactoryReset = 0x00D9,
    /// Scan mode setting
    ScanMode = 0x0001,
    /// QR code only mode
    QrOnly = 0x0002,
    /// Scan interval
    ScanInterval = 0x0003,
}

impl Register {
    /// Get the 2-byte address for this register
    pub fn address_bytes(&self) -> [u8; 2] {
        let addr = *self as u16;
        [(addr >> 8) as u8, (addr & 0xFF) as u8]
    }
}

/// Baud rate values for Register::BaudRate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaudRate {
    /// 9600 bps (default)
    Bps9600 = 0x00,
    /// 19200 bps
    Bps19200 = 0x01,
    /// 38400 bps
    Bps38400 = 0x02,
    /// 57600 bps
    Bps57600 = 0x03,
    /// 115200 bps (recommended for fast transfers)
    Bps115200 = 0x1A,
}

impl BaudRate {
    /// Get the value byte for this baud rate
    pub fn value(&self) -> u8 {
        *self as u8
    }

    /// Get the actual baud rate as u32
    pub fn as_u32(&self) -> u32 {
        match self {
            BaudRate::Bps9600 => 9600,
            BaudRate::Bps19200 => 19200,
            BaudRate::Bps38400 => 38400,
            BaudRate::Bps57600 => 57600,
            BaudRate::Bps115200 => 115200,
        }
    }
}

/// GM65 command builder
pub struct Gm65CommandBuilder {
    cmd_type: u8,
    register: Register,
    value: Vec<u8>,
}

impl Gm65CommandBuilder {
    /// Create a new command builder for setting a parameter
    pub fn set(register: Register) -> Self {
        Self {
            cmd_type: CMD_SET_PARAM,
            register,
            value: Vec::new(),
        }
    }

    /// Create a new command builder for reading a parameter
    pub fn get(register: Register) -> Self {
        Self {
            cmd_type: CMD_GET_PARAM,
            register,
            value: Vec::new(),
        }
    }

    /// Add a value byte to the command
    pub fn with_value(mut self, value: u8) -> Self {
        self.value.push(value);
        self
    }

    /// Add multiple value bytes to the command
    pub fn with_values(mut self, values: &[u8]) -> Self {
        self.value.extend_from_slice(values);
        self
    }

    /// Build the complete command bytes
    pub fn build(self) -> Vec<u8> {
        let addr = self.register.address_bytes();
        let payload_len = 2 + self.value.len(); // address + value

        let mut cmd = Vec::with_capacity(8 + self.value.len());
        cmd.extend_from_slice(&HEADER);
        cmd.push(self.cmd_type);
        cmd.push(payload_len as u8);
        cmd.extend_from_slice(&addr);
        cmd.extend_from_slice(&self.value);

        // Calculate CRC (XOR of bytes from type to before CRC)
        let crc = calculate_crc(&cmd[2..]);
        cmd.push(crc);
        cmd.push(FOOTER);

        cmd
    }
}

/// Calculate GM65 CRC (XOR checksum)
///
/// XORs all bytes from the type field to the value field
pub fn calculate_crc(data: &[u8]) -> u8 {
    data.iter().fold(0, |acc, &b| acc ^ b)
}

/// Parse a GM65 response
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gm65Response {
    /// Command acknowledged successfully
    Ack,
    /// Command failed with error code
    Nack(u8),
    /// Version information
    Version { major: u8, minor: u8 },
    /// Parameter value read
    ParamValue(Vec<u8>),
    /// Invalid/unknown response
    Invalid,
}

impl Gm65Response {
    /// Parse response bytes into a Gm65Response
    pub fn parse(data: &[u8]) -> Self {
        // Minimum response: header(2) + type(1) + status(1) + crc(1) + footer(1) = 6
        if data.len() < 6 {
            return Gm65Response::Invalid;
        }

        // Check header
        if data[0] != HEADER[0] || data[1] != HEADER[1] {
            return Gm65Response::Invalid;
        }

        // Check footer
        if data[data.len() - 1] != FOOTER {
            return Gm65Response::Invalid;
        }

        // Verify CRC
        let expected_crc = calculate_crc(&data[2..data.len() - 2]);
        if data[data.len() - 2] != expected_crc {
            return Gm65Response::Invalid;
        }

        // Parse response type
        let status = data[3];
        match status {
            0x00 => Gm65Response::Ack,
            0xEE => Gm65Response::Nack(data.get(4).copied().unwrap_or(0)),
            _ => {
                // Could be version or parameter data
                if data.len() > 5 {
                    Gm65Response::ParamValue(data[4..data.len() - 2].to_vec())
                } else {
                    Gm65Response::Invalid
                }
            }
        }
    }
}

/// Factory functions for common GM65 commands
pub mod commands {
    use super::*;

    /// Build factory reset command
    pub fn factory_reset() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::FactoryReset)
            .with_value(0x00)
            .build()
    }

    /// Build serial output enable command
    pub fn enable_serial_output() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::SerialOutput)
            .with_value(0x01)
            .build()
    }

    /// Build baud rate change command
    pub fn set_baud_rate(rate: BaudRate) -> Vec<u8> {
        Gm65CommandBuilder::set(Register::BaudRate)
            .with_value(rate.value())
            .build()
    }

    /// Build RAW mode enable command (critical for binary data)
    pub fn enable_raw_mode() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::RawMode)
            .with_value(0x08)
            .build()
    }

    /// Build QR-only mode command (disable 1D barcodes)
    pub fn set_qr_only() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::QrOnly)
            .with_value(0x01)
            .build()
    }

    /// Build version query command
    pub fn query_version() -> Vec<u8> {
        vec![0x7E, 0x00, 0x01, 0x00, 0x01, 0x01, 0x55]
    }

    /// Build scan trigger command (for command-triggered mode)
    pub fn trigger_scan() -> Vec<u8> {
        vec![0x7E, 0x00, 0x04, 0x00, 0x04, 0x00, 0x55]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc_calculation() {
        // CRC is XOR of all bytes from type to value
        let data = [0x08, 0x01, 0x00, 0x00, 0x01];
        let crc = calculate_crc(&data);
        assert_eq!(crc, 0x08 ^ 0x01 ^ 0x00 ^ 0x00 ^ 0x01);
    }

    #[test]
    fn test_register_address() {
        assert_eq!(Register::SerialOutput.address_bytes(), [0x00, 0x00]);
        assert_eq!(Register::BaudRate.address_bytes(), [0x00, 0x2A]);
        assert_eq!(Register::RawMode.address_bytes(), [0x00, 0xBC]);
    }

    #[test]
    fn test_baud_rate_values() {
        assert_eq!(BaudRate::Bps9600.as_u32(), 9600);
        assert_eq!(BaudRate::Bps115200.as_u32(), 115200);
        assert_eq!(BaudRate::Bps115200.value(), 0x1A);
    }

    #[test]
    fn test_enable_serial_output_command() {
        let cmd = commands::enable_serial_output();
        assert_eq!(&cmd[..2], &HEADER);
        assert_eq!(cmd[cmd.len() - 1], FOOTER);
        assert_eq!(cmd[2], CMD_SET_PARAM);
    }

    #[test]
    fn test_set_baud_rate_command() {
        let cmd = commands::set_baud_rate(BaudRate::Bps115200);
        assert_eq!(&cmd[..2], &HEADER);
        assert_eq!(cmd[2], CMD_SET_PARAM);
        // Should contain the baud rate address (0x00, 0x2A) and value (0x1A)
    }

    #[test]
    fn test_response_parse_ack() {
        // ACK response: 7E 00 00 00 CRC 55
        let response = [0x7E, 0x00, 0x00, 0x00, 0x00, 0x55];
        let parsed = Gm65Response::parse(&response);
        assert_eq!(parsed, Gm65Response::Ack);
    }

    #[test]
    fn test_response_parse_invalid() {
        // Too short
        let response = [0x7E, 0x00];
        let parsed = Gm65Response::parse(&response);
        assert_eq!(parsed, Gm65Response::Invalid);

        // Wrong header
        let response = [0x7F, 0x00, 0x00, 0x00, 0x00, 0x55];
        let parsed = Gm65Response::parse(&response);
        assert_eq!(parsed, Gm65Response::Invalid);
    }
}
