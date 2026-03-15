//! USB CDC communication layer
//!
//! Handles serial communication with the host mint tool

use embedded_hal::serial::Write;

/// Command opcodes for device-host protocol
#[repr(u8)]
pub enum Command {
    /// Import a V4 token to device
    ImportToken = 0x01,
    /// Request token info from device
    GetTokenInfo = 0x02,
    /// Request blinded outputs from device
    GetBlinded = 0x03,
    /// Send blind signatures to device
    SendSignatures = 0x04,
    /// Request unblinded proofs from device
    GetProofs = 0x05,
}

/// Response status codes
#[repr(u8)]
pub enum Status {
    Ok = 0x00,
    Error = 0xFF,
    InvalidCommand = 0x01,
    InvalidPayload = 0x02,
    BufferOverflow = 0x03,
    CryptoError = 0x04,
}

/// Protocol frame structure
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub command: u8,
    pub length: u16,
    pub payload: [u8; 256],
}

impl Frame {
    pub fn new(command: Command) -> Self {
        Self {
            command: command as u8,
            length: 0,
            payload: [0; 256],
        }
    }

    pub fn with_payload(command: Command, data: &[u8]) -> Option<Self> {
        if data.len() > 256 {
            return None;
        }
        let mut frame = Self::new(command);
        frame.length = data.len() as u16;
        frame.payload[..data.len()].copy_from_slice(data);
        Some(frame)
    }
}
