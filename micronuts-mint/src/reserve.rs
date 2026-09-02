//! Upstream reserve wallet: proofs of the upstream mint are our Lightning
//! reserve (in-memory prototype; persistence milestone moves proofs to
//! durable storage with the persist-before-use rule).
//!
//! Secrets are random per output (NOT NUT-13 deterministic — deterministic
//! secrets across restarts would let a wiped store re-create already-spent
//! proofs). Era rule (cashu-cf hard rule): never change the upstream URL
//! while reserve proofs from the old era are outstanding — melts pay from
//! the CURRENT upstream regardless of which era minted the tokens.

use cashu_core_lite::crypto::unblind_signature;
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::nut00;
use serde_json::{json, Value};

use crate::upstream::{json_amount, json_str, upstream_get, upstream_post};
use crate::upstream_wire::{
    blind_outputs, json_point, proof_json, signatures_array, PendingChange, UpstreamKeyset,
};

/// Holds the upstream proofs our mint uses to settle melts.
pub struct ReserveWallet {
    base_url: String,
    unit: String,
    bootstrap_sats: u64,
    keyset: Option<UpstreamKeyset>,
    proofs: Vec<nut00::Proof>,
}

impl ReserveWallet {
    pub fn new(base_url: &str, unit: &str, bootstrap_sats: u64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            unit: unit.to_string(),
            bootstrap_sats,
            keyset: None,
            proofs: Vec::new(),
        }
    }

    /// Total reserve balance in sats.
    pub fn balance_sats(&self) -> u64 {
        self.proofs
            .iter()
            .try_fold(0u64, |acc, p| acc.checked_add(p.amount))
            .unwrap_or(u64::MAX)
    }

    /// Number of reserve proofs (diagnostics/tests).
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// Mint `amount_sat` of fresh upstream proofs into the reserve:
    /// upstream mint quote → poll until PAID → blind → `/v1/mint/bolt11`
    /// → unblind.
    pub fn bootstrap(&mut self, amount_sat: u64, http: &ureq::Agent) -> Result<(), CashuError> {
        if amount_sat == 0 {
            return Err(CashuError::InvalidAmount);
        }
        let keyset = self.ensure_keyset(http)?.clone();
        let quote = upstream_post(
            http,
            &format!("{}/v1/mint/quote/bolt11", self.base_url),
            &json!({"amount": amount_sat, "unit": self.unit}),
        )?;
        let quote_id = json_str("quote", &quote["quote"])?;
        // A real upstream needs an external payer — surface the invoice so
        // automation (or an operator) can pay it within the poll window.
        let invoice = json_str("request", &quote["request"])?;
        eprintln!(
            "micronuts reserve: bootstrap invoice for {amount_sat} sats (pay within the poll window): {invoice}"
        );
        if !self.poll_until_paid(&quote_id, http)? {
            return Err(CashuError::QuoteNotPaid);
        }
        let (outputs, pending) = blind_outputs(&nut00::decompose_amount(amount_sat), &keyset.id)?;
        let minted = upstream_post(
            http,
            &format!("{}/v1/mint/bolt11", self.base_url),
            &json!({"quote": quote_id, "outputs": outputs}),
        )?;
        let signatures = signatures_array(&minted)?;
        self.append_unblinded(pending, signatures, &keyset, amount_sat)?;
        eprintln!(
            "micronuts reserve: bootstrapped +{amount_sat} sats, balance {} sats",
            self.balance_sats()
        );
        Ok(())
    }

    /// Pay upstream melt quote `quote_id` from the reserve; returns the
    /// payment preimage hex.
    ///
    /// Auto-top-up: when the balance is below `amount_needed`, first
    /// bootstrap the deficit plus the bootstrap margin. Inputs are selected
    /// to cover `amount_needed` plus a 5% + 1 sat buffer for the upstream
    /// NUT-08 input fee; the overpay returns as blinded change outputs that
    /// are unblinded back into the reserve.
    ///
    /// Outcome mapping (see the module doc in [`crate::upstream`] for the
    /// payment-safety rationale): PAID → unblind change, Ok(preimage);
    /// FAILED → proofs retained, Err(PaymentFailed); PENDING/unknown →
    /// selected proofs parked (they may still be consumed upstream),
    /// Err(Protocol).
    pub fn pay(
        &mut self,
        quote_id: &str,
        amount_needed: u64,
        http: &ureq::Agent,
    ) -> Result<String, CashuError> {
        if amount_needed == 0 {
            return Err(CashuError::InvalidAmount);
        }
        let balance = self.balance_sats();
        if balance < amount_needed {
            let deficit = amount_needed - balance;
            let topup = deficit
                .checked_add(self.bootstrap_sats)
                .ok_or(CashuError::InvalidAmount)?;
            self.bootstrap(topup, http)?;
        }
        let target = amount_needed
            .checked_add(amount_needed / 20)
            .and_then(|v| v.checked_add(1))
            .ok_or(CashuError::InvalidAmount)?;
        let selected = self.select_covering(target);
        let selected_sum = selected
            .iter()
            .try_fold(0u64, |acc, &i| acc.checked_add(self.proofs[i].amount))
            .ok_or(CashuError::InvalidAmount)?;
        if selected_sum < amount_needed {
            return Err(CashuError::InsufficientInputs);
        }

        let keyset = self.ensure_keyset(http)?.clone();
        let input_fee = keyset
            .input_fee_ppk
            .checked_mul(selected.len() as u64)
            .map(|ppk| ppk.div_ceil(1000))
            .ok_or(CashuError::InvalidAmount)?;
        let change_amount = selected_sum
            .checked_sub(amount_needed)
            .and_then(|v| v.checked_sub(input_fee))
            .ok_or(CashuError::InvalidAmount)?;
        // Explicit-amount change (binary decomposition of the overpay).
        // Real cashu upstreams do NOT imprint blanks: signut (cashu-cf
        // saga, 2026-09-02) signed our amount-0 blanks as-is, returning
        // worthless 0-amount signatures — blanks are a micronuts-local
        // convention (our own FakeWallet front mint honors them).
        let amounts = if change_amount > 0 {
            nut00::decompose_amount(change_amount)
        } else {
            Vec::new()
        };
        let (outputs, pending) = blind_outputs(&amounts, &keyset.id)?;
        let inputs: Vec<Value> = selected
            .iter()
            .map(|&i| proof_json(&self.proofs[i]))
            .collect();
        let url = format!("{}/v1/melt/bolt11", self.base_url);
        let response = match upstream_post(
            http,
            &url,
            &json!({"quote": quote_id, "inputs": inputs, "outputs": outputs}),
        ) {
            Ok(response) => response,
            // Task-fixed prototype rule: an HTTP error on the melt POST is
            // treated as a definitive failure (the ambiguity poller that
            // would re-check the quote state is the follow-up).
            Err(err) => {
                eprintln!("micronuts reserve: upstream melt POST failed ({err})");
                return Err(CashuError::PaymentFailed);
            }
        };

        let state = response.get("state").and_then(Value::as_str).unwrap_or("");
        let paid = response
            .get("paid")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if paid || state == "PAID" {
            // The payment happened; everything below is bookkeeping.
            // Post-PAID problems are logged, never returned — an error here
            // would roll back the caller's proofs while the invoice is
            // already paid (double-loss class).
            self.remove_selected(&selected);
            let change = response.get("change").and_then(Value::as_array);
            if change.is_none_or(|c| c.is_empty()) {
                if change_amount > 0 {
                    // Saga-v2 cashu-cf upstreams return no melt change at
                    // all (verified with a real cashu-ts v4 wallet) — the
                    // overpay stays upstream. Minimal selection keeps this
                    // at dust size.
                    eprintln!(
                        "micronuts reserve: upstream returned no change \
                         ({change_amount} sats overpay kept upstream), \
                         balance {} sats",
                        self.balance_sats()
                    );
                }
            } else if let Some(change_sigs) = change.filter(|c| !c.is_empty()) {
                if let Err(err) =
                    self.append_unblinded(pending, change_sigs, &keyset, change_amount)
                {
                    let entries = change_sigs
                        .iter()
                        .map(|s| {
                            format!(
                                "amount={} id={}",
                                s.get("amount").map(|v| v.to_string()).unwrap_or_default(),
                                s.get("id").and_then(Value::as_str).unwrap_or("?")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "micronuts reserve: change recovery failed ({err}); \
                         {change_amount} sats at risk upstream, balance {} sats; \
                         change entries: [{entries}] (cached keyset {})",
                        self.balance_sats(),
                        keyset.id
                    );
                }
            }
            // cashu-cf saga serializes the preimage as `preimage`; the
            // NUT-05 wire name is `payment_preimage` — accept both.
            let preimage = response
                .get("payment_preimage")
                .or_else(|| response.get("preimage"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if preimage.is_empty() {
                eprintln!("micronuts reserve: upstream melt PAID without a preimage");
            }
            eprintln!(
                "micronuts reserve: melted {amount_needed} sats, balance {} sats",
                self.balance_sats()
            );
            Ok(preimage.to_string())
        } else if state == "FAILED" {
            eprintln!(
                "micronuts reserve: upstream melt FAILED, proofs retained, balance {} sats",
                self.balance_sats()
            );
            Err(CashuError::PaymentFailed)
        } else {
            self.remove_selected(&selected);
            eprintln!(
                "micronuts reserve: upstream melt state ambiguous ({state:?}), proofs parked, balance {} sats",
                self.balance_sats()
            );
            Err(CashuError::Protocol(format!(
                "upstream melt state ambiguous: {state}"
            )))
        }
    }

    fn poll_until_paid(&self, quote_id: &str, http: &ureq::Agent) -> Result<bool, CashuError> {
        let url = format!("{}/v1/mint/quote/bolt11/{quote_id}", self.base_url);
        // Fake upstreams settle within the 5s default; real ones need an
        // external payer, so the window is env-tunable.
        let timeout_secs: u64 = std::env::var("MICRONUTS_UPSTREAM_PAY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        let tries = timeout_secs.saturating_mul(2).max(1);
        for i in 0..tries {
            let quote = upstream_get(http, &url)?;
            let state = json_str("state", &quote["state"])?;
            if matches!(state.as_str(), "PAID" | "ISSUED") {
                return Ok(true);
            }
            if i > 0 && i % 20 == 0 {
                eprintln!(
                    "micronuts reserve: still waiting for payment of quote {quote_id} ({i}/{tries} polls)"
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Ok(false)
    }

    fn ensure_keyset(&mut self, http: &ureq::Agent) -> Result<&UpstreamKeyset, CashuError> {
        if self.keyset.is_none() {
            let keys = upstream_get(http, &format!("{}/v1/keys", self.base_url))?;
            self.keyset = Some(UpstreamKeyset::from_keys_response(&keys, &self.unit)?);
        }
        self.keyset.as_ref().ok_or(CashuError::Unknown(
            "keyset missing after fetch".to_string(),
        ))
    }

    /// Indices of proofs covering `target`, largest denomination first.
    fn select_covering(&self, target: u64) -> Vec<usize> {
        // Ascending greedy = minimal overshoot. Upstream melt change is
        // recovered via explicit outputs, but a no-change upstream (the
        // FakeWallet testnut mint) keeps the overpay — so overshoot stays
        // minimized by preferring small denominations.
        let mut order: Vec<usize> = (0..self.proofs.len()).collect();
        order.sort_by_key(|&i| self.proofs[i].amount);
        let mut sum = 0u64;
        let mut chosen = Vec::new();
        for idx in order {
            if sum >= target {
                break;
            }
            match sum.checked_add(self.proofs[idx].amount) {
                Some(next) => {
                    sum = next;
                    chosen.push(idx);
                }
                None => break,
            }
        }
        chosen
    }

    fn remove_selected(&mut self, selected: &[usize]) {
        let mut doomed = selected.to_vec();
        doomed.sort_unstable();
        for idx in doomed.into_iter().rev() {
            self.proofs.swap_remove(idx);
        }
    }

    /// Unblind blind signatures and append the resulting proofs.
    fn append_unblinded(
        &mut self,
        pending: Vec<PendingChange>,
        signatures: &[Value],
        keyset: &UpstreamKeyset,
        expected_change: u64,
    ) -> Result<(), CashuError> {
        if signatures.len() > pending.len() {
            return Err(CashuError::Protocol(format!(
                "upstream returned {} change signatures for {} blanks",
                signatures.len(),
                pending.len()
            )));
        }
        // Blank-output convention: the mint fills blanks in order, so the
        // i-th signature pairs with the i-th pending blinder; the imprinted
        // amount comes from the signature (the blank was sent as amount 0).
        let mut total = 0u64;
        for (p, sig) in pending.into_iter().zip(signatures.iter()) {
            let amount = json_amount("signature amount", &sig["amount"])?;
            let c_prime = json_point(&sig["C_"])?;
            let pubkey = keyset.keys.get(&amount).ok_or(CashuError::KeysetNotFound)?;
            let c = unblind_signature(&c_prime, &p.blinder, pubkey)
                .map_err(|_| CashuError::Crypto("reserve unblind failed".to_string()))?;
            total = total.checked_add(amount).ok_or(CashuError::InvalidAmount)?;
            self.proofs.push(nut00::Proof {
                amount,
                id: keyset.id.clone(),
                secret: p.secret_hex,
                c,
                dleq: None,
            });
        }
        if total != expected_change {
            return Err(CashuError::Protocol(format!(
                "upstream change sum {total} != expected overpay {expected_change}"
            )));
        }
        Ok(())
    }
}
