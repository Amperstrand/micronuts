use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::Parser;
use k256::PublicKey;
use std::path::PathBuf;

mod mint;
mod protocol;
mod usb;

use mint::DemoMint;
use protocol::{
    Frame, CMD_GET_BLINDED, CMD_GET_PROOFS, CMD_IMPORT_TOKEN, CMD_SEND_SIGNATURES, STATUS_OK,
};
use usb::UsbConnection;

#[derive(Parser)]
#[command(name = "mint-tool")]
#[command(about = "Demo mint signer for Micronuts hardware wallet")]
struct Cli {
    #[arg(short, long)]
    port: Option<PathBuf>,

    #[arg(short, long, default_value = "115200")]
    baud: u32,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    List,
    Generate {
        #[arg(short, long, default_value = "1000")]
        amount: u64,
    },
    Blind,
    Sign,
    Export,
    Monitor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt::init();

    match cli.command {
        Commands::List => {
            let devices = UsbConnection::list_devices()?;
            if devices.is_empty() {
                println!("No USB serial devices found");
            } else {
                println!("Available USB serial devices:");
                for dev in devices {
                    println!("  {}", dev);
                }
            }
        }
        Commands::Generate { amount } => {
            let port = cli.port.context("--port is required for this command")?;
            let mut usb = UsbConnection::open(&port, cli.baud)?;

            let mint = DemoMint::new();
            let token = generate_test_token(&mint, amount)?;

            let frame = Frame::new(CMD_IMPORT_TOKEN, token.clone());
            let response = usb.send_and_receive(&frame)?;

            if response.command == STATUS_OK {
                println!("Generated and imported token with amount: {}", amount);
                println!("Mint public key: {:?}", mint.public_key());
            } else {
                anyhow::bail!("Device returned error status: 0x{:02X}", response.command);
            }
        }
        Commands::Blind => {
            let port = cli.port.context("--port is required for this command")?;
            let mut usb = UsbConnection::open(&port, cli.baud)?;

            let frame = Frame::new(CMD_GET_BLINDED, vec![]);
            let response = usb.send_and_receive(&frame)?;

            if response.command == STATUS_OK {
                println!("Device generated blinded outputs");
                println!("Blinded data length: {} bytes", response.payload.len());
            } else {
                anyhow::bail!("Device returned error status: 0x{:02X}", response.command);
            }
        }
        Commands::Sign => {
            let port = cli.port.context("--port is required for this command")?;
            let mut usb = UsbConnection::open(&port, cli.baud)?;

            let mint = DemoMint::new();

            let request_frame = Frame::new(CMD_GET_BLINDED, vec![]);
            let blinded_response = usb.send_and_receive(&request_frame)?;

            if blinded_response.command != STATUS_OK {
                anyhow::bail!(
                    "Failed to get blinded outputs: 0x{:02X}",
                    blinded_response.command
                );
            }

            let signatures = sign_blinded_outputs(&mint, &blinded_response.payload)?;

            let sign_frame = Frame::new(CMD_SEND_SIGNATURES, signatures);
            let response = usb.send_and_receive(&sign_frame)?;

            if response.command == STATUS_OK {
                println!("Signed blinded outputs successfully");
            } else {
                anyhow::bail!("Device returned error status: 0x{:02X}", response.command);
            }
        }
        Commands::Export => {
            let port = cli.port.context("--port is required for this command")?;
            let mut usb = UsbConnection::open(&port, cli.baud)?;

            let frame = Frame::new(CMD_GET_PROOFS, vec![]);
            let response = usb.send_and_receive(&frame)?;

            if response.command == STATUS_OK {
                println!("Exported proofs successfully");
                println!("Proof data length: {} bytes", response.payload.len());

                let encoded = STANDARD.encode(&response.payload);
                println!("Token: cashuB{}", encoded);
            } else {
                anyhow::bail!("Device returned error status: 0x{:02X}", response.command);
            }
        }
        Commands::Monitor => {
            let port = cli.port.context("--port is required for this command")?;
            let mut usb = UsbConnection::open(&port, cli.baud)?;

            println!("Monitoring USB connection. Press Ctrl+C to stop.");
            loop {
                match usb.receive_frame() {
                    Ok(frame) => {
                        println!(
                            "Received: cmd=0x{:02X}, len={}",
                            frame.command,
                            frame.payload.len()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Receive error: {}", e);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }
    }

    Ok(())
}

fn generate_test_token(mint: &DemoMint, amount: u64) -> Result<Vec<u8>> {
    use cashu_core_lite::{blind_message, Proof, TokenV4, TokenV4Token};
    use rand::RngCore;

    let mut proofs = Vec::new();
    let mut remaining = amount;

    let keyset_id = "00".to_string();

    while remaining > 0 {
        let value = 2u64.pow(remaining.ilog2());
        remaining -= value;

        let mut secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);

        let blinded = blind_message(&secret, None)?;
        let blinded_sig = mint.sign(&blinded.blinded);
        let sig =
            cashu_core_lite::unblind_signature(&blinded_sig, &blinded.blinder, &mint.public_key())
                .map_err(|_| anyhow::anyhow!("Failed to unblind signature"))?;

        let c = sig.to_sec1_bytes().to_vec();

        proofs.push(Proof {
            amount: value,
            keyset_id: keyset_id.clone(),
            secret: hex::encode(secret),
            c,
        });
    }

    let token = TokenV4 {
        mint: "demo://micronuts".to_string(),
        unit: "sat".to_string(),
        memo: Some("Generated test token".to_string()),
        tokens: vec![TokenV4Token { keyset_id, proofs }],
    };

    let encoded = cashu_core_lite::encode_token(&token)?;
    Ok(encoded)
}

fn sign_blinded_outputs(mint: &DemoMint, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() % 33 != 0 {
        anyhow::bail!("Invalid blinded outputs payload: length not multiple of 33");
    }

    let mut signatures = Vec::new();

    for chunk in payload.chunks(33) {
        let blinded = PublicKey::from_sec1_bytes(chunk).context("Invalid blinded public key")?;
        let sig = mint.sign(&blinded);
        signatures.extend_from_slice(&sig.to_sec1_bytes());
    }

    Ok(signatures)
}
