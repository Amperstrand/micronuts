//! Wallet engine: the money operations behind the UI.
//!
//! Wraps a [`PersistentWallet`] (proof custody, NUT-13 determinism, NUT-09
//! restore) plus a clone of the transport for metadata calls the
//! `PersistentWallet` does not expose (info/keys/keysets/quotes). Send and
//! melt pre-swap to exact denominations so a largest-first selection can
//! never burn overshoot value (NUT-05 returns change only for the fee
//! reserve).

use cashu_core_lite::crypto::hash_to_curve;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut01, nut04, nut05, nut07};
use cashu_core_lite::persistent::{MeltOutcome, PersistentWallet};
use cashu_core_lite::store::ProofStore;
use cashu_core_lite::token::{
    decode_token, encode_token_wire, Proof as TokenProof, TokenV4, TokenV4Token,
};
use cashu_core_lite::transport::MintClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryKind {
    Mint,
    Send,
    Receive,
    Melt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub kind: HistoryKind,
    pub amount: u64,
    pub detail: String,
    pub ts_secs: u64,
}

pub struct WalletEngine<T: MintClient + Clone, S: ProofStore> {
    mint_url: String,
    seed: [u8; 32],
    meta: T,
    wallet: PersistentWallet<T, S>,
    keyset_id: String,
    keys: nut01::KeySet,
    fee_ppk: u64,
    unit: String,
    mint_name: String,
    history: Vec<HistoryEntry>,
}

impl<T: MintClient + Clone, S: ProofStore> WalletEngine<T, S> {
    pub fn new(
        mint_url: &str,
        transport: T,
        store: S,
        seed: [u8; 32],
        history: Vec<HistoryEntry>,
    ) -> Result<Self, CashuError> {
        let mint_url = mint_url.trim_end_matches('/').to_string();
        let wallet = PersistentWallet::new(&mint_url, transport.clone(), store, seed)?;
        Ok(Self {
            mint_url,
            seed,
            meta: transport,
            wallet,
            keyset_id: String::new(),
            keys: nut01::KeySet {
                id: String::new(),
                unit: String::from("sat"),
                keys: Vec::new(),
            },
            fee_ppk: 0,
            unit: String::from("sat"),
            mint_name: String::new(),
            history,
        })
    }

    /// Fetch mint info, keysets, and keys; cache the active keyset.
    pub fn connect(&mut self) -> Result<(), CashuError> {
        let info = self.meta.get_info()?;
        self.mint_name = info.name;

        let keysets = self.meta.get_keysets()?;
        let active = keysets
            .keysets
            .iter()
            .find(|k| k.active)
            .ok_or_else(|| CashuError::Protocol(String::from("mint has no active keyset")))?;
        self.fee_ppk = active.input_fee_ppk;
        self.unit = active.unit.clone();

        let keys = self.meta.get_keys()?;
        let keyset = keys
            .keysets
            .iter()
            .find(|k| k.id == active.id)
            .ok_or(CashuError::KeysetNotFound)?;
        self.keyset_id = keyset.id.clone();
        self.keys = keyset.clone();
        Ok(())
    }

    fn ensure_connected(&self) -> Result<(), CashuError> {
        if self.keyset_id.is_empty() {
            return Err(CashuError::Protocol(String::from(
                "not connected — call connect() first",
            )));
        }
        Ok(())
    }

    pub fn mint_name(&self) -> &str {
        &self.mint_name
    }

    pub fn mint_url(&self) -> &str {
        &self.mint_url
    }

    pub fn keyset_id(&self) -> &str {
        &self.keyset_id
    }

    pub fn fee_ppk(&self) -> u64 {
        self.fee_ppk
    }

