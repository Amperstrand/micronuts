use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    #[cfg(feature = "uart-log")]
    {
        let defmt_x = out.join("defmt.x");
        std::fs::write(defmt_x, "").unwrap();
        println!("cargo:rustc-link-search=native={}", out.display());
    }

    println!("cargo:rerun-if-changed=../Cargo.toml");

    let git_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir("..")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    let git_date = Command::new("git")
        .args(["log", "-1", "--format=%ci"])
        .current_dir("..")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_DATE={}", git_date);

    let build_date = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_DATE={}", build_date);

    let workspace_toml = std::fs::read_to_string("../Cargo.toml").unwrap_or_default();

    let embassy_rev = extract_rev(&workspace_toml, "embassy");
    println!("cargo:rustc-env=EMBASSY_REV={}", embassy_rev);

    let bsp_rev = extract_rev_exact(&workspace_toml, "embassy-stm32f469i-disco");
    println!("cargo:rustc-env=BSP_REV={}", bsp_rev);

    let gm65_rev = extract_rev_exact(&workspace_toml, "gm65-scanner");
    println!("cargo:rustc-env=GM65_REV={}", gm65_rev);
}

fn extract_rev(toml: &str, crate_name: &str) -> String {
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if !trimmed.contains(crate_name) {
            continue;
        }
        if let Some(pos) = trimmed.find("rev = \"") {
            let start = pos + 7;
            if let Some(end) = trimmed[start..].find('"') {
                return trimmed[start..start + end].to_string();
            }
        }
    }
    "unknown".to_string()
}

fn extract_rev_exact(toml: &str, package_name: &str) -> String {
    let search = format!("package = \"{}\"", package_name);
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if !trimmed.contains(&search) && !trimmed.contains(&format!("{} =", package_name)) {
            continue;
        }
        if let Some(pos) = trimmed.find("rev = \"") {
            let start = pos + 7;
            if let Some(end) = trimmed[start..].find('"') {
                return trimmed[start..start + end].to_string();
            }
        }
    }
    "unknown".to_string()
}
