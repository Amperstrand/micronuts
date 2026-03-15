//! USB CDC communication layer
//!
//! Handles serial communication with the host mint tool using a frame-based protocol.
//!
//! # Protocol
//!
//! Request:  `[Cmd:1][Len:2][Payload:N]`
//! Response: `[Status:1][Len:2][Payload:N]`
//!
//! ## Commands
//! - 0x01 IMPORT_TOKEN    - Send V4 token to device
//! - 0x02 GET_TOKEN_INFO  - Request token summary
//! - 0x03 GET_BLINDED     - Request blinded outputs
//! - 0x04 SEND_SIGNATURES - Send blind signatures
//! - 0x05 GET_PROOFS      - Request unblinded proofs

use stm32f469i_disc::hal::otg_fs::UsbBusType;

/// Maximum payload size (256 bytes)
pub const MAX_PAYLOAD_SIZE: usize = 256;

/// Receive buffer size (header + max payload)
const RX_BUF_SIZE: usize = 3 + MAX_PAYLOAD_SIZE;

/// Command opcodes for device-host protocol
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl Command {
    /// Try to convert a byte to a Command
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Command::ImportToken),
            0x02 => Some(Command::GetTokenInfo),
            0x03 => Some(Command::GetBlinded),
            0x04 => Some(Command::SendSignatures),
            0x05 => Some(Command::GetProofs),
            _ => None,
        }
    }
}

/// Response status codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Success
    Ok = 0x00,
    /// Generic error
    Error = 0xFF,
    /// Unknown/invalid command
    InvalidCommand = 0x01,
    /// Malformed payload
    InvalidPayload = 0x02,
    /// Buffer overflow
    BufferOverflow = 0x03,
    /// Cryptographic operation failed
    CryptoError = 0x04,
}

