//! ESP32 front-end for the micronuts Cashu mint.
//!
//! Split (ccid-firmware-rs pattern): [`json`] holds pure response-body
//! builders with no ESP-IDF dependency; `wifi`/`http` glue lives behind
//! `target_os = "espidf"` so the pure parts can move to a host-test tier
//! unchanged.

#[cfg(target_os = "espidf")]
pub mod wifi;

/// Pure JSON response builders for the NUT GET endpoints.
///
/// Hand-rolled (tollgate-s3-rs `/api/info` style) instead of serde_json to
/// keep the device dependency surface minimal. Safe because every formatted
/// value is hex, a fixed literal, or a u64 — no user-controlled strings.
pub mod json {
    use cashu_core_lite::nuts::{nut01, nut02};
    use micronuts_mint::DemoMint;

    /// GET /v1/info — NUT-06.
    ///
    /// `nuts` is a map of nut-number → settings (spec shape, mirrors the
    /// cashu-cf reference): 4/5 advertise bolt11+sat, the rest empty objects.
    pub fn info_body(mint: &DemoMint) -> String {
        let info = match mint.get_info() {
            Ok(info) => info,
            Err(e) => return super::json::error_body(500, &e.to_string()),
        };
        format!(
            concat!(
                "{{\"name\":\"{}\",\"pubkey\":\"{}\",\"version\":\"{}\",",
                "\"description\":\"{}\",\"contact\":[],",
                "\"nuts\":{{\"4\":{{\"methods\":[{{\"method\":\"bolt11\",\"unit\":\"sat\"}}]}},",
                "\"5\":{{\"methods\":[{{\"method\":\"bolt11\",\"unit\":\"sat\"}}]}},",
                "\"3\":{{}},\"6\":{{}},\"7\":{{}},\"9\":{{}}}}}}"
            ),
            info.name, info.pubkey, info.version, info.description,
        )
    }

    /// GET /v1/keys — NUT-01.
    pub fn keys_body(mint: &DemoMint) -> String {
        let keys: Vec<String> = match mint.get_keys() {
            Ok(resp) => resp
                .keysets
                .iter()
                .map(keyset_json)
                .collect(),
            Err(e) => return super::json::error_body(500, &e.to_string()),
        };
        format!("{{\"keysets\":[{}]}}", keys.join(","))
    }

    /// GET /v1/keysets — NUT-02.
    pub fn keysets_body(mint: &DemoMint) -> String {
        let infos: Vec<String> = match mint.get_keysets() {
            Ok(resp) => resp.keysets.iter().map(keyset_info_json).collect(),
            Err(e) => return super::json::error_body(500, &e.to_string()),
        };
        format!("{{\"keysets\":[{}]}}", infos.join(","))
    }

    /// NUT-00-style error envelope (code + error).
    pub fn error_body(status: u16, message: &str) -> String {
        format!(
            "{{\"code\":{},\"error\":\"{}\"}}",
            status,
            message.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }

    fn keyset_json(ks: &nut01::KeySet) -> String {
        let keys: Vec<String> = ks
            .keys
            .iter()
            .map(|kp| format!("\"{}\":\"{}\"", kp.amount, hex::encode(kp.pubkey.to_bytes())))
            .collect();
        format!(
            "{{\"id\":\"{}\",\"unit\":\"{}\",\"keys\":{{{}}}}}",
            ks.id,
            ks.unit,
            keys.join(",")
        )
    }

    fn keyset_info_json(info: &nut02::KeysetInfo) -> String {
        format!(
            "{{\"id\":\"{}\",\"unit\":\"{}\",\"active\":{},\"input_fee_ppk\":{}}}",
            info.id, info.unit, info.active, info.input_fee_ppk
        )
    }
}
