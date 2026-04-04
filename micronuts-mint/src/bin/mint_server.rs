//! Explicit mint-role demo binary.
//!
//! Build and run:
//!   cargo run -p micronuts-mint --bin mint_server
//!
//! Protocol:
//!   stdin:  one hex-encoded CBOR `MintRpcRequest` per line
//!   stdout: one hex-encoded CBOR `MintRpcResponse` per line

fn main() -> Result<(), Box<dyn std::error::Error>> {
    micronuts_mint::run_mint_server_stdio()
}
