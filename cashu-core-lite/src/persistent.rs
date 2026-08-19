//! Persistent wallet: wraps [`crate::wallet::Wallet`] with a proof store
//! and NUT-13 deterministic derivation.
//!
//! # Design
//!
//! **Determinism is the recovery story.** Secrets and blinders derive from
//! a master seed via NUT-13 (`derive_secret`/`derive_blinder`, indexed by a
//! counter). Everything the wallet mints can therefore be reconstructed:
//! the same seed reproduces the same `B_` values, so a NUT-09 restore
//! re-fetches signatures for outputs the wallet no longer has — including
//! outputs that were minted remotely but lost before persistence (crash
//! window). The restore scan covers `counter + RESTORE_HEADROOM` indices
//! for exactly that case.
//!
//! **Counter atomicity.** The counter advances in memory before the mint
//! call but is persisted only with the proofs on success. A failed mint
//! leaves the store's counter untouched (the mint rejected the request, so
//! those indices were never minted — a harmless gap). A lost *response*
//! after a successful remote mint is recovered by the restore headroom.
//!
//! **Envelope framing.** Payload is `MAGIC || crc32 || CBOR{version,
//! counter, proofs}`. A corrupt or foreign blob decodes as `None` and the
//! wallet starts fresh rather than panicking — with a deterministic seed,
//! "fresh" is still fully recoverable via restore.
//!
//! **Spend semantics.** `spend` removes proofs from the store before they
//! leave the wallet; a caller whose transfer failed rolls back with
//! [`PersistentWallet::undo_spend`]. Proofs handed out are the caller's
//! responsibility (standard ecash wallet behavior).

#[cfg(not(feature = "std"))]
use alloc::collections::{BTreeMap, BTreeSet};
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};

use crate::crypto::{blind_message, unblind_signature};
use crate::error::CashuError;
use crate::keypair::SecretKey;
use crate::nuts::nut00;
use crate::nuts::nut01;
use crate::nuts::nut04;
use crate::nuts::nut05;
use crate::nuts::nut09;
use crate::nuts::nut13;
use crate::store::{MemoryStore, ProofStore, StoreError};
use crate::wallet::{PendingOutput, Wallet};
use minicbor::{Decode, Encode};
use sha2::{Digest, Sha256};

/// Indices beyond the persisted counter scanned by
/// [`PersistentWallet::restore`] — covers outputs minted remotely in the
/// crash window before persistence.
pub const RESTORE_HEADROOM: u32 = 64;

const ENVELOPE_MAGIC: [u8; 4] = *b"CCL1";
const ENVELOPE_VERSION: u32 = 1;

#[derive(Debug, Clone, Encode, Decode)]
struct Envelope {
    #[n(0)]
    version: u32,
    #[n(1)]
    counter: u32,
    #[n(2)]
    proofs: Vec<nut00::Proof>,
    /// Binds the blob to the deriving seed — a wallet opened with a
    /// different seed sees "nothing stored for me" instead of silently
    /// loading another seed's proofs.
    #[n(3)]
    seed_id: [u8; 8],
}

/// Non-secret seed fingerprint: the seed itself never enters the blob.
fn seed_fingerprint(seed: &[u8; 32]) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(b"CCL1-seed-fingerprint");
    let digest = hasher.finalize();
    let mut fp = [0u8; 8];
    fp.copy_from_slice(&digest[..8]);
    fp
}

/// CRC-32/ISO-HDLC (bitwise, table-free) over the CBOR payload — detects
/// torn writes and medium corruption that strict CBOR decoding alone might
/// accept as a different valid document.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn encode_envelope(
    counter: u32,
    proofs: &[nut00::Proof],
    seed_id: [u8; 8],
) -> Result<Vec<u8>, CashuError> {
    let cbor = minicbor::to_vec(Envelope {
        version: ENVELOPE_VERSION,
        counter,
        proofs: proofs.to_vec(),
        seed_id,
    })
    .map_err(|_| CashuError::Storage(String::from("envelope encode failed")))?;

    let mut blob = Vec::with_capacity(ENVELOPE_MAGIC.len() + 4 + cbor.len());
    blob.extend_from_slice(&ENVELOPE_MAGIC);
    blob.extend_from_slice(&crc32(&cbor).to_le_bytes());
    blob.extend_from_slice(&cbor);
    Ok(blob)
}

