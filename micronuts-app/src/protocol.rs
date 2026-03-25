extern crate alloc;

pub const MAX_PAYLOAD_SIZE: usize = 256;

const RX_BUF_SIZE: usize = 3 + MAX_PAYLOAD_SIZE;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ImportToken = 0x01,
    GetTokenInfo = 0x02,
    GetBlinded = 0x03,
    SendSignatures = 0x04,
    GetProofs = 0x05,
    ScannerStatus = 0x10,
    ScannerTrigger = 0x11,
    ScannerData = 0x12,
}

impl Command {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Command::ImportToken),
            0x02 => Some(Command::GetTokenInfo),
            0x03 => Some(Command::GetBlinded),
            0x04 => Some(Command::SendSignatures),
            0x05 => Some(Command::GetProofs),
            0x10 => Some(Command::ScannerStatus),
            0x11 => Some(Command::ScannerTrigger),
            0x12 => Some(Command::ScannerData),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok = 0x00,
    Error = 0xFF,
    InvalidCommand = 0x01,
    InvalidPayload = 0x02,
    BufferOverflow = 0x03,
    CryptoError = 0x04,
    ScannerNotConnected = 0x10,
    ScannerBusy = 0x11,
    NoScanData = 0x12,
}

impl Status {
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub command: Command,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl Frame {
    pub fn new(command: Command) -> Self {
        Self {
            command,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn with_payload(command: Command, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut frame = Self::new(command);
        frame.length = data.len() as u16;
        frame.payload[..data.len()].copy_from_slice(data);
        Some(frame)
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

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

    pub fn encoded_size(&self) -> usize {
        3 + self.length as usize
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: Status,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl Response {
    pub fn new(status: Status) -> Self {
        Self {
            status,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn with_payload(status: Status, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut resp = Self::new(status);
        resp.length = data.len() as u16;
        resp.payload[..data.len()].copy_from_slice(data);
        Some(resp)
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

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

    pub fn encoded_size(&self) -> usize {
        3 + self.length as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeState {
    Idle,
    LenHigh,
    LenLow,
    Payload,
}

#[derive(Debug)]
pub struct FrameDecoder {
    state: DecodeState,
    command_byte: u8,
    length: u16,
    payload_idx: usize,
    payload: [u8; MAX_PAYLOAD_SIZE],
}

impl FrameDecoder {
    pub const fn new() -> Self {
        Self {
            state: DecodeState::Idle,
            command_byte: 0,
            length: 0,
            payload_idx: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn reset(&mut self) {
        self.state = DecodeState::Idle;
        self.command_byte = 0;
        self.length = 0;
        self.payload_idx = 0;
    }

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

                    if self.length as usize > MAX_PAYLOAD_SIZE {
                        self.reset();
                        return None;
                    }

                    if self.length == 0 {
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

pub struct FrameEncoder {
    tx_buf: [u8; RX_BUF_SIZE],
}

impl FrameEncoder {
    pub const fn new() -> Self {
        Self {
            tx_buf: [0; RX_BUF_SIZE],
        }
    }

    pub fn encode_response(&self, response: &Response) -> &[u8] {
        let len = response.encode(&mut self.tx_buf.clone());
        &self.tx_buf[..len]
    }
}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self::new()
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
        assert_eq!(buf[0], 0x01);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[2], 5);
        assert_eq!(&buf[3..8], payload);
    }

    #[test]
    fn test_response_encode() {
        let payload = b"world";
        let response = Response::with_payload(Status::Ok, payload).unwrap();

        let mut buf = [0u8; 260];
        let len = response.encode(&mut buf);

        assert_eq!(len, 3 + payload.len());
        assert_eq!(buf[0], 0x00);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[2], 5);
        assert_eq!(&buf[3..8], payload);
    }

    #[test]
    fn test_decoder_simple() {
        let mut decoder = FrameDecoder::new();

        let frame = Frame::with_payload(Command::GetTokenInfo, b"test").unwrap();
        let mut buf = [0u8; 260];
        let len = frame.encode(&mut buf);

        let decoded = decoder.decode(&buf[..len]).unwrap();

        assert_eq!(decoded.command, Command::GetTokenInfo);
        assert_eq!(decoded.length, 4);
        assert_eq!(decoded.payload(), b"test");
    }

    #[test]
    fn test_decoder_partial() {
        let mut decoder = FrameDecoder::new();

        let result = decoder.decode(&[0x02]);
        assert!(result.is_none());

        let result = decoder.decode(&[0x00]);
        assert!(result.is_none());

        let result = decoder.decode(&[0x02]);
        assert!(result.is_none());

        let result = decoder.decode(&[0xAB]);
        assert!(result.is_none());

        let frame = decoder.decode(&[0xCD]).unwrap();
        assert_eq!(frame.command, Command::GetTokenInfo);
        assert_eq!(frame.payload(), &[0xAB, 0xCD]);
    }

    #[test]
    fn test_decoder_zero_length_payload() {
        let mut decoder = FrameDecoder::new();

        let frame = Frame::new(Command::GetProofs);
        let mut buf = [0u8; 260];
        let len = frame.encode(&mut buf);

        let decoded = decoder.decode(&buf[..len]).unwrap();
        assert_eq!(decoded.command, Command::GetProofs);
        assert_eq!(decoded.length, 0);
    }
}
