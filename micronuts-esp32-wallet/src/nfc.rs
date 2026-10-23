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

/// Latched-RC522 recovery (port of the nucula `axp192_grove_power`
/// pattern, display_st7789.c): the M5Stick's AXP192 PMU (I2C1 @0x34,
/// pins 21/22) holds the Grove rail across ESP32 reboots, so a latched
/// reader (SDA stuck LOW, immune to SCL clocking) only clears when
/// EXTEN (reg 0x10 bit 2) actually drops. One rail cycle before the
/// first MFRC522 init clears the latch.
pub fn axp192_grove_rail_cycle() -> Result<(), String> {
    use esp_idf_svc::hal::i2c;
    use esp_idf_svc::hal::peripherals::Peripherals;

    let periph = unsafe { Peripherals::steal() };
    let config = i2c::config::Config::new().baudrate(esp_idf_svc::hal::units::Hertz(400_000));
    let mut driver = i2c::I2cDriver::new(
        periph.i2c1,
        periph.pins.gpio21,
        periph.pins.gpio22,
        &config,
    )
    .map_err(|e| format!("axp192 i2c1: {e:?}"))?;

    let mut buf = [0u8; 1];
    driver
        .write_read(0x34, &[0x10], &mut buf, 200)
        .map_err(|e| format!("axp192 read: {e:?}"))?;
    let cur = buf[0];
    driver
        .write(0x34, &[0x10, cur & !0x04], 200) // EXTEN off — rail drops
        .map_err(|e| format!("axp192 write off: {e:?}"))?;
    esp_idf_svc::hal::delay::Ets::delay_ms(80);
    driver
        .write(0x34, &[0x10, cur | 0x04], 200) // rail back up
        .map_err(|e| format!("axp192 write on: {e:?}"))?;
    esp_idf_svc::hal::delay::Ets::delay_ms(20);
    Ok(())
}

impl NfcFrontend {
    pub fn new(i2c: i2c::I2cDriver<'static>) -> Result<Self, String> {
        // Clear any latched reader left by a previous session — the PMU
        // keeps the rail up across reboots, so plain re-init is not
        // enough (bench-proven class, AGENTS lore).
        if let Err(e) = axp192_grove_rail_cycle() {
            println!("nfc: axp192 rail cycle skipped: {e}");
        }
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
    ///
    /// Two bench-doctrine rules from nucula's rc522.c, ported whole:
    /// 1. PER-SCAN frontend reset ritual (antenna cycle, framing
    ///    restore, 100%-ASK + CRC preset, 4x FIFO flush, RxGain DB33,
    ///    conductance boost) — clears the marginal front-end states
    ///    that make every REQA return IncompleteFrame.
    /// 2. ANTENNA OFF WHEN IDLE ("a co-located writer — e.g. the
    ///    bench ACR1252 — never sees two fields"). Leaving the carrier
    ///    on wedges a co-located ACR1252 at ITS next power-on
    ///    (bench-proven 2026-10-22: a fresh ACR plug wedged in ~10 s
    ///    next to a radiating coil).
    pub fn scan_once(
        &mut self,
    ) -> Result<Option<(mfrc522::AtqA, mfrc522::Uid)>, String> {
        use mfrc522::Register;
        macro_rules! w {
            ($r:expr, $v:expr) => {{
                let _ = self.reader.write_register($r, $v);
            }};
        }
        macro_rules! antenna_off {
            () => {{
                let _ = self
                    .reader
                    .rmw_register(Register::TxControlReg, |b| b & !0b11);
            }};
        }
        // -- ritual: reset the front end, then field on --
        antenna_off!();
        w!(Register::TxModeReg, 0x00);
        w!(Register::RxModeReg, 0x00);
        w!(Register::ModWidthReg, 0x26);
        w!(Register::TxASKReg, 0x40); // FORCE_100_ASK
        w!(Register::ModeReg, 0x3D);  // CRC preset 0x6363
        // Marginal-coupling boost: max n-driver + modulated p-driver
        // conductance (default GsN=0x08 starves a small antenna).
        w!(Register::GsNReg, 0xBF);
        w!(Register::ModGsPReg, 0x3F);
        for _ in 0..4 {
            w!(Register::FIFOLevelReg, 0x80); // FIFO flush
        }
        w!(Register::RFCfgReg, 0x40); // RxGain DB33
        let _ = self
            .reader
            .rmw_register(Register::TxControlReg, |b| b | 0b11); // field on
        esp_idf_svc::hal::delay::Ets::delay_ms(10); // settle

        // -- one activation, then field OFF regardless of outcome --
        let result = match self.reader.reqa() {
            Ok(atqa) => match self.reader.select(&atqa) {
                Ok(uid) => {
                    let _ = self.reader.hlta();
                    Ok(Some((atqa, uid)))
                }
                Err(e) => Err(format!("select: {e:?}")),
            },
            Err(mfrc522::Error::Timeout) => Ok(None),
            Err(e) => Err(format!("reqa: {e:?}")),
        };
        antenna_off!();
        result
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
