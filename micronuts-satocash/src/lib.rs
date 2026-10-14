//! SatoCash JavaCard client for micronuts.
//!
//! SatoCash (Toporin/Satocash-Applet) is a Cashu ecash wallet applet for
//! JavaCards: it stores mints, keysets and ecash proofs (68-byte objects:
//! state, keyset index, amount exponent, unblinded public key, 32-byte
//! secret) inside the secure element. This crate speaks the applet's
//! ISO 7816 APDU surface so a micronuts host wallet can use a SatoCash
//! card as its token store.
//!
//! Wire reference: `src/main/java/org/satocash/applet/Satocash.java`
//! (INS constants + `satocashGetStatus` layout), CLA `0xB0`, applet AID
//! `5361746f63617368` ("satocash").
//!
//! Four commands run without the ECDH/AES secure channel (applet
//! dispatcher Satocash.java:661): GET_STATUS, SATOCASH_GET_STATUS,
//! INIT_SECURE_CHANNEL and PROCESS_SECURE_CHANNEL. Everything else
//! (PIN, mint/keyset/proof import-export) is rejected until the channel
//! is established; that layer is the follow-up lane and the transport
//! seam here is shaped for it.

#![forbid(unsafe_code)]

use core::fmt;

#[cfg(feature = "pcsc")]
pub mod pcsc;

/// SatoCash applet AID: ASCII "satocash".
pub const SATOCASH_AID: [u8; 8] = [0x53, 0x61, 0x74, 0x6f, 0x63, 0x61, 0x73, 0x68];

/// CLA byte for every non-SELECT SatoCash command.
pub const CLA_SATOCASH: u8 = 0xB0;

/// Bench-verified short-APDU case forms (capture 2026-10-14,
/// `scripts/capture_wire_vectors.py`): empty data → header + Le
/// (case-2); data → header + Lc + data with NO trailing Le (case-3 —
/// this applet counts Le as incoming data; import_mint case-4 is
/// 6700). The 6-byte zero-data form (bare Lc=0 before Le) is 6700 AND
/// wedges the nucula board card-session — never emitted.
pub fn encode_apdu(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5 + data.len());
    cmd.extend_from_slice(&[cla, ins, p1, p2]);
    if data.is_empty() {
        cmd.push(0);
    } else {
        cmd.push(data.len() as u8);
        cmd.extend_from_slice(data);
    }
    cmd
}

/// Card status words beyond ISO 7816 the applet defines (Satocash.java).
pub mod sw {
    pub const SETUP_NOT_DONE: u16 = 0x9C04;
    pub const UNAUTHORIZED: u16 = 0x9C06;
    pub const NO_MEMORY_LEFT: u16 = 0x9C01;
    pub const INVALID_P1: u16 = 0x9C10;
    pub const INVALID_PARAMETER: u16 = 0x9C0F;
    pub const INCORRECT_P2: u16 = 0x9C11;
    pub const SECURE_CHANNEL_REQUIRED: u16 = 0x9C20;
}

pub mod wallet;

/// Instruction bytes (Satocash.java INS_* constants).
pub mod ins {
    pub const SETUP: u8 = 0x2A;
    pub const VERIFY_PIN: u8 = 0x42;
    pub const CHANGE_PIN: u8 = 0x44;
    pub const UNBLOCK_PIN: u8 = 0x46;
    pub const LOGOUT_ALL: u8 = 0x60;
    pub const GET_STATUS: u8 = 0x3C;
    pub const CARD_LABEL: u8 = 0x3D;
    pub const INIT_SECURE_CHANNEL: u8 = 0x81;
    pub const PROCESS_SECURE_CHANNEL: u8 = 0x82;
    pub const SATOCASH_GET_STATUS: u8 = 0xB0;
    pub const IMPORT_MINT: u8 = 0xB1;
    pub const EXPORT_MINT: u8 = 0xB2;
    pub const REMOVE_MINT: u8 = 0xB3;
    pub const IMPORT_KEYSET: u8 = 0xB4;
    pub const EXPORT_KEYSET: u8 = 0xB5;
    pub const REMOVE_KEYSET: u8 = 0xB6;
    pub const IMPORT_PROOF: u8 = 0xB7;
    pub const EXPORT_PROOFS: u8 = 0xB8;
    pub const GET_PROOF_INFO: u8 = 0xB9;
    pub const RESET_TO_FACTORY: u8 = 0xFF;
}

