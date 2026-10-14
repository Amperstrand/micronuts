//! SatoCash wallet-storage commands: mints, keysets, proofs.
//!
//! Wire reference: Satocash.java `satocashImportMint` (0xB1),
//! `satocashExportMint` (0xB2), `satocashImportKeyset` (0xB4),
//! `satocashImportProof` (0xB7), `satocashExportProofs` (0xB8) —
//! verbatim blocks pinned in SPEC-wire.md via greatspectations
//! (`// SATO:` quotes below).
//!
//! On-card these run INSIDE the ECDH/AES secure channel (INS 0x81/0x82):
//! a card with `needs_secure_channel` answers every one of them with
//! `SW 9C20` until the channel layer (follow-up milestone) wraps them.
//! The encodings here are the inner-APDU payloads, unchanged by the
//! channel wrapper (which encrypts the full inner APDU).

use crate::{ins, CardTransport, Error, SatocashClient};

/// P2 operation selector for `EXPORT_PROOFS` / `EXPORT_MINT` style
/// multi-step commands (Satocash.java OP_INIT/OP_PROCESS).
pub const OP_INIT: u8 = 0x01;
pub const OP_PROCESS: u8 = 0x02;

/// Cashu amounts are powers of two; the card stores the exponent
/// (MAX_PROOF_EXPONENT = 31).
pub fn proof_value(amount_exponent: u8) -> u64 {
    debug_assert!(amount_exponent < 32);
    1u64 << (amount_exponent & 31)
}

/// Proof lifecycle state on-card (Satocash.java STATE_*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofState {
    Empty,
    Unspent,
    Spent,
    Unknown(u8),
}

impl From<u8> for ProofState {
    fn from(v: u8) -> Self {
        match v {
            0 => ProofState::Empty,
            1 => ProofState::Unspent,
            2 => ProofState::Spent,
            other => ProofState::Unknown(other),
        }
    }
}

/// One stored ecash proof: the unblinded signature public key plus the
/// 32-byte secret — together these ARE the spendable token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofRecord {
    pub index: u16,
    pub state: ProofState,
    pub keyset_index: u8,
    pub amount_exponent: u8,
    pub unblinded_key: [u8; 33],
    pub secret: [u8; 32],
}

impl ProofRecord {
    pub fn amount(&self) -> u64 {
        proof_value(self.amount_exponent)
    }

    /// Parse the EXPORT_PROOFS response: a stream of 70-byte records
    /// `[index(2) | state(1) | keyset_index(1) | amount_exponent(1) |
    /// unblinded_key(33) | secret(32)]`.
    pub fn parse_stream(r: &[u8]) -> Result<Vec<ProofRecord>, Error> {
        const REC: usize = 70;
        if !r.len().is_multiple_of(REC) {
            return Err(Error::Malformed("proof stream not a 70-byte multiple"));
        }
        let mut out = Vec::with_capacity(r.len() / REC);
        for chunk in r.chunks(REC) {
            let mut unblinded_key = [0u8; 33];
            unblinded_key.copy_from_slice(&chunk[5..38]);
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&chunk[38..70]);
            out.push(ProofRecord {
                index: u16::from_be_bytes([chunk[0], chunk[1]]),
                state: ProofState::from(chunk[2]),
                keyset_index: chunk[3],
                amount_exponent: chunk[4],
                unblinded_key,
                secret,
            });
        }
        Ok(out)
    }
}

impl<T: CardTransport> SatocashClient<T> {
    /// `IMPORT_MINT` (0xB1): store a mint URL, returns its mint index.
    // SATO: ins: 0xB1
    // p1: RFU
    // p2: RFU
    // data: [mint_url_size(1b) | mint_url]
    // return: [MINT_INDEX(1b)]
    pub fn import_mint(&mut self, url: &[u8]) -> Result<u8, Error> {
        if url.len() > 48 {
            return Err(Error::Malformed("mint url longer than MAX_MINT_URL_SIZE"));
        }
        let mut data = Vec::with_capacity(1 + url.len());
        data.push(url.len() as u8);
        data.extend_from_slice(url);
        let (resp, sw) = self.xfer(ins::IMPORT_MINT, 0x00, 0x00, &data)?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        resp.first()
            .copied()
            .ok_or(Error::Malformed("empty mint index"))
    }

