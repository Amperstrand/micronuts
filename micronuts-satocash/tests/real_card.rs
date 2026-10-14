//! Real-card e2e against the bench nucula chain (ccid-firmware-rs).
//!
//! Runs only where a reader is present; skipped otherwise so plain
//! `cargo test` stays green on CI:
//!
//! ```text
//! SATOCASH_READER="Nucula VPCD 00 00" \
//!   cargo test -p micronuts-satocash --features pcsc --test real_card -- --nocapture
//! ```
//!
//! On the bench (ai-legion) the full chain is
//! pcscd → libifdvpcd (board connects out to :35963) → nucula
//! wifi-vpcd firmware → PN7160 NFC → javacard with SatoCash v0.1.

#![cfg(feature = "pcsc")]

use micronuts_satocash::pcsc::PcscTransport;
use micronuts_satocash::SatocashClient;

#[test]
fn real_card_select_and_status() {
    let Ok(reader) = std::env::var("SATOCASH_READER") else {
        eprintln!("skipping: SATOCASH_READER not set (no reader on this host)");
        return;
    };

    let transport = PcscTransport::connect_by_name(&reader).expect("reader + card present");
    assert!(!transport.atr().expect("ATR").is_empty());
    let mut client = SatocashClient::new(transport);

    let st = client.select().expect("SatoCash applet answers SELECT");
    println!(
        "applet v{}.{} protocol v{}.{}",
        st.applet_version.0, st.applet_version.1, st.protocol_version.0, st.protocol_version.1
    );
    assert_eq!(st.applet_version, (0, 1), "bench card runs SatoCash v0.1");
    assert!(
        st.needs_secure_channel,
        "virgin bench card requires the secure channel"
    );
    assert!(
        !st.setup_done,
        "bench card must stay virgin (no SETUP from tests)"
    );
    assert!(st.max_mints > 0 && st.max_keysets > 0 && st.max_proofs > 0);

    let st2 = client
        .satocash_get_status()
        .expect("SATOCASH_GET_STATUS without channel");
    assert_eq!(
        st2, st,
        "SELECT and SATOCASH_GET_STATUS report identical status"
    );

    let gs = client.get_status().expect("GET_STATUS without channel");
    assert_eq!(gs.applet_version, st.applet_version);
    assert_eq!(gs.needs_secure_channel, st.needs_secure_channel);

    // Storage ops are channel-gated on real hardware: 9C20 until the
    // secure-channel milestone. Assert the card enforces it.
    let err = client
        .import_mint(b"https://mint.example")
        .expect_err("must be rejected");
    assert_eq!(
        err,
        micronuts_satocash::Error::Sw(micronuts_satocash::sw::SECURE_CHANNEL_REQUIRED)
    );
}
