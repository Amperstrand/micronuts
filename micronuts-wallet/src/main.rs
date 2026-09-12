//! `micronuts-wallet` binary entry.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--demo") {
        if let Err(err) = run_demo() {
            eprintln!("FAIL: {err}");
            std::process::exit(1);
        }
        return;
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

/// Full wallet cycle against a live mint (default
/// `http://127.0.0.1:3030` — run `micronuts-audit-adapter` first, or point
/// `MICRONUTS_WALLET_MINT` at any Cashu mint).
fn run_demo() -> Result<(), String> {
    use micronuts_wallet::engine::WalletEngine;
    use micronuts_wallet::http::HttpMintClient;
    use micronuts_wallet::state::FileStore;

    let mint_url = std::env::var("MICRONUTS_WALLET_MINT")
        .unwrap_or_else(|_| String::from("http://127.0.0.1:3030"));
    let dir = std::env::temp_dir().join("micronuts-wallet-demo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("demo dir: {e}"))?;

    let store = FileStore::new(dir.join("proofs.bin")).map_err(|e| format!("store: {e:?}"))?;
    let client = HttpMintClient::new(&mint_url);
    let mut engine = WalletEngine::new(&mint_url, client, store, [0xd0; 32], Vec::new())
        .map_err(|e| format!("engine: {e}"))?;

    engine.connect().map_err(|e| format!("connect: {e}"))?;
    println!(
        "STEP 1 OK: connected to '{}' keyset {} (fee {} ppk)",
        engine.mint_name(),
        engine.keyset_id(),
        engine.fee_ppk()
    );

    let quote = engine
        .mint_via_invoice(100)
        .map_err(|e| format!("mint quote: {e}"))?;
    let quote = engine
        .poll_mint_quote(&quote.quote)
        .map_err(|e| format!("quote poll: {e}"))?;
    if quote.state != "PAID" {
        return Err(format!("quote not paid: {}", quote.state));
    }
    let minted = engine
        .mint_paid_quote(&quote.quote, 100)
        .map_err(|e| format!("mint: {e}"))?;
    println!(
        "STEP 2 OK: minted {minted} sat via invoice (balance {})",
        engine.balance()
    );

    let token = engine
        .send_token(21, Some("demo"))
        .map_err(|e| format!("send: {e}"))?;
    let head: String = token.chars().take(24).collect();
    println!(
        "STEP 3 OK: token for 21 sat ({head}…) (balance {})",
        engine.balance()
    );

    let received = engine
        .receive_token(&token)
        .map_err(|e| format!("receive: {e}"))?;
    println!(
        "STEP 4 OK: received {received} sat back (balance {})",
        engine.balance()
    );

    let (_quote, outcome) = engine
        .melt("lnbcdemo30sat1micronuts")
        .map_err(|e| format!("melt: {e}"))?;
    if !outcome.paid {
        return Err(String::from("melt did not pay"));
    }
    println!(
        "STEP 5 OK: paid 30-sat invoice (preimage {}) (balance {})",
        outcome.preimage.as_deref().unwrap_or("-"),
        engine.balance()
    );

    let summary = engine
        .history()
        .iter()
        .map(|entry| format!("{:?}", entry.kind))
        .collect::<Vec<_>>()
        .join(", ");
    println!("STEP 6 OK: history [{summary}]");
    println!("Final balance: {} sats", engine.balance());
    Ok(())
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
