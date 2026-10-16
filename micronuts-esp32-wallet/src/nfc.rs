//! NFC frontend for the MFRC522 bench reader (feature `nfc-mfrc522`,
//! M5StickC Plus Grove I2C port: SDA 32 / SCL 33, address 0x28 — the
//! same wiring as ccid-firmware-rs `board-m5stick` and the nucula
//! rig's M5 boards).
//!
//! Bring-up mirrors the bench-proven ccid-firmware-rs init
//! (`mfrc522_driver.rs`): version check 0x91/0x92, 33 dB antenna gain.
//! Scanning is one REQA + select (the mfrc522 crate's select handles
//! ISO 14443-3 cascade levels internally and returns the full UID).

use esp_idf_svc::hal::i2c;
use esp_idf_svc::hal::units::Hertz;

pub struct NfcFrontend {
    reader:
        mfrc522::Mfrc522<mfrc522::comm::blocking::i2c::I2cInterface<i2c::I2cDriver<'static>>, mfrc522::Initialized>,
}

impl NfcFrontend {
    pub fn new(i2c: i2c::I2cDriver<'static>) -> Result<Self, String> {
        let mut reader = mfrc522::Mfrc522::new(
            mfrc522::comm::blocking::i2c::I2cInterface::new(i2c, 0x28),
        )
        .init()
        .map_err(|e| format!("MFRC522 init: {e:?}"))?;
        let version = reader
            .version()
            .map_err(|e| format!("version read: {e:?}"))?;
        if version != 0x91 && version != 0x92 {
            return Err(format!("unexpected MFRC522 version 0x{version:02X}"));
        }
        reader
            .set_antenna_gain(mfrc522::RxGain::DB33)
            .map_err(|e| format!("antenna gain: {e:?}"))?;
        Ok(Self { reader })
    }

    /// One ISO 14443-3 activation attempt: REQA -> select (full UID,
    /// cascade included) -> HLTA. Ok(None) = field quiet.
    pub fn scan_once(
        &mut self,
    ) -> Result<Option<(mfrc522::AtqA, mfrc522::Uid)>, String> {
        match self.reader.reqa() {
            Ok(atqa) => {
                let uid = self
                    .reader
                    .select(&atqa)
                    .map_err(|e| format!("select: {e:?}"))?;
                let _ = self.reader.hlta();
                Ok(Some((atqa, uid)))
            }
            Err(mfrc522::Error::Timeout) => Ok(None),
            Err(e) => Err(format!("reqa: {e:?}")),
        }
    }
}

/// M5StickC Plus Grove I2C pins (SDA 32 / SCL 33) — the same map the
/// nucula fork's `board.h` M5STICK lane uses for its MFRC522.
pub fn m5stick_i2c(
    i2c0: esp_idf_svc::hal::i2c::I2C0<'static>,
    sda: impl esp_idf_svc::hal::gpio::InputPin + esp_idf_svc::hal::gpio::OutputPin + 'static,
    scl: impl esp_idf_svc::hal::gpio::InputPin + esp_idf_svc::hal::gpio::OutputPin + 'static,
) -> Result<i2c::I2cDriver<'static>, String> {
    let config = i2c::config::Config::new().baudrate(Hertz(100_000).into());
    i2c::I2cDriver::new(i2c0, sda, scl, &config).map_err(|e| format!("i2c0: {e:?}"))
}
