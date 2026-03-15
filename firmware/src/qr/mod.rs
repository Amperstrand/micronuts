//! QR Scanner Support for Micronuts
//!
//! This module provides QR scanner integration for the Micronuts firmware.
//! It supports GM65 and M3Y scanner modules connected via UART.
//!
//! # Architecture
//!
//! - `driver`: Low-level hardware driver and types
//! - `protocol`: GM65 command/response protocol
//! - `decoder`: QR payload decoding (Cashu, UR, plain text)
//!
//! # Usage
//!
//! ```rust,ignore
//! use firmware::qr::{decode_qr, ScannerConfig, ScannerModel, QrPayload};
//!
//! // Decode scanned data
//! let payload = decode_qr(&scanned_data);
//! match payload {
//!     QrPayload::CashuV4 { encoded } => {
//!         // Handle Cashu token
//!     }
//!     QrPayload::PlainText(data) => {
//!         // Handle plain text
//!     }
//!     _ => {}
//! }
//! ```

pub mod decoder;
pub mod driver;
pub mod protocol;

pub use decoder::{decode_qr, is_qr_payload, QrPayload, UrDecoder};
pub use driver::{
    BaudRate, ScanBuffer, ScanMode, ScannerConfig, ScannerDriver, ScannerError, ScannerModel,
    ScannerState, ScannerStatus, MAX_SCAN_SIZE,
};
pub use protocol::{
    calculate_crc, commands, BaudRate as Gm65BaudRate, Gm65CommandBuilder, Gm65Response, Register,
};