/// `None` means "nothing valid stored" — corrupted, foreign, or seed-
/// mismatched blobs are treated as a fresh wallet, never a panic.
fn decode_envelope(bytes: &[u8], expected_seed_id: [u8; 8]) -> Option<(u32, Vec<nut00::Proof>)> {
    if bytes.len() < ENVELOPE_MAGIC.len() + 4 || bytes[..4] != ENVELOPE_MAGIC {
        return None;
    }
    let stored_crc = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let cbor = &bytes[8..];
    if crc32(cbor) != stored_crc {
        return None;
    }
    let envelope: Envelope = minicbor::decode(cbor).ok()?;
    if envelope.version != ENVELOPE_VERSION || envelope.seed_id != expected_seed_id {
        return None;
    }
    Some((envelope.counter, envelope.proofs))
}

fn store_error(e: StoreError) -> CashuError {
    match e {
        StoreError::Unavailable => CashuError::Storage(String::from("store unavailable")),
        StoreError::Failed(_) => CashuError::Storage(String::from("store write failed")),
    }
}

/// A [`Wallet`] with persistent proofs and deterministic (NUT-13) output
/// derivation, generic over mint transport `T` and storage `S`.
pub struct PersistentWallet<T, S>
where
    T: crate::transport::MintClient,
    S: ProofStore,
{
    inner: Wallet<T>,
    store: S,
    seed: [u8; 32],
    seed_id: [u8; 8],
    counter: u32,
    proofs: Vec<nut00::Proof>,
}