    pub fn balance(&self) -> u64 {
        self.wallet.balance()
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    pub fn seed_hex(&self) -> String {
        hex::encode(self.seed)
    }

    /// NUT-04 step 1: create a mint quote (Lightning invoice) for `amount`.
    pub fn mint_via_invoice(
        &mut self,
        amount: u64,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.ensure_connected()?;
        self.meta.post_mint_quote(nut04::MintQuoteRequest {
            amount,
            unit: self.unit.clone(),
            pubkey: None,
        })
    }

    /// NUT-04 step 2: poll a mint quote's state.
    pub fn poll_mint_quote(
        &mut self,
        quote_id: &str,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.meta.get_mint_quote(quote_id)
    }

    /// NUT-04 step 3: mint ecash against a paid quote.
    pub fn mint_paid_quote(&mut self, quote_id: &str, amount: u64) -> Result<u64, CashuError> {
        self.ensure_connected()?;
        let minted =
            self.wallet
                .mint_deterministic(quote_id, amount, &self.keyset_id, &self.keys)?;
        self.record(HistoryKind::Mint, minted, format!("quote {quote_id}"));
        Ok(minted)
    }

    /// Redeem an incoming ecash token: swap its proofs for fresh
    /// deterministic outputs (secret-rotation hygiene — the sender never
    /// learns the new secrets). Returns the received amount.
    pub fn receive_token(&mut self, token_str: &str) -> Result<u64, CashuError> {
        self.ensure_connected()?;
        let token = decode_token(token_str.as_bytes())
            .map_err(|e| CashuError::Protocol(format!("invalid token: {e}")))?;
        if token.mint.trim_end_matches('/') != self.mint_url {
            return Err(CashuError::Protocol(format!(
                "token is from a different mint: {} (active: {})",
                token.mint, self.mint_url
            )));
        }

        let mut total = 0u64;
        for group in &token.tokens {
            let mut inputs = Vec::with_capacity(group.proofs.len());
            for proof in &group.proofs {
                inputs.push(token_proof_to_wallet(proof, &group.keyset_id)?);
            }
            let group_total: u64 = inputs.iter().map(|p| p.amount).sum();
            self.wallet.swap_deterministic(
                inputs,
                &nut00::decompose_amount(group_total),
                &self.keyset_id,
                &self.keys,
            )?;
            total += group_total;
        }
        self.record(HistoryKind::Receive, total, String::from("ecash token"));
        Ok(total)
    }

    /// NUT-07 health check for an incoming token (before redemption):
    /// reports SPENT/PENDING/UNSPENT per proof, by Y value.
    pub fn check_token_state(
        &mut self,
        token_str: &str,
    ) -> Result<Vec<nut07::ProofState>, CashuError> {
        let token = decode_token(token_str.as_bytes())
            .map_err(|e| CashuError::Protocol(format!("invalid token: {e}")))?;
        let mut ys = Vec::new();
        for proof in token.tokens.iter().flat_map(|g| &g.proofs) {
            let y = hash_to_curve(proof.secret.as_bytes())
                .map_err(|_| CashuError::Crypto(String::from("hash_to_curve failed")))?;
            ys.push(y);
        }
        let response = self
            .meta
            .post_check_state(nut07::CheckStateRequest { ys })?;
        Ok(response.states)
    }

    /// Compose an exact-amount ecash token (NUT-00 V4 wire string).
    /// Over-selection is swapped into exact send + keep denominations so
    /// no value is lost to rounding.
    pub fn send_token(&mut self, amount: u64, memo: Option<&str>) -> Result<String, CashuError> {
        self.ensure_connected()?;
        let send_proofs = self.compose_exact(amount)?;
        let tokens = vec![TokenV4Token {
            keyset_id: self.keyset_id.clone(),
            proofs: send_proofs
                .iter()
                .map(|p| TokenProof {
                    amount: p.amount,
                    keyset_id: p.id.clone(),
                    secret: p.secret.clone(),
                    c: p.c.to_bytes().to_vec(),
                    dleq: p.dleq.clone(),
                })
                .collect(),
        }];
        let token = TokenV4 {
            mint: self.mint_url.clone(),
            unit: self.unit.clone(),
            memo: memo.map(str::to_string),
            tokens,
        };
        let wire = encode_token_wire(&token)
            .map_err(|e| CashuError::Protocol(format!("token encode failed: {e}")))?;
        self.wallet.remove_proofs(&send_proofs)?;
        self.record(HistoryKind::Send, amount, String::from("ecash token"));
        Ok(wire)
    }

    /// NUT-05: pay a Lightning invoice. The whole balance is first swapped
    /// into an exact `amount + fee_reserve` set plus change capped at the
    /// target's lowest denomination, because the wallet's melt selects
    /// inputs largest-first and NUT-05 change only refunds the fee
    /// reserve — an over-large coin would otherwise burn its overshoot.
    pub fn melt(
        &mut self,
        invoice: &str,
    ) -> Result<(nut05::MeltQuoteResponse, MeltOutcome), CashuError> {
        self.ensure_connected()?;
        let quote = self.meta.post_melt_quote(nut05::MeltQuoteRequest {
            request: String::from(invoice),
            unit: self.unit.clone(),
        })?;
        let target = quote
            .amount
            .checked_add(quote.fee_reserve)
            .ok_or(CashuError::InvalidAmount)?;

        let total = self.wallet.balance();
        let all = self.wallet.spend(total)?;
        let swap_fee = if self.fee_ppk == 0 {
            0
        } else {
            (self.fee_ppk * all.len() as u64).div_ceil(1000)
        };
        let rest = total - swap_fee - target;
        let cap = target & target.wrapping_neg();
        let mut amounts = nut00::decompose_amount(target);
        amounts.extend(decompose_capped(rest, cap));

        if let Err(err) =
            self.wallet
                .swap_deterministic(all.clone(), &amounts, &self.keyset_id, &self.keys)
        {
            let _ = self.wallet.undo_spend(all);
            return Err(err);
        }

        let outcome = self.wallet.melt_deterministic(
            &quote.quote,
            quote.amount,
            quote.fee_reserve,
            &self.keyset_id,
            &self.keys,
        )?;
        let detail = match &outcome.preimage {
            Some(preimage) => format!("preimage {preimage}"),
            None => String::from("no preimage"),
        };
        self.record(HistoryKind::Melt, quote.amount, detail);
        Ok((quote, outcome))
    }

    /// NUT-09 restore from the deterministic seed.
    pub fn restore(&mut self) -> Result<u64, CashuError> {
        self.ensure_connected()?;
        self.wallet.restore(&self.keyset_id, &self.keys)
    }

    /// Produce exactly `amount` of spendable proofs in the wallet (change
    /// stays stored alongside). `spend` may over-select; the overshoot is
    /// swapped back alongside the exact denominations. On swap failure the
    /// selection is rolled back. Callers that hand the exact proofs out
    /// (send) must remove them from the wallet themselves.
    fn compose_exact(&mut self, amount: u64) -> Result<Vec<nut00::Proof>, CashuError> {
        if amount == 0 {
            return Err(CashuError::InvalidAmount);
        }
        let fee = self.swap_fee_hint(amount)?;
        let selected = self
            .wallet
            .spend(amount.checked_add(fee).ok_or(CashuError::InvalidAmount)?)?;
        let selected_total: u64 = selected.iter().map(|p| p.amount).sum();
        let keep = selected_total - amount - fee;

        let send_amounts = nut00::decompose_amount(amount);
        let mut amounts = send_amounts.clone();
        amounts.extend(nut00::decompose_amount(keep));

        let fresh = match self.wallet.swap_deterministic(
            selected.clone(),
            &amounts,
            &self.keyset_id,
            &self.keys,
        ) {
            Ok(fresh) => fresh,
            Err(err) => {
                let _ = self.wallet.undo_spend(selected);
                return Err(err);
            }
        };

        Ok(fresh.into_iter().take(send_amounts.len()).collect())
    }

    /// NUT-08 fee estimate for the number of inputs a `spend(amount)` would
    /// select (two wallet round-trips; exact for zero-fee keysets).
    fn swap_fee_hint(&mut self, amount: u64) -> Result<u64, CashuError> {
        if self.fee_ppk == 0 {
            return Ok(0);
        }
        let selected = self.wallet.spend(amount)?;
        let count = selected.len() as u64;
        let _ = self.wallet.undo_spend(selected);
        Ok((self.fee_ppk * count).div_ceil(1000))
    }

    fn record(&mut self, kind: HistoryKind, amount: u64, detail: String) {
        let ts_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.history.push(HistoryEntry {
            kind,
            amount,
            detail,
            ts_secs,
        });
    }
}

/// Decompose `total` into power-of-two coins no larger than `cap`
/// (greedy). `cap` must be a power of two ≥ 1.
fn decompose_capped(total: u64, cap: u64) -> Vec<u64> {
    let mut remaining = total;
    let mut coins = Vec::new();
    while remaining > 0 {
        let mut coin = cap.min(remaining);
        while coin > remaining {
            coin >>= 1;
        }
        coins.push(coin);
        remaining -= coin;
    }
    coins
}

fn token_proof_to_wallet(proof: &TokenProof, keyset_id: &str) -> Result<nut00::Proof, CashuError> {
    let c_bytes: [u8; 33] = proof
        .c
        .as_slice()
        .try_into()
        .map_err(|_| CashuError::InvalidProof)?;
    Ok(nut00::Proof {
        amount: proof.amount,
        id: String::from(keyset_id),
        secret: proof.secret.clone(),
        c: cashu_core_lite::PublicKey::from_bytes(&c_bytes).ok_or(CashuError::InvalidProof)?,
        dleq: proof.dleq.clone(),
        witness: None,
    })
}