/// APDU status words the client surfaces distinctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Transport-level failure (reader gone, card removed, ...).
    Transport(&'static str),
    /// Card returned SW != 9000.
    Sw(u16),
    /// Applet answered but the response violated the documented layout.
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(m) => write!(f, "transport: {m}"),
            Error::Sw(sw) => write!(f, "card status word {sw:04X}"),
            Error::Malformed(m) => write!(f, "malformed response: {m}"),
        }
    }
}

impl core::error::Error for Error {}

/// One exchange with the card: command APDU in, response bytes + SW out.
pub trait CardTransport {
    fn exchange(
        &mut self,
        cla: u8,
        ins: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
    ) -> Result<(Vec<u8>, u16), Error>;
}

/// `satocashGetStatus` (INS 0xB0 / SELECT response) payload.
///
/// Layout per Satocash.java:1046-1100:
/// versions(4) | tries(4) | needs2FA(1) | RFU(1) | setupDone(1) |
/// needsSecureChannel(1) | nfcPolicy(1) | pinPolicy(1) | RFU(1) |
/// maxMints(1) | usedMints(1) | maxKeysets(1) | usedKeysets(1) |
/// maxProofs(2) | unspentProofs(2) | spentProofs(2) | ...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SatocashStatus {
    pub protocol_version: (u8, u8),
    pub applet_version: (u8, u8),
    pub pin_tries_remaining: u8,
    pub puk_tries_remaining: u8,
    pub needs_2fa: bool,
    pub setup_done: bool,
    pub needs_secure_channel: bool,
    pub nfc_policy: u8,
    pub pin_policy: u8,
    pub max_mints: u8,
    pub used_mints: u8,
    pub max_keysets: u8,
    pub used_keysets: u8,
    pub max_proofs: u16,
    pub unspent_proofs: u16,
    pub spent_proofs: u16,
}

impl SatocashStatus {
    /// Sum of ecash held on-card, in proof-count terms (each unspent
    /// proof is one Cashu token; the sats value needs keyset lookup).
    pub fn token_count(&self) -> u16 {
        self.unspent_proofs
    }

    pub fn parse(r: &[u8]) -> Result<Self, Error> {
        if r.len() < 22 {
            return Err(Error::Malformed("status shorter than 22 bytes"));
        }
        Ok(Self {
            protocol_version: (r[0], r[1]),
            applet_version: (r[2], r[3]),
            pin_tries_remaining: r[4],
            puk_tries_remaining: r[5],
            needs_2fa: r[8] == 1,
            setup_done: r[10] == 1,
            needs_secure_channel: r[11] == 1,
            nfc_policy: r[12],
            pin_policy: r[13],
            max_mints: r[15],
            used_mints: r[16],
            max_keysets: r[17],
            used_keysets: r[18],
            max_proofs: u16::from_be_bytes([r[19], r[20]]),
            unspent_proofs: u16::from_be_bytes([r[21], r[22]]),
            spent_proofs: u16::from_be_bytes([r[23], r[24]]),
        })
    }
}

/// `GET_STATUS` (INS 0x3C) answer: the general applet/session status —
/// versions, tries and flags, without the satocash capacities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneralStatus {
    pub protocol_version: (u8, u8),
    pub applet_version: (u8, u8),
    pub pin_tries_remaining: u8,
    pub puk_tries_remaining: u8,
    pub needs_2fa: bool,
    pub setup_done: bool,
    pub needs_secure_channel: bool,
}