    /// `EXPORT_MINT` (0xB2): read mint URL by index.
    // SATO: ins: 0xB2
    // p1: index
    // p2: RFU
    // data: none
    // return: [URL_size (1b) | URL ]
    pub fn export_mint(&mut self, mint_index: u8) -> Result<Vec<u8>, Error> {
        let (resp, sw) = self.xfer(ins::EXPORT_MINT, mint_index, 0x00, &[])?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        let n = resp
            .first()
            .copied()
            .ok_or(Error::Malformed("empty mint export"))? as usize;
        resp.get(1..1 + n)
            .map(<[u8]>::to_vec)
            .ok_or(Error::Malformed("mint url truncated"))
    }

    /// `IMPORT_KEYSET` (0xB4): bind a Cashu keyset to a stored mint.
    // SATO: ins: 0xB4
    // p1: RFU
    // p2: RFU
    // data: [keyset_id(8b) | mint_index(1b) | unit(1b) ]
    // return: [KEYSET_INDEX(1b)]
    pub fn import_keyset(
        &mut self,
        keyset_id: [u8; 8],
        mint_index: u8,
        unit: u8,
    ) -> Result<u8, Error> {
        let mut data = Vec::with_capacity(10);
        data.extend_from_slice(&keyset_id);
        data.push(mint_index);
        data.push(unit);
        let (resp, sw) = self.xfer(ins::IMPORT_KEYSET, 0x00, 0x00, &data)?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        resp.first()
            .copied()
            .ok_or(Error::Malformed("empty keyset index"))
    }

    /// `IMPORT_PROOF` (0xB7): store one ecash token.
    // SATO: ins: 0xB7
    // p1: RFU
    // p2: RFU
    // data: [keyset_index(1b) | amount_exponent(1b) | unblinded_key(33b) | secret(32b)]
    // return: [index(2b)]
    pub fn import_proof(
        &mut self,
        keyset_index: u8,
        amount_exponent: u8,
        unblinded_key: &[u8; 33],
        secret: &[u8; 32],
    ) -> Result<u16, Error> {
        let mut data = Vec::with_capacity(67);
        data.push(keyset_index);
        data.push(amount_exponent);
        data.extend_from_slice(unblinded_key);
        data.extend_from_slice(secret);
        let (resp, sw) = self.xfer(ins::IMPORT_PROOF, 0x00, 0x00, &data)?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        if resp.len() < 2 {
            return Err(Error::Malformed("proof index truncated"));
        }
        Ok(u16::from_be_bytes([resp[0], resp[1]]))
    }