impl Status {
    /// Convert to byte
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

/// Protocol frame structure for requests
#[derive(Debug, Clone)]
pub struct Frame {
    pub command: Command,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl Frame {
    /// Create a new empty frame with the given command
    pub fn new(command: Command) -> Self {
        Self {
            command,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    /// Create a frame with payload data
    ///
    /// Returns None if payload exceeds MAX_PAYLOAD_SIZE
    pub fn with_payload(command: Command, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut frame = Self::new(command);
        frame.length = data.len() as u16;
        frame.payload[..data.len()].copy_from_slice(data);
        Some(frame)
    }

    /// Get the payload as a slice
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

    /// Encode this frame to a byte buffer
    ///
    /// Format: `[Cmd:1][Len:2][Payload:N]`
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let total_len = 3 + self.length as usize;
        if buf.len() < total_len {
            return 0;
        }
        buf[0] = self.command as u8;
        buf[1] = (self.length >> 8) as u8;
        buf[2] = (self.length & 0xFF) as u8;
        buf[3..total_len].copy_from_slice(self.payload());
        total_len
    }

    /// Calculate the encoded size
    pub fn encoded_size(&self) -> usize {
        3 + self.length as usize
    }
}

/// Response frame structure
#[derive(Debug, Clone)]
pub struct Response {
    pub status: Status,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl Response {
    /// Create a new response with just a status (no payload)
    pub fn new(status: Status) -> Self {
        Self {
            status,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    /// Create a response with payload data
    ///
    /// Returns None if payload exceeds MAX_PAYLOAD_SIZE
    pub fn with_payload(status: Status, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut resp = Self::new(status);
        resp.length = data.len() as u16;
        resp.payload[..data.len()].copy_from_slice(data);
        Some(resp)
    }

    /// Get the payload as a slice
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

    /// Encode this response to a byte buffer
    ///
    /// Format: `[Status:1][Len:2][Payload:N]`
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let total_len = 3 + self.length as usize;
        if buf.len() < total_len {
            return 0;
        }
        buf[0] = self.status.to_byte();
        buf[1] = (self.length >> 8) as u8;
        buf[2] = (self.length & 0xFF) as u8;
        buf[3..total_len].copy_from_slice(self.payload());
        total_len
    }

    /// Calculate the encoded size
    pub fn encoded_size(&self) -> usize {
        3 + self.length as usize
    }
}

/// Frame decoder state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeState {
    /// Waiting for command byte
    Idle,
    /// Got command, waiting for length high byte
    LenHigh,
    /// Got length high, waiting for length low byte
    LenLow,
    /// Receiving payload bytes
    Payload,
}

/// Frame decoder with buffering for incomplete frames
#[derive(Debug)]
pub struct FrameDecoder {
    state: DecodeState,
    command_byte: u8,
    length: u16,
    payload_idx: usize,
    payload: [u8; MAX_PAYLOAD_SIZE],
}

impl FrameDecoder {
    /// Create a new frame decoder
    pub const fn new() -> Self {
        Self {
            state: DecodeState::Idle,
            command_byte: 0,
            length: 0,
            payload_idx: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    /// Reset decoder state
    pub fn reset(&mut self) {
        self.state = DecodeState::Idle;
        self.command_byte = 0;
        self.length = 0;
        self.payload_idx = 0;
    }

    /// Process incoming bytes and return a complete frame if available
    ///
    /// Returns `Some(Frame)` when a complete frame has been received,
    /// `None` if more bytes are needed.
    pub fn decode(&mut self, data: &[u8]) -> Option<Frame> {
        for &byte in data {
            match self.state {
                DecodeState::Idle => {
                    self.command_byte = byte;
                    self.state = DecodeState::LenHigh;
                }
                DecodeState::LenHigh => {
                    self.length = (byte as u16) << 8;
                    self.state = DecodeState::LenLow;
                }
                DecodeState::LenLow => {
                    self.length |= byte as u16;

                    // Validate length
                    if self.length as usize > MAX_PAYLOAD_SIZE {
                        self.reset();
                        return None;
                    }

                    if self.length == 0 {
                        // No payload, frame complete
                        let cmd = Command::from_byte(self.command_byte)?;
                        let frame = Frame::new(cmd);
                        self.reset();
                        return Some(frame);
                    }

                    self.payload_idx = 0;
                    self.state = DecodeState::Payload;
                }
                DecodeState::Payload => {
                    self.payload[self.payload_idx] = byte;
                    self.payload_idx += 1;

                    if self.payload_idx >= self.length as usize {
                        // Frame complete
                        let cmd = Command::from_byte(self.command_byte)?;
                        let frame = Frame::with_payload(cmd, &self.payload[..self.payload_idx])?;
                        self.reset();
                        return Some(frame);
                    }
                }
            }
        }
        None
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// CDC port wrapper for frame-based communication
///
/// Manages the USB serial port and handles frame encoding/decoding.
pub struct CdcPort<'a> {
    serial: usbd_serial::SerialPort<'a, UsbBusType>,
    decoder: FrameDecoder,
    tx_buf: [u8; RX_BUF_SIZE],
}

impl<'a> CdcPort<'a> {
    /// Create a new CDC port wrapper
    pub fn new(serial: usbd_serial::SerialPort<'a, UsbBusType>) -> Self {
        Self {
            serial,
            decoder: FrameDecoder::new(),
            tx_buf: [0; RX_BUF_SIZE],
        }
    }

    /// Read bytes from the serial port into the provided buffer
    ///
    /// Returns the number of bytes read, or 0 if no data available
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        match self.serial.read(buf) {
            Ok(count) => count,
            Err(_) => 0,
        }
    }

    /// Try to receive a complete frame
    ///
    /// Returns `Some(Frame)` when a complete frame has been received,
    /// `None` if more bytes are needed or no data available.
    pub fn receive_frame(&mut self) -> Option<Frame> {
        let mut rx_buf = [0u8; 64];

        match self.serial.read(&mut rx_buf) {
            Ok(count) if count > 0 => self.decoder.decode(&rx_buf[..count]),
            _ => None,
        }
    }

    /// Send a response frame
    ///
    /// Returns true if the frame was sent successfully
    pub fn send_response(&mut self, response: &Response) -> bool {
        let len = response.encode(&mut self.tx_buf);
        if len == 0 {
            return false;
        }

        let mut offset = 0;
        while offset < len {
            match self.serial.write(&self.tx_buf[offset..len]) {
                Ok(written) if written > 0 => {
                    offset += written;
                }
                _ => {
                    // Flush and retry
                    let _ = self.serial.flush();
                }
            }
        }

        let _ = self.serial.flush();
        true
    }

    /// Send an error response with the given status
    pub fn send_error(&mut self, status: Status) -> bool {
        let response = Response::new(status);
        self.send_response(&response)
    }

    /// Send a success response with optional payload
    pub fn send_ok(&mut self, payload: Option<&[u8]>) -> bool {
        let response = match payload {
            Some(data) => Response::with_payload(Status::Ok, data),
            None => Some(Response::new(Status::Ok)),
        };

        match response {
            Some(resp) => self.send_response(&resp),
            None => self.send_error(Status::BufferOverflow),
        }
    }

    pub fn serial_mut(&mut self) -> &mut usbd_serial::SerialPort<'a, UsbBusType> {
        &mut self.serial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_encode_decode() {
        let payload = b"hello";
        let frame = Frame::with_payload(Command::ImportToken, payload).unwrap();

        let mut buf = [0u8; 260];
        let len = frame.encode(&mut buf);

        assert_eq!(len, 3 + payload.len());
        assert_eq!(buf[0], 0x01); // Command
        assert_eq!(buf[1], 0); // Length high
        assert_eq!(buf[2], 5); // Length low
        assert_eq!(&buf[3..8], payload);
    }

    #[test]
    fn test_response_encode() {
        let payload = b"world";
        let response = Response::with_payload(Status::Ok, payload).unwrap();

        let mut buf = [0u8; 260];
        let len = response.encode(&mut buf);

        assert_eq!(len, 3 + payload.len());
        assert_eq!(buf[0], 0x00); // Status::Ok
        assert_eq!(buf[1], 0); // Length high
        assert_eq!(buf[2], 5); // Length low
        assert_eq!(&buf[3..8], payload);
    }

    #[test]
    fn test_decoder_simple() {
        let mut decoder = FrameDecoder::new();

        // Encode a frame
        let frame = Frame::with_payload(Command::GetTokenInfo, b"test").unwrap();
        let mut buf = [0u8; 260];
        let len = frame.encode(&mut buf);

        // Decode it
        let decoded = decoder.decode(&buf[..len]).unwrap();

        assert_eq!(decoded.command, Command::GetTokenInfo);
        assert_eq!(decoded.length, 4);
        assert_eq!(decoded.payload(), b"test");
    }

    #[test]
    fn test_decoder_partial() {
        let mut decoder = FrameDecoder::new();

        // Send partial data
        let result = decoder.decode(&[0x02]); // Command byte only
        assert!(result.is_none());

        let result = decoder.decode(&[0x00]); // Length high
        assert!(result.is_none());

        let result = decoder.decode(&[0x02]); // Length low (2)
        assert!(result.is_none());

        // Send first payload byte
        let result = decoder.decode(&[0xAB]);
        assert!(result.is_none());

        // Send final byte - frame complete
        let frame = decoder.decode(&[0xCD]).unwrap();
        assert_eq!(frame.command, Command::GetTokenInfo);
        assert_eq!(frame.payload(), &[0xAB, 0xCD]);
    }

    #[test]
    fn test_decoder_zero_length_payload() {
        let mut decoder = FrameDecoder::new();

        // Encode a frame with no payload
        let frame = Frame::new(Command::GetProofs);
        let mut buf = [0u8; 260];
        let len = frame.encode(&mut buf);

        let decoded = decoder.decode(&buf[..len]).unwrap();
        assert_eq!(decoded.command, Command::GetProofs);
        assert_eq!(decoded.length, 0);
    }
}
