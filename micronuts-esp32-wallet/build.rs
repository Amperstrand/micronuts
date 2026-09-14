fn main() {
    // Credentials arrive via env ONLY (option_env! in main) — the secret
    // lives in the binary, never in the repo. Rerun when they change.
    println!("cargo:rerun-if-env-changed=MICRONUTS_WIFI_SSID");
    println!("cargo:rerun-if-env-changed=MICRONUTS_WIFI_PASS");
    embuild::espidf::sysenv::output();
}
