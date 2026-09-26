//! Typed WiFi manager (bolty-rs `apps/bolty-esp32/src/wifi.rs` pattern):
//! owns `BlockingWifi<EspWifi<'static>>`, deterministic connect lifecycle
//! (disconnect → stop → configure → start → connect → wait_netif_up),
//! bounded credentials, typed errors.

use core::fmt;

use esp_idf_hal::modem::Modem;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};
use esp_idf_sys::EspError;
use log::info;

const MAX_SSID_LEN: usize = 32;
const MAX_PASSWORD_LEN: usize = 64;

#[derive(Debug)]
pub enum WifiError {
    SsidTooLong,
    PasswordTooLong,
    /// Station has no DHCP lease yet (gateway still 0.0.0.0).
    NoLease,
    Esp(EspError),
}

impl fmt::Display for WifiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SsidTooLong => f.write_str("ssid too long"),
            Self::PasswordTooLong => f.write_str("password too long"),
            Self::NoLease => f.write_str("no DHCP lease (gateway unset)"),
            Self::Esp(err) => write!(f, "{err}"),
        }
    }
}

impl From<EspError> for WifiError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl std::error::Error for WifiError {}

pub struct WifiManager {
    wifi: BlockingWifi<EspWifi<'static>>,
}

impl WifiManager {
    /// `nvs` is passed in (not taken here): esp-idf-svc guards the default
    /// partition with a one-take flag — a second `take()` while another
    /// holder is alive fails with `ESP_ERR_INVALID_STATE`. main takes it
    /// once and Arc-shares clones (the mint's `NvsStateStore` keeps one).
    pub fn new(modem: Modem<'static>, nvs: EspDefaultNvsPartition) -> Result<Self, WifiError> {
        let sys_loop = EspSystemEventLoop::take()?;
        let wifi = BlockingWifi::wrap(EspWifi::new(modem, sys_loop.clone(), Some(nvs))?, sys_loop)?;
        Ok(Self { wifi })
    }

    pub fn is_connected(&self) -> bool {
        self.wifi.is_connected().unwrap_or(false)
    }

    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), WifiError> {
        if ssid.len() > MAX_SSID_LEN {
            return Err(WifiError::SsidTooLong);
        }
        if password.len() > MAX_PASSWORD_LEN {
            return Err(WifiError::PasswordTooLong);
        }

        if self.wifi.is_connected()? {
            self.wifi.disconnect()?;
        }
        if self.wifi.is_started()? {
            self.wifi.stop()?;
        }

        let wifi_configuration = Configuration::Client(ClientConfiguration {
            ssid: ssid.try_into().map_err(|_| WifiError::SsidTooLong)?,
            password: password
                .try_into()
                .map_err(|_| WifiError::PasswordTooLong)?,
            // Open networks (captive portals) authenticate with None —
            // an empty passphrase + WPA2Personal never associates.
            auth_method: if password.is_empty() {
                AuthMethod::None
            } else {
                AuthMethod::WPA2Personal
            },
            bssid: None,
            channel: None,
            ..Default::default()
        });

        self.wifi.set_configuration(&wifi_configuration)?;
        self.wifi.start()?;
        self.wifi.connect()?;
        self.wifi.wait_netif_up()?;

        let ip_info = self.wifi.wifi().sta_netif().get_ip_info()?;
        info!("WiFi connected: ip={}", ip_info.ip);
        Ok(())
    }

    /// DHCP-lease gateway of the station interface. TollGate discovery
    /// targets the backend on the gateway — never a hardcoded address.
    pub fn gateway_ip(&self) -> Result<String, WifiError> {
        let ip_info = self.wifi.wifi().sta_netif().get_ip_info()?;
        let gw = ip_info.subnet.gateway;
        if gw.is_unspecified() {
            return Err(WifiError::NoLease);
        }
        Ok(gw.to_string())
    }
}
