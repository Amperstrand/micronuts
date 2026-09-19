//! Receive a Python/coincurve-minted token through the Rust WalletEngine.
fn main() {
    let token = std::env::args().nth(1).expect("token as arg");
    let mint = "http://127.0.0.1:3338";

    let transport = micronuts_wallet::http::http_mint_client(mint);
    let store = cashu_core_lite::store::MemoryStore::new();
    let seed: [u8; 32] = [42u8; 32];

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
    match engine.receive_token(&token) {
        Ok(amount) => println!(
            "SUCCESS: received {amount} sats, balance: {} sats",
            engine.balance()
        ),
        Err(e) => println!("receive error: {e:?}"),
    }
}