    /// `EXPORT_PROOFS` (0xB8, OP_INIT): read proofs by on-card index,
    /// optionally restricted to `indices`. Returns parsed proof records.
    // SATO: data (OP_INIT): [ proof_index_list_size(1b) | proof_index(2b) ... | 2FA_size(1b) | 2FA ] else
    // data (OP_PROCESS): []
    // return: [proof_index(2b) | proof_state(1b) | keyset_index(1b) | amount_exponent(1b) | unblinded_key(33b) | secret(32b)]
    pub fn export_proofs(&mut self, indices: &[u16]) -> Result<Vec<ProofRecord>, Error> {
        let mut data = Vec::with_capacity(2 + indices.len() * 2);
        data.push(indices.len() as u8);
        for i in indices {
            data.extend_from_slice(&i.to_be_bytes());
        }
        data.push(0); // 2FA size: none
        let (resp, sw) = self.xfer(ins::EXPORT_PROOFS, 0x00, OP_INIT, &data)?;
        if sw != 0x9000 {
            return Err(Error::Sw(sw));
        }
        ProofRecord::parse_stream(&resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SatocashClient;

    struct ScriptedCard {
        script: Vec<(Vec<u8>, Vec<u8>, u16)>,
    }

    impl CardTransport for ScriptedCard {
        fn exchange(
            &mut self,
            cla: u8,
            i: u8,
            p1: u8,
            p2: u8,
            data: &[u8],
        ) -> Result<(Vec<u8>, u16), Error> {
            let cmd = crate::encode_apdu(cla, i, p1, p2, data);
            for (want, resp, sw) in &self.script {
                if *want == cmd {
                    return Ok((resp.clone(), *sw));
                }
            }
            Err(Error::Sw(0x6D00))
        }
    }

    fn apdu(ins: u8, p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
        crate::encode_apdu(0xB0, ins, p1, p2, data)
    }

    /// Wallet ladder on a scripted applet: the encodings the card expects,
    /// byte-exact per SPEC-wire.md (case-3 — no trailing Le).
    #[test]
    fn wallet_ladder_encodings() {
        let mint_url = b"https://mint.example";
        let keyset_id: [u8; 8] = [0xAA; 8];
        let unblinded: [u8; 33] = [0x02; 33]; // SEC1 uncompressed prefix + X
        let secret: [u8; 32] = [0x5E; 32];

        let mut script = Vec::new();
        let mut mint_data = vec![mint_url.len() as u8];
        mint_data.extend_from_slice(mint_url);
        script.push((apdu(0xB1, 0, 0, &mint_data), vec![0x00], 0x9000));
        let mut ks_data = keyset_id.to_vec();
        ks_data.extend_from_slice(&[0x00, 0x00]);
        script.push((apdu(0xB4, 0, 0, &ks_data), vec![0x00], 0x9000));
        let mut pf_data = vec![0x00, 0x03];
        pf_data.extend_from_slice(&unblinded);
        pf_data.extend_from_slice(&secret);
        script.push((apdu(0xB7, 0, 0, &pf_data), vec![0x00, 0x07], 0x9000));
        let mut record = vec![];
        record.extend_from_slice(&7u16.to_be_bytes());
        record.push(0x01);
        record.push(0x00);
        record.push(0x03);
        record.extend_from_slice(&unblinded);
        record.extend_from_slice(&secret);
        let mut ex_data = vec![1u8];
        ex_data.extend_from_slice(&7u16.to_be_bytes());
        ex_data.push(0);
        script.push((apdu(0xB8, 0, OP_INIT, &ex_data), record, 0x9000));
        let mut mint_resp = vec![mint_url.len() as u8];
        mint_resp.extend_from_slice(mint_url);
        script.push((apdu(0xB2, 0x00, 0x00, &[]), mint_resp, 0x9000));

        let mut client = SatocashClient::new(ScriptedCard { script });
        assert_eq!(client.import_mint(mint_url), Ok(0));
        assert_eq!(client.import_keyset(keyset_id, 0, 0), Ok(0));
        assert_eq!(client.import_proof(0, 3, &unblinded, &secret), Ok(7));
        let proofs = client.export_proofs(&[7]).expect("export ok");
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].state, ProofState::Unspent);
        assert_eq!(proofs[0].amount(), 8); // 2^3
        assert_eq!(proofs[0].secret, secret);
        assert_eq!(client.export_mint(0).unwrap(), mint_url.to_vec());
    }

    /// A locked card refuses storage ops with SW 9C20 — the client must
    /// surface it verbatim (captured on the bench card 2026-10-14).
    #[test]
    fn storage_op_on_locked_card_is_9c20() {
        let cmd = apdu(0xB1, 0, 0, &[1, b'x']);
        let card = ScriptedCard {
            script: vec![(cmd, vec![], 0x9C20)],
        };
        let mut client = SatocashClient::new(card);
        assert_eq!(client.import_mint(b"x"), Err(Error::Sw(0x9C20)));
    }

    #[test]
    fn oversized_mint_url_rejected_client_side() {
        let cmd = apdu(0xB1, 0, 0, &[1, b'x']);
        let card = ScriptedCard {
            script: vec![(cmd, vec![0], 0x9000)],
        };
        let mut client = SatocashClient::new(card);
        assert_eq!(
            client.import_mint(&[b'u'; 49]),
            Err(Error::Malformed("mint url longer than MAX_MINT_URL_SIZE"))
        );
    }

    #[test]
    fn proof_stream_rejects_partial_record() {
        assert_eq!(
            ProofRecord::parse_stream(&[0u8; 69]),
            Err(Error::Malformed("proof stream not a 70-byte multiple"))
        );
    }
}
