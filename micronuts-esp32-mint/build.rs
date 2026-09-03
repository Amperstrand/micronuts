fn main() {
    // Gate-2 W5: release-shaped builds must never bake placeholder WiFi
    // credentials (debug/CI builds may — the esp32-build CI job is debug).
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let has_creds = std::env::var_os("MICRONUTS_WIFI_SSID").is_some()
        && std::env::var_os("MICRONUTS_WIFI_PASS").is_some();
    if profile == "release" && !has_creds {
        panic!(
            "release build without MICRONUTS_WIFI_SSID/MICRONUTS_WIFI_PASS — \
             refusing to bake the in-source placeholders into a deployable \
             binary (set the credentials or build debug)"
        );
    }
    // option_env! is invisible to cargo change detection — rerun on the
    // credential env so stale-credential binaries are not produced.
    println!("cargo:rerun-if-env-changed=MICRONUTS_WIFI_SSID");
    println!("cargo:rerun-if-env-changed=MICRONUTS_WIFI_PASS");
    embuild::espidf::sysenv::output()
}
