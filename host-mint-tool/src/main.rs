use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod mint;
mod protocol;

#[derive(Parser)]
#[command(name = "mint-tool")]
#[command(about = "Demo mint signer for Micronuts hardware wallet")]
struct Cli {
    #[arg(short, long)]
    port: PathBuf,

    #[arg(short, long, default_value = "115200")]
    baud: u32,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    GenerateToken {
        #[arg(short, long, default_value = "1000")]
        amount: u64,
    },
    Sign,
    Export,
    Monitor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt::init();

    match cli.command {
        Commands::GenerateToken { amount } => {
            println!("Generating test token with amount: {}", amount);
        }
        Commands::Sign => {
            println!("Signing blinded outputs");
        }
        Commands::Export => {
            println!("Exporting proofs");
        }
        Commands::Monitor => {
            println!("Monitoring USB connection");
        }
    }

    Ok(())
}
