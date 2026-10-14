//! Bench-truth replay: the captured wire vectors ARE the mock.
//!
//! `bench-vectors.json` was captured through the real chain (pcscd →
//! libifdvpcd → nucula wifi-vpcd → PN7160 → SatoCash v0.1; see the
//! `provenance` block) by `scripts/capture_wire_vectors.py`. This test
//! proves (1) our encoders emit byte-identical commands and (2) our
//! parsers decode the captured responses — against card truth, not
//! hand-written expectations. Re-capture on bench sessions and review
//! the diff; a change here means the card or chain changed.

use micronuts_satocash::encode_apdu as enc;
use micronuts_satocash::{CardTransport, Error, SatocashClient};
use serde_json::Value;
use std::collections::HashMap;

struct Recording {
    script: HashMap<Vec<u8>, (Vec<u8>, u16)>,
}

impl CardTransport for Recording {
    fn exchange(
        &mut self,
        cla: u8,
        ins: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
    ) -> Result<(Vec<u8>, u16), Error> {
        let cmd = enc(cla, ins, p1, p2, data);
        self.script.get(&cmd).cloned().ok_or(Error::Sw(0x6D00))
    }
}

fn vectors() -> Vec<(String, Vec<u8>, Vec<u8>, u16)> {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/bench-vectors.json"))
        .expect("bench-vectors.json committed beside the crate");
    let v: Value = serde_json::from_str(&raw).expect("valid json");
    assert!(
        v["provenance"]["chain"]
            .as_str()
            .unwrap()
            .contains("ccid-firmware-rs"),
        "fixture provenance must name the bench chain"
    );
    v["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().to_string(),
                hex(e["command"].as_str().unwrap()),
                hex(e["response"].as_str().unwrap()),
                u16::from_str_radix(e["sw"].as_str().unwrap(), 16).unwrap(),
            )
        })
        .collect()
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn encoders_reproduce_captured_command_bytes() {
    let mut script = HashMap::new();
    for (_, cmd, resp, sw) in vectors() {
        script.insert(cmd, (resp, sw));
    }
    let mut client = SatocashClient::new(Recording { script });

    let st = client.select().expect("captured SELECT answers 9000");
    assert_eq!(st.protocol_version, (0, 1));
    assert_eq!(st.applet_version, (0, 1));
    assert!(!st.setup_done);
    assert!(st.needs_secure_channel);
    assert_eq!(st.max_mints, 16);
    assert_eq!(st.max_keysets, 32);
    assert_eq!(st.max_proofs, 128);
    assert_eq!(st.token_count(), 0);

    let gs = client
        .get_status()
        .expect("captured GET_STATUS answers 9000");
    assert_eq!(gs.applet_version, (0, 1));
    assert!(gs.needs_secure_channel);

    assert_eq!(
        client
            .satocash_get_status()
            .expect("captured SATOCASH_GET_STATUS"),
        st
    );

    assert_eq!(
        client.import_mint(b"https://mint.example"),
        Err(Error::Sw(0x9C20)),
        "virgin card gates storage ops on the secure channel"
    );
}

#[test]
fn captured_wrong_aid_select_is_6a82() {
    let (_, cmd, resp, sw) = vectors()
        .into_iter()
        .find(|(n, _, _, _)| n == "select_wrong_aid")
        .expect("wrong-aid vector present");
    assert_eq!(sw, 0x6A82);
    assert!(resp.is_empty());
    assert_eq!(cmd.len(), 11); // header + Lc + 5-byte AID + Le (JCRE-routed, Le tolerated)
}
