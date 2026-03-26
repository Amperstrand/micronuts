pub mod decoder;

pub use gm65_scanner::Gm65BaudRate;
pub use gm65_scanner::Register;
pub use gm65_scanner::{
    build_factory_reset, build_get_setting, build_save_settings, build_set_setting,
    build_trigger_scan, commands, Gm65ScannerAsync, ScanBuffer, ScannerConfig, ScannerDriver,
    ScannerDriverSync, ScannerError, ScannerModel, ScannerState, ScannerStatus,
    UrDecoder as Gm65UrDecoder,
};

pub use decoder::{decode_qr, is_qr_payload, DecodedPayload, QrPayload, UrDecoder};
