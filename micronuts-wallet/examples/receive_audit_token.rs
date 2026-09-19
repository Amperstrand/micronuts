//! Receive a Python/coincurve-minted token through the Rust WalletEngine.
//! `--p2pk-pubkey` prints the wallet identity's lock-facing public key.
//! Optional second arg: the HTLC preimage for NUT-14 tokens.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: [u8; 32] = [42u8; 32];
    if args.iter().any(|a| a == "--p2pk-pubkey") {
        let identity = micronuts_wallet_core::conditions::derive_identity(&seed).unwrap();
        println!("{}", micronuts_wallet_core::conditions::p2pk_pubkey_hex(&identity));
        return;
    }
    let token = args.get(1).expect("token as arg").clone();
    let preimage = args.get(2).map(String::as_str);
    let mint = "http://127.0.0.1:3338";

    let transport = micronuts_wallet::http::http_mint_client(mint);
    let store = cashu_core_lite::store::MemoryStore::new();

    let mut engine = match micronuts_wallet_core::engine::WalletEngine::new(
        mint,
        transport,
        store,
        seed,
        Vec::new(),
    ) {
        Ok(e) => e,
        Err(e) => {
            println!("engine construct: {e:?}");
            return;
        }
    };

    if let Err(e) = engine.connect() {
        println!("connect: {e:?}");
        return;
    }
    println!(
        "connected: mint={}, keyset={}",
        engine.mint_url(),
        engine.keyset_id()
    );
    // Parse first to show the token's shape (V3 or V4)
    match micronuts_wallet_core::token_compat::decode_token_any(&token) {
        Ok(t) => println!(
            "token: mint={}, unit={}, {} proofs, {} sats (V3 or V4, both work)",
            t.mint,
            t.unit,
            t.tokens.iter().map(|g| g.proofs.len()).sum::<usize>(),
            t.tokens
                .iter()
                .flat_map(|g| g.proofs.iter())
                .map(|p| p.amount)
                .sum::<u64>()
        ),
        Err(e) => {
            println!("token parse: {e:?}");
            return;
        }
    }
    println!("receiving Python-minted token...");
    let options = micronuts_wallet_core::engine::ReceiveOptions { preimage };
    match engine.receive_token_with(&token, options) {
        Ok(amount) => println!(
            "SUCCESS: received {amount} sats, balance: {} sats",
            engine.balance()
        ),
        Err(e) => println!("receive error: {e:?}"),
    }
}
