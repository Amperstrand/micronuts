//! Offline gate validation: the edge firmware profile.
//!
//! Decision pipeline (see design doc §4): decode → trust check →
//! per-proof NUT-12 DLEQ verification against pinned keysets → value
//! check → persist spent secrets **before** returning `Open`.

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::{decode_token_or_err, find_keyset, secret_has_spending_conditions, WalletPortError};
use cashu_core_lite::nuts::nut01;
use cashu_core_lite::nuts::nut12::verify_proof_dleq;
use cashu_core_lite::store::ProofStore;
use minicbor::{Decode, Encode};

/// Upper bound on tracked spent secrets — flash-friendly ring: oldest
/// entries are evicted, bounding the replay window (bounded-risk
/// trade-off documented in the design; sync-on-online reconciliation
/// closes it).
pub const SPENT_RING_CAPACITY: usize = 4096;

/// Outcome of an offline validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// All proofs verified against the pinned keyset and value met the
    /// price; secrets persisted as spent — open the gate.
    Open { total_sats: u64 },
    /// Verified correctly but below price — caller may surface a
    /// top-up prompt. Secrets are NOT marked spent.
    Underpaid { total_sats: u64 },
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpentRing {
    #[n(0)]
    version: u32,
    /// Secrets (hex) most-recent-last; capped by the caller before save.
    #[n(1)]
    secrets: Vec<String>,
}

/// Offline validator over a pinned trust anchor.
pub struct OfflineGateValidator<S: ProofStore> {
    trust: crate::TrustModel,
    pinned_keysets: Vec<nut01::KeySet>,
    store: S,
    spent: Vec<String>,
}

impl<S: ProofStore> OfflineGateValidator<S> {
    /// Load (or start) the validator. A corrupt store starts with an
    /// empty ring — the documented trade-off of the bounded replay
    /// window, never a panic.
    pub fn new(
        accepted_mints: Vec<String>,
        pinned_keysets: Vec<nut01::KeySet>,
        mut store: S,
    ) -> Result<Self, WalletPortError> {
        let spent = match store.load() {
            Ok(Some(blob)) => minicbor::decode::<SpentRing>(&blob)
                .ok()
                .filter(|r| r.version == 1)
                .map(|r| r.secrets)
                .unwrap_or_default(),
            Ok(None) => Vec::new(),
            Err(_) => return Err(WalletPortError::Storage(String::from("store unavailable"))),
        };
        Ok(Self {
            trust: crate::TrustModel::new(accepted_mints, false),
            pinned_keysets,
            store,
            spent,
        })
    }

    /// Validate a `cashuB…` token against the pinned keyset. Secrets are
    /// persisted as spent iff the decision is `Open`.
    pub fn verify_token(
        &mut self,
        token_wire: &str,
        price_sats: u64,
    ) -> Result<GateDecision, WalletPortError> {
        let token = decode_token_or_err(token_wire)?;

        if !self.trust.mint_accepted(&token.mint) {
            return Err(WalletPortError::UntrustedMint(token.mint));
        }

        for proof in token.tokens.iter().flat_map(|t| t.proofs.iter()) {
            if secret_has_spending_conditions(&proof.secret) {
                return Err(WalletPortError::LockedSecret);
            }
            if self.spent.contains(&proof.secret) {
                return Err(WalletPortError::Replay);
            }
            let Some(dleq) = proof.dleq.as_ref() else {
                return Err(WalletPortError::InvalidProof(String::from(
                    "proof carries no NUT-12 dleq — offline verification impossible",
                )));
            };
            let Some(keyset) = find_keyset(&self.pinned_keysets, &proof.keyset_id) else {
                return Err(WalletPortError::InvalidProof(String::from(
                    "proof keyset not pinned",
                )));
            };
            let Some(amount_key) = keyset.keys.iter().find(|k| k.amount == proof.amount) else {
                return Err(WalletPortError::InvalidProof(String::from(
                    "no pinned key for denomination",
                )));
            };
            // Secrets are valid hex (sanity), but hashed as their ASCII
            // bytes verbatim — the cross-vector lesson: never re-encode.
            hex::decode(&proof.secret)
                .map_err(|_| WalletPortError::InvalidProof(String::from("secret not hex")))?;
            let c_point = cashu_core_lite::keypair::PublicKey::from_sec1_bytes(&proof.c)
                .map_err(|_| WalletPortError::InvalidProof(String::from("C not a point")))?;
            if !verify_proof_dleq(proof.secret.as_bytes(), &c_point, dleq, &amount_key.pubkey) {
                return Err(WalletPortError::InvalidProof(String::from(
                    "DLEQ verification failed",
                )));
            }
        }

        let total = token.total_amount();
        if total < price_sats {
            return Ok(GateDecision::Underpaid { total_sats: total });
        }

        // Persist-before-open: if this fails, the gate must NOT open.
        for proof in token.tokens.iter().flat_map(|t| t.proofs.iter()) {
            self.spent.push(proof.secret.clone());
        }
        let overflow = self.spent.len().saturating_sub(SPENT_RING_CAPACITY);
        if overflow > 0 {
            self.spent.drain(..overflow);
        }
        self.persist()?;

        Ok(GateDecision::Open { total_sats: total })
    }

    fn persist(&mut self) -> Result<(), WalletPortError> {
        let ring = SpentRing {
            version: 1,
            secrets: self.spent.clone(),
        };
        let blob = minicbor::to_vec(ring)
            .map_err(|_| WalletPortError::Storage(String::from("ring encode failed")))?;
        self.store
            .save(&blob)
            .map_err(|_| WalletPortError::Storage(String::from("ring save failed")))
    }
}
