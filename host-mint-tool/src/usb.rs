use anyhow::{Context, Result};
use serialport::{SerialPort, SerialPortType};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use crate::protocol::Frame;

const TIMEOUT_MS: u64 = 5000;

pub struct UsbConnection {
    port: Box<dyn SerialPort>,
}

impl UsbConnection {
    pub fn open(path: &Path, baud: u32) -> Result<Self> {
        let port = serialport::new(path.to_string_lossy(), baud)
            .timeout(Duration::from_millis(TIMEOUT_MS))
            .open()
            .with_context(|| format!("Failed to open serial port {:?}", path))?;

        // Give the device a moment to reset after connection
        std::thread::sleep(Duration::from_millis(100));

        Ok(Self { port })
    }

    pub fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        let data = frame.encode();
        self.port
            .write_all(&data)
            .context("Failed to write frame to serial port")?;
        self.port.flush().context("Failed to flush serial port")?;
        tracing::debug!(
            "Sent frame: cmd=0x{:02X}, len={}",
            frame.command,
            frame.payload.len()
        );
        Ok(())
    }

    pub fn receive_frame(&mut self) -> Result<Frame> {
        let mut header = [0u8; 3];
        self.port
            .read_exact(&mut header)
            .context("Failed to read frame header")?;

        let status = header[0];
        let len = u16::from_be_bytes([header[1], header[2]]) as usize;

        let mut payload = vec![0u8; len];
        if len > 0 {
            self.port
                .read_exact(&mut payload)
                .context("Failed to read frame payload")?;
        }

        tracing::debug!("Received frame: status=0x{:02X}, len={}", status, len);

        // Treat status as command for response frames
        Ok(Frame::new(status, payload))
    }

    pub fn send_and_receive(&mut self, frame: &Frame) -> Result<Frame> {
        self.send_frame(frame)?;
        self.receive_frame()
    }

    pub fn list_devices() -> Result<Vec<String>> {
        let ports = serialport::available_ports().context("Failed to list serial ports")?;

        let devices: Vec<String> = ports
            .into_iter()
            .filter_map(|p| {
                // Filter for USB CDC devices (likely our STM32 board)
                match p.port_type {
                    SerialPortType::UsbPort(_) => Some(p.port_name),
                    _ => None,
                }
            })
            .collect();

        Ok(devices)
    }
}
