//! QR Scanner Hardware Driver
//!
//! Low-level driver for GM65/M3Y QR scanner modules connected via UART.
//! Handles scanner detection, initialization, and basic scan operations.
//!
//! # Hardware Configuration
//!
//! - UART: USART6 (PC6=TX, PC7=RX)
//! - Baud: 9600 (default) → 115200 (after config)
//! - Trigger: Optional GPIO (PB2 recommended)
//!
//! # Example
//!
//! ```rust,ignore
//! let mut scanner = ScannerDriver::new(uart, Some(trigger_pin));
//! scanner.init().await.ok();
//! if let Some(data) = scanner.read_scan().await {
//!     // Process scanned data
//! }
//! ```

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

/// Maximum scan data buffer size (QR codes can be large)
pub const MAX_SCAN_SIZE: usize = 2048;

/// Scanner model detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerModel {
    /// GM65 scanner module (primary target)
    Gm65,
    /// M3Y scanner module (alternative)
    M3Y,
    /// Generic UART scanner
    Generic,
    /// Unknown/unresponsive scanner
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

/// Scanner driver errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerError {
    /// Scanner not detected on UART
    NotDetected,
    /// Communication timeout
    Timeout,
    /// Invalid response from scanner
    InvalidResponse,
    /// Buffer overflow
    BufferOverflow,
    /// Configuration failed
    ConfigFailed,
    /// Scanner not initialized
    NotInitialized,
    /// UART error
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

/// Scanner operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Continuous scanning (scanner auto-triggers)
    Continuous,
    /// Command-triggered scanning (host initiates)
    CommandTriggered,
    /// Hardware-triggered scanning (GPIO pin)
    HardwareTriggered,
}

/// Scanner configuration
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// Scanner model (auto-detected if Unknown)
    pub model: ScannerModel,
    /// Baud rate (default: 9600, switches to 115200 after init)
    pub baud_rate: u32,
    /// Scanning mode
    pub mode: ScanMode,
    /// Enable RAW mode for binary data (required for Cashu tokens)
    pub raw_mode: bool,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            model: ScannerModel::Unknown,
            baud_rate: 9600,
            mode: ScanMode::CommandTriggered,
            raw_mode: true, // Required for Cashu tokens
        }
    }
}

/// Scanner status for debug display
#[derive(Debug, Clone)]
pub struct ScannerStatus {
    /// Detected scanner model
    pub model: ScannerModel,
    /// Whether scanner is connected and responsive
    pub connected: bool,
    /// Whether scanner has been initialized
    pub initialized: bool,
    /// Current configuration
    pub config: ScannerConfig,
    /// Last scan data length (if any)
    pub last_scan_len: Option<usize>,
}

/// RX buffer for incoming scan data
/// Scanner sends EOL-terminated data ('\r' or '\r\n')
pub struct ScanBuffer {
    /// Data buffer
    data: [u8; MAX_SCAN_SIZE],
    /// Current write position
    len: usize,
}

impl ScanBuffer {
    /// Create a new empty scan buffer
    pub const fn new() -> Self {
        Self {
            data: [0u8; MAX_SCAN_SIZE],
            len: 0,
        }
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Append a byte to the buffer
    /// Returns true if successful, false if buffer full
    pub fn push(&mut self, byte: u8) -> bool {
        if self.len >= MAX_SCAN_SIZE {
            return false;
        }
        self.data[self.len] = byte;
        self.len += 1;
        true
    }

    /// Get the current data as a slice
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Get the current length
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Check if buffer ends with EOL marker
    pub fn has_eol(&self) -> bool {
        if self.len == 0 {
            return false;
        }
        // Check for \r\n
        if self.len >= 2 && self.data[self.len - 2] == b'\r' && self.data[self.len - 1] == b'\n' {
            return true;
        }
        // Check for just \r
        if self.data[self.len - 1] == b'\r' {
            return true;
        }
        // Check for just \n
        if self.data[self.len - 1] == b'\n' {
            return true;
        }
        false
    }

    /// Get data without EOL markers
    pub fn data_without_eol(&self) -> &[u8] {
        let mut end = self.len;
        
        // Strip trailing \n
        if end > 0 && self.data[end - 1] == b'\n' {
            end -= 1;
        }
        // Strip trailing \r
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

/// Scanner state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerState {
    /// Scanner not yet initialized
    Uninitialized,
    /// Attempting to detect scanner model
    Detecting,
    /// Scanner detected, configuring
    Configuring,
    /// Ready to scan
    Ready,
    /// Currently scanning (waiting for data)
    Scanning,
    /// Scan complete, data available
    ScanComplete,
    /// Error state
    Error(ScannerError),
}

/// Trait for scanner drivers
/// 
/// This trait abstracts the scanner hardware, allowing for different
/// implementations (real hardware, mock for testing, etc.)
pub trait ScannerDriver {
    /// Initialize the scanner and detect model
    /// Returns the detected model on success
    async fn init(&mut self) -> Result<ScannerModel, ScannerError>;
    
    /// Check if scanner is connected and responsive
    async fn ping(&mut self) -> bool;
    
    /// Trigger a scan (for command-triggered mode)
    async fn trigger_scan(&mut self) -> Result<(), ScannerError>;
    
    /// Read scanned data
    /// Returns None if no data available or timeout
    async fn read_scan(&mut self) -> Option<Vec<u8>>;
    
    /// Get current scanner state
    fn state(&self) -> ScannerState;
    
    /// Get scanner status for display
    fn status(&self) -> ScannerStatus;
    
    /// Check if data is ready to read
    fn data_ready(&self) -> bool;
}


