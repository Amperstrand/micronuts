pub const CMD_IMPORT_TOKEN: u8 = 0x01;
pub const CMD_GET_TOKEN_INFO: u8 = 0x02;
pub const CMD_GET_BLINDED: u8 = 0x03;
pub const CMD_SEND_SIGNATURES: u8 = 0x04;
pub const CMD_GET_PROOFS: u8 = 0x05;
pub const CMD_SCANNER_STATUS: u8 = 0x10;
pub const CMD_SCANNER_TRIGGER: u8 = 0x11;
pub const CMD_SCANNER_DATA: u8 = 0x12;

pub const STATUS_OK: u8 = 0x00;
pub const STATUS_ERROR: u8 = 0xFF;
pub const STATUS_NO_SCAN_DATA: u8 = 0x12;

#[derive(Debug, Clone)]
pub struct Frame {
    pub command: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(command: u8, payload: Vec<u8>) -> Self {
        Self { command, payload }
    }

    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len() as u16;
        let mut buf = Vec::with_capacity(3 + self.payload.len());
        buf.push(self.command);
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        let command = data[0];
        let len = u16::from_be_bytes([data[1], data[2]]) as usize;
        if data.len() < 3 + len {
            return None;
        }
        Some(Self {
            command,
            payload: data[3..3 + len].to_vec(),
        })
    }
}
