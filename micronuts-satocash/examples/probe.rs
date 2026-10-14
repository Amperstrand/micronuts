//! Probe a SatoCash card over PC/SC: SELECT + status dump.
//!
//! Usage: `satocash-probe [reader-name-substring]` (default "Nucula").
//! Exit 0 = applet answered; exit 1 = transport/SW error (printed).

#[cfg(feature = "pcsc")]
fn main() {
    let filter = std::env::args().nth(1).unwrap_or_else(|| "Nucula".into());
    match probe(&filter) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("satocash-probe: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "pcsc")]
fn probe(filter: &str) -> Result<(), Box<dyn std::error::Error>> {
    let transport = micronuts_satocash::pcsc::PcscTransport::connect_by_name(filter)?;
    println!("reader: {filter}  ATR: {}", hex(&transport.atr()?));
    let mut client = micronuts_satocash::SatocashClient::new(transport);
    match client.select() {
        Ok(st) => {
            println!(
                "SatoCash applet v{}.{} (protocol v{}.{})",
                st.applet_version.0,
                st.applet_version.1,
                st.protocol_version.0,
                st.protocol_version.1
            );
            println!(
                "setup_done={} secure_channel_required={} 2FA={}",
                st.setup_done, st.needs_secure_channel, st.needs_2fa
            );
            println!(
                "mints {}/{}  keysets {}/{}  proofs {}/{} unspent ({} spent)",
                st.used_mints,
                st.max_mints,
                st.used_keysets,
                st.max_keysets,
                st.unspent_proofs,
                st.max_proofs,
                st.spent_proofs
            );
            Ok(())
        }
        Err(micronuts_satocash::Error::Sw(0x6A82)) => {
            println!("card reachable, SatoCash applet NOT installed (SW 6A82)");
            std::process::exit(2);
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(feature = "pcsc")]
fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(not(feature = "pcsc"))]
fn main() {
    eprintln!("satocash-probe: rebuild with --features pcsc");
    std::process::exit(1);
}