impl GeneralStatus {
    pub fn parse(r: &[u8]) -> Result<Self, Error> {
        if r.len() < 13 {
            return Err(Error::Malformed("general status shorter than 13 bytes"));
        }
        Ok(Self {
            protocol_version: (r[0], r[1]),
            applet_version: (r[2], r[3]),
            pin_tries_remaining: r[4],
            puk_tries_remaining: r[5],
            needs_2fa: r[8] == 1,
            setup_done: r[10] == 1,
            needs_secure_channel: r[11] == 1,
        })
    }
}

/// SatoCash client over any card transport.
pub struct SatocashClient<T: CardTransport> {
    transport: T,
}

impl<T: CardTransport> SatocashClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    fn xfer(&mut self, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<(Vec<u8>, u16), Error> {
        self.transport.exchange(CLA_SATOCASH, ins, p1, p2, data)
    }

    /// SELECT the SatoCash applet; the applet answers with its status
    /// payload (Satocash.java selects reply with `satocashGetStatus`).
    pub fn select(&mut self) -> Result<SatocashStatus, Error> {
        let (data, sw) = self
            .transport
            .exchange(0x00, 0xA4, 0x04, 0x00, &SATOCASH_AID)?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        SatocashStatus::parse(&data)
    }

    /// `SATOCASH_GET_STATUS` (INS 0xB0, plaintext): full status with
    /// capacities — same payload the applet returns on SELECT.
    // SATO: ins: 0xB0
    // p1: 0x00
    // p2: 0x00
    // data: none
    // return: [versions(4b) | PIN0-PUK0-PIN1-PUK1 tries (4b) |
    // needs2FA (1b) | RFU(1b) | setupDone(1b) | needs_secure_channel(1b) | nfc_policy(1b) |
    // pin_policy(1b) | RFU(1b) |
    // MAX_NB_MINT (1b) | NB_USED_MINT(1b) |
    // MAX_NB_KEYSET(1b) | NB_USED_KEYSET(1b)  |
    // MAX_NB_PROOFS(2b) | NB_PROOFS_UNSPENT(2b) | NB_PROOFS_SPENT(2b) |
    // RFU(6b) |
    // size(NB_UNITS*PIN_POLICY_AMOUNT_THRESHOLD_SIZE) | pin_policy_amount_threshold |
    // size(NB_UNITS*PIN_POLICY_AMOUNT_THRESHOLD_SIZE) | pin_policy_cumulative_amount
    // ]
    pub fn satocash_get_status(&mut self) -> Result<SatocashStatus, Error> {
        let (data, sw) = self.xfer(ins::SATOCASH_GET_STATUS, 0x00, 0x00, &[])?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        SatocashStatus::parse(&data)
    }

    /// General applet/session status (INS 0x3C, plaintext).
    // SATO: ins: 0x3C
    // p1: 0x00
    // p2: 0x00
    // data: none
    // return: [versions(4b) | PIN0-PUK0-PIN1-PUK1 tries (4b) | needs2FA (1b) | is_seeded(1b) | setupDone(1b) | needs_secure_channel(1b) | nfc_policy(1b)]
    pub fn get_status(&mut self) -> Result<GeneralStatus, Error> {
        let (data, sw) = self.xfer(ins::GET_STATUS, 0x00, 0x00, &[])?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        GeneralStatus::parse(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// scripted card: (cmd APDU bytes) -> (resp, sw)
    struct MockCard {
        script: Vec<(Vec<u8>, Vec<u8>, u16)>,
        seen: Vec<Vec<u8>>,
    }

    // alloc in tests is fine
    impl CardTransport for MockCard {
        fn exchange(
            &mut self,
            cla: u8,
            i: u8,
            p1: u8,
            p2: u8,
            data: &[u8],
        ) -> Result<(Vec<u8>, u16), Error> {
            let cmd = crate::encode_apdu(cla, i, p1, p2, data);
            self.seen.push(cmd.clone());
            for (want, resp, sw) in &self.script {
                if *want == cmd {
                    return Ok((resp.clone(), *sw));
                }
            }
            Err(Error::Sw(0x6D00))
        }
    }

    /// A realistic virgin-card status payload per the documented layout:
    /// proto 1.0, applet 0.1, 3 PIN / 3 PUK tries, no 2FA, not set up,
    /// secure channel required, nfc policy 0x01, pin policy 0x00,
    /// 16/0 mints, 32/0 keysets, 512 max proofs, 0 unspent, 0 spent.
    fn virgin_status() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0x01, 0x00, 0x00, 0x01]); // versions
        v.extend_from_slice(&[0x03, 0x03, 0x00, 0x00]); // tries
        v.push(0x00); // needs2FA
        v.push(0x00); // RFU
        v.push(0x00); // setupDone
        v.push(0x01); // needs_secure_channel
        v.push(0x01); // nfc_policy
        v.push(0x00); // pin_policy
        v.push(0x00); // RFU
        v.extend_from_slice(&[16, 0, 32, 0]); // mints/keysets
        v.extend_from_slice(&0x0200u16.to_be_bytes()); // max proofs 512
        v.extend_from_slice(&0u16.to_be_bytes()); // unspent
        v.extend_from_slice(&0u16.to_be_bytes()); // spent
        v.extend_from_slice(&[0; 6]); // RFU tail
        v
    }

    #[test]
    fn select_encodes_documented_aid_and_parses_status() {
        let status = virgin_status();
        let select_cmd = {
            let mut c = vec![0x00, 0xA4, 0x04, 0x00, SATOCASH_AID.len() as u8];
            c.extend_from_slice(&SATOCASH_AID);
            c
        };
        let card = MockCard {
            script: vec![(select_cmd, status.clone(), 0x9000)],
            seen: vec![],
        };
        let mut client = SatocashClient::new(card);
        let st = client.select().expect("select ok");
        assert_eq!(st.protocol_version, (1, 0));
        assert_eq!(st.applet_version, (0, 1));
        assert!(st.needs_secure_channel);
        assert!(!st.setup_done);
        assert_eq!(st.max_mints, 16);
        assert_eq!(st.max_keysets, 32);
        assert_eq!(st.max_proofs, 512);
        assert_eq!(st.token_count(), 0);
    }

    #[test]
    fn status_commands_use_case2_apdu_forms() {
        let status = virgin_status();
        // SATOCASH_GET_STATUS: 5-byte case-2 (no Lc/Le) — the 6-byte
        // form is 6700 on hardware and wedges the nucula session.
        let card = MockCard {
            script: vec![(vec![0xB0, 0xB0, 0x00, 0x00, 0x00], status.clone(), 0x9000)],
            seen: vec![],
        };
        let mut client = SatocashClient::new(card);
        assert!(client.satocash_get_status().is_ok());

        let mut general = virgin_status();
        general.truncate(13);
        let card = MockCard {
            script: vec![(vec![0xB0, 0x3C, 0x00, 0x00, 0x00], general, 0x9000)],
            seen: vec![],
        };
        let mut client = SatocashClient::new(card);
        let gs = client.get_status().expect("general status ok");
        assert_eq!(gs.protocol_version, (1, 0));
        assert!(gs.needs_secure_channel);
        assert!(!gs.setup_done);
    }

    #[test]
    fn sw_6a82_select_surfaces_as_error() {
        let select_cmd = {
            let mut c = vec![0x00, 0xA4, 0x04, 0x00, SATOCASH_AID.len() as u8];
            c.extend_from_slice(&SATOCASH_AID);
            c
        };
        let card = MockCard {
            script: vec![(select_cmd, vec![], 0x6A82)],
            seen: vec![],
        };
        let mut client = SatocashClient::new(card);
        assert_eq!(client.select(), Err(Error::Sw(0x6A82)));
    }

    #[test]
    fn short_status_is_malformed() {
        assert_eq!(
            SatocashStatus::parse(&[0; 10]),
            Err(Error::Malformed("status shorter than 22 bytes"))
        );
    }
}
