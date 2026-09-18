//! Parse and summarize a Cashu V4 token from the F469 hardware wallet.
fn main() {
    let token_str = std::env::args().nth(1).expect("token as arg");
    // decode_token expects the raw CBOR bytes; the wire form is base64url
    let stripped = token_str
        .strip_prefix("cashuB")
        .or_else(|| token_str.strip_prefix("cashuA"))
        .unwrap_or(&token_str);
    use base64::Engine;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(stripped)
        .expect("base64url decode");
    match cashu_core_lite::token::decode_token(&raw) {
        Ok(t) => {
            let total: u64 = t
                .tokens
                .iter()
                .flat_map(|tok| tok.proofs.iter())
                .map(|p| p.amount)
                .sum();
            println!(
                "V4 token: unit={}, {} proofs, {} sat total",
                t.unit,
                t.tokens.iter().map(|tok| tok.proofs.len()).sum::<usize>(),
                total
            );
            println!("mint: {}", t.mint);
            for (i, p) in t
                .tokens
                .iter()
                .flat_map(|tok| tok.proofs.iter())
                .enumerate()
            {
                println!(
                    "  proof[{}]: {} sat, keyset={}, dleq={}",
                    i,
                    p.amount,
                    p.keyset_id,
                    if p.dleq.is_some() { "yes" } else { "no" }
                );
            }
        }
        Err(e) => println!("parse error: {e:?}"),
    }
}