impl<T, S> PersistentWallet<T, S>
where
    T: crate::transport::MintClient,
    S: ProofStore,
{
    /// Load (or start) a wallet. A missing or corrupt stored blob starts
    /// fresh; with a deterministic seed, restore() re-fetches anything the
    /// mint still holds.
    pub fn new(
        mint_url: &str,
        transport: T,
        mut store: S,
        seed: [u8; 32],
    ) -> Result<Self, CashuError> {
        let seed_id = seed_fingerprint(&seed);
        let (counter, proofs) = match store.load().map_err(store_error)? {
            None => (0, Vec::new()),
            Some(bytes) => decode_envelope(&bytes, seed_id).unwrap_or((0, Vec::new())),
        };
        Ok(Self {
            inner: Wallet::new(mint_url, transport),
            store,
            seed,
            seed_id,
            counter,
            proofs,
        })
    }

    /// Total unspent balance across stored proofs.
    pub fn balance(&self) -> u64 {
        self.proofs.iter().map(|p| p.amount).sum()
    }

    /// Number of stored proofs.
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// NUT-04 mint with NUT-13 deterministic outputs: secrets and blinders
    /// derive from the seed and `counter..counter+n`, so outputs survive
    /// crashes via [`Self::restore`].
    ///
    /// The counter advances in memory before the transport call but is
    /// persisted only together with the proofs on success — see the module
    /// docs for the crash-window analysis.
    pub fn mint_deterministic(
        &mut self,
        quote_id: &str,
        amount: u64,
        keyset_id: &str,
        mint_keys: &nut01::KeySet,
    ) -> Result<u64, CashuError> {
        let denominations = nut00::decompose_amount(amount);
        if denominations.is_empty() {
            return Err(CashuError::InvalidAmount);
        }

        let (outputs, pending) = self.deterministic_outputs(&denominations, keyset_id)?;
        self.counter = self.counter.saturating_add(pending.len() as u32);

        let response = self.inner.transport.post_mint(nut04::MintRequest {
            quote: String::from(quote_id),
            outputs,
        })?;

        let proofs = self
            .inner
            .unblind_to_proofs(&pending, &response.signatures, mint_keys)?;
        let minted: u64 = proofs.iter().map(|p| p.amount).sum();
        self.proofs.extend(proofs);
        self.persist()?;
        Ok(minted)
    }

    /// Select and remove proofs covering `amount` (largest-first, may
    /// overshoot — exact change requires a NUT-03 swap by the caller).
    /// Persisted before the proofs leave the wallet; roll back a failed
    /// transfer with [`Self::undo_spend`].
    pub fn spend(&mut self, amount: u64) -> Result<Vec<nut00::Proof>, CashuError> {
        if amount == 0 {
            return Err(CashuError::InvalidAmount);
        }
        let total: u64 = self.proofs.iter().map(|p| p.amount).sum();
        if total < amount {
            return Err(CashuError::InsufficientInputs);
        }

        let mut sorted = self.proofs.clone();
        sorted.sort_by_key(|p| core::cmp::Reverse(p.amount));

        let mut selected: Vec<nut00::Proof> = Vec::new();
        let mut acc: u64 = 0;
        for proof in sorted {
            if acc >= amount {
                break;
            }
            acc += proof.amount;
            selected.push(proof);
        }

        self.proofs.retain(|p| !selected.contains(p));
        self.persist()?;
        Ok(selected)
    }

    /// NUT-05 melt with NUT-08 fee-return, wallet-managed inputs.
    ///
    /// Selects proofs covering `amount_plus_fee_reserve` itself. Proofs
    /// leave the wallet **only when the mint reports paid** — a pending
    /// or failed melt returns them to the store untouched (the
    /// funds-safety ordering reviewed in tmbg #299). Blank outputs for
    /// the fee reserve derive from the NUT-13 counter (restore-covered),
    /// and returned change signatures are unblinded into proofs using
    /// the *mint-chosen* denomination keys.
    pub fn melt_deterministic(
        &mut self,
        quote_id: &str,
        invoice_amount: u64,
        fee_reserve: u64,
        keyset_id: &str,
        mint_keys: &nut01::KeySet,
    ) -> Result<MeltOutcome, CashuError> {
        let total_needed = invoice_amount
            .checked_add(fee_reserve)
            .ok_or(CashuError::InvalidAmount)?;
        if total_needed == 0 {
            return Err(CashuError::InvalidAmount);
        }

        // Select now; persist the removal only after `paid`.
        let selected = self.select_covering(total_needed)?;

        // NUT-08 blanks: power-of-two outputs covering the fee reserve —
        // the only part of the inputs the mint can hand back.
        let blank_amounts = nut00::decompose_amount(fee_reserve);
        let (blanks, pending_blanks) = self.deterministic_outputs(&blank_amounts, keyset_id)?;
        self.counter = self.counter.saturating_add(pending_blanks.len() as u32);

        let response = self.inner.transport.post_melt(nut05::MeltRequest {
            quote: String::from(quote_id),
            inputs: selected.clone(),
            outputs: Some(blanks),
        })?;

        if !response.paid {
            // Funds-safe: inputs return to the wallet; counter stays
            // advanced (blanks were submitted; NUT-09 restore with
            // headroom covers any signatures the mint later returns).
            self.persist()?;
            return Ok(MeltOutcome {
                paid: false,
                preimage: response.payment_preimage,
                change_sats: 0,
            });
        }

        self.proofs.retain(|p| !selected.contains(p));

        // Unblind NUT-08 change: signatures zip to blanks by index
        // (mint-chosen amounts; mint key looked up per signature amount).
        let mut change_sats = 0u64;
        if let Some(change) = response.change.as_ref() {
            for (pending, sig) in pending_blanks.iter().zip(change.iter()) {
                let mint_pubkey = mint_keys
                    .keys
                    .iter()
                    .find(|kp| kp.amount == sig.amount)
                    .map(|kp| &kp.pubkey)
                    .ok_or(CashuError::KeysetNotFound)?;
                let c = unblind_signature(&sig.c, &pending.blinder, mint_pubkey)
                    .map_err(|_| CashuError::Crypto(String::from("change unblind failed")))?;
                change_sats += sig.amount;
                self.proofs.push(nut00::Proof {
                    amount: sig.amount,
                    id: sig.id.clone(),
                    secret: String::from_utf8(pending.secret.clone())
                        .map_err(|_| CashuError::Crypto(String::from("invalid secret string")))?,
                    c,
                    dleq: sig.dleq.as_ref().map(|d| {
                        crate::nuts::nut12::ProofDleq::new(
                            d.e.clone(),
                            d.s.clone(),
                            pending.blinder.clone(),
                        )
                    }),
                });
            }
        }

        self.persist()?;
        Ok(MeltOutcome {
            paid: true,
            preimage: response.payment_preimage,
            change_sats,
        })
    }

    /// Largest-first selection without removing from the wallet.
    fn select_covering(&self, amount: u64) -> Result<Vec<nut00::Proof>, CashuError> {
        let total: u64 = self.proofs.iter().map(|p| p.amount).sum();
        if total < amount {
            return Err(CashuError::InsufficientInputs);
        }
        let mut sorted = self.proofs.clone();
        sorted.sort_by_key(|p| core::cmp::Reverse(p.amount));
        let mut acc = 0u64;
        Ok(sorted
            .into_iter()
            .take_while(|p| {
                if acc >= amount {
                    false
                } else {
                    acc += p.amount;
                    true
                }
            })
            .collect())
    }

    /// Roll back a [`Self::spend`] whose transfer never completed.
    pub fn undo_spend(&mut self, proofs: Vec<nut00::Proof>) -> Result<(), CashuError> {
        self.proofs.extend(proofs);
        self.persist()
    }

    /// NUT-09 restore: re-derive every output in `0..counter+HEADROOM`,
    /// recompute the blinded messages `B_`, and ask the mint for their
    /// signatures. Recovers proofs lost to a corrupt store or a crash in
    /// the mint/persist window. Advances the counter past any restored
    /// index beyond it.
    pub fn restore(
        &mut self,
        keyset_id: &str,
        mint_keys: &nut01::KeySet,
    ) -> Result<u64, CashuError> {
        let end = self.counter.saturating_add(RESTORE_HEADROOM);

        // B_ (hex) → (index, blinder, secret) for every derivable output.
        let mut by_blinded: BTreeMap<String, (u32, SecretKey, String)> = BTreeMap::new();
        let mut outputs = Vec::new();
        for idx in 0..end {
            let secret = nut13::derive_secret(&self.seed, keyset_id, idx)?;
            let blinder_bytes = nut13::derive_blinder(&self.seed, keyset_id, idx)?;
            let blinder = SecretKey::from_slice(&blinder_bytes)
                .map_err(|_| CashuError::Crypto(String::from("bad blinder scalar")))?;
            let secret_hex = hex::encode(secret);
            let blinded = blind_message(secret_hex.as_bytes(), Some(blinder.clone()))
                .map_err(|_| CashuError::Crypto(String::from("blind_message failed")))?
                .blinded;
            by_blinded.insert(hex::encode(blinded.to_bytes()), (idx, blinder, secret_hex));
            outputs.push(blinded);
        }

        let response = self
            .inner
            .transport
            .post_restore(nut09::RestoreRequest { outputs })?;

        let mut known: BTreeSet<String> = self.proofs.iter().map(|p| p.secret.clone()).collect();
        let mut added: u64 = 0;
        let mut max_idx: Option<u32> = None;

        for out in response.outputs {
            let key = hex::encode(out.y.to_bytes());
            let Some(&(idx, ref blinder, ref secret_hex)) = by_blinded.get(&key) else {
                continue;
            };
            if known.contains(secret_hex) {
                continue;
            }

            let mint_pubkey = mint_keys
                .keys
                .iter()
                .find(|kp| kp.amount == out.signature.amount)
                .map(|kp| &kp.pubkey)
                .ok_or(CashuError::KeysetNotFound)?;

            let c = unblind_signature(&out.signature.c, blinder, mint_pubkey)
                .map_err(|_| CashuError::Crypto(String::from("unblind failed")))?;

            self.proofs.push(nut00::Proof {
                amount: out.signature.amount,
                id: out.signature.id.clone(),
                secret: secret_hex.clone(),
                c,
                dleq: out.signature.dleq.as_ref().map(|d| {
                    crate::nuts::nut12::ProofDleq::new(d.e.clone(), d.s.clone(), blinder.clone())
                }),
            });
            known.insert(secret_hex.clone());
            added += 1;
            max_idx = Some(max_idx.map_or(idx, |m: u32| m.max(idx)));
        }

        if let Some(m) = max_idx {
            self.counter = self.counter.max(m + 1);
        }
        if added > 0 {
            self.persist()?;
        }
        Ok(added)
    }

    fn deterministic_outputs(
        &self,
        amounts: &[u64],
        keyset_id: &str,
    ) -> Result<(Vec<nut00::BlindedMessage>, Vec<PendingOutput>), CashuError> {
        let mut messages = Vec::with_capacity(amounts.len());
        let mut pending = Vec::with_capacity(amounts.len());

        for (i, &amount) in amounts.iter().enumerate() {
            let idx = self.counter + i as u32;
            let secret = nut13::derive_secret(&self.seed, keyset_id, idx)?;
            let blinder_bytes = nut13::derive_blinder(&self.seed, keyset_id, idx)?;
            let blinder = SecretKey::from_slice(&blinder_bytes)
                .map_err(|_| CashuError::Crypto(String::from("bad blinder scalar")))?;

            let secret_hex = hex::encode(secret);
            let bm = blind_message(secret_hex.as_bytes(), Some(blinder.clone()))
                .map_err(|_| CashuError::Crypto(String::from("blind_message failed")))?;

            messages.push(nut00::BlindedMessage {
                amount,
                id: String::from(keyset_id),
                b: bm.blinded,
            });
            pending.push(PendingOutput {
                secret: secret_hex.into_bytes(),
                blinder: bm.blinder,
                amount,
            });
        }
        Ok((messages, pending))
    }

    fn persist(&mut self) -> Result<(), CashuError> {
        let blob = encode_envelope(self.counter, &self.proofs, self.seed_id)?;
        self.store.save(&blob).map_err(store_error)
    }
}

/// Outcome of a NUT-05 melt with NUT-08 change handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeltOutcome {
    /// Whether the invoice was paid.
    pub paid: bool,
    /// Payment preimage when the mint disclosed it.
    pub preimage: Option<String>,
    /// Value of NUT-08 change reclaimed (overpaid fee reserve).
    pub change_sats: u64,
}

/// Re-exported for doc links; `MemoryStore` is the reference `ProofStore`.
pub type VolatileWallet<T> = PersistentWallet<T, MemoryStore>;
