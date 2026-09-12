//! `micronuts-wallet` binary entry.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--demo") {
        // Wired with the live-demo todo: full mint/send/receive/melt cycle.
        eprintln!("micronuts-wallet: --demo arrives with the engine");
        std::process::exit(2);
    }
    let dir = data_dir_from(&args);
    match micronuts_wallet::ui::run(dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("micronuts-wallet: UI error: {err}");
            std::process::exit(1);
        }
    }
}

/// `--dir <path>` overrides the data directory.
fn data_dir_from(args: &[String]) -> PathBuf {
    match args.iter().position(|a| a == "--dir") {
        Some(i) => args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir),
        None => default_data_dir(),
    }
}

/// `$XDG_DATA_HOME/micronuts-wallet` (or `~/.local/share/micronuts-wallet`).
fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                let mut path = PathBuf::from(home);
                path.push(".local/share");
                path
            })
        })
        .unwrap_or_default();
    base.join("micronuts-wallet")
}
