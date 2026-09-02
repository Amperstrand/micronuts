//! Demo mint core state machine.
//!
//! Implements the Cashu mint API methods as direct function calls.
//! All state is in-memory; nothing survives a restart.
//!
//! Payment safety (backend-driven prototype):
//! - Mint quotes: UNPAID → PAID → ISSUED, settled lazily via the
//!   [`LightningBackend`](crate::ln::LightningBackend) seam, with expiry.
//! - Melt quotes: UNPAID → PENDING → PAID | FAILED, with proof rollback
//!   when the backend payment fails.
//! - Double-spends are rejected atomically (see [`DemoMint::claim_proofs`]).
//! - Input fees follow NUT-08: `mint_fee = (sum_ppk + 999) / 1000`.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

// Crypto primitives delegate to the upstream `cashu` crate; conversions to
// `cashu-core-lite` types (the MintService seam) happen at the call sites.
use cashu::dhke::{
    hash_to_curve as cashu_hash_to_curve, sign_message as cashu_sign_message,
    verify_message as cashu_verify_message,
};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};

use crate::keyset::DemoKeyset;
use crate::ln::{FakeWallet, LightningBackend, MintClock, SystemClock};
use crate::type_conversion::{
    cashu_pk_to_lite, cashu_sk_to_lite, lite_pk_to_cashu, lite_sk_to_cashu,
};

/// Lifetime granted to new mint and melt quotes (unix seconds).
const QUOTE_TTL_SECS: u64 = 3600;

/// Prototype-local terminal melt state for "the backend payment failed".
/// NUT-05 itself only defines UNPAID/PENDING/PAID; FAILED records that the
/// payment errored and the input proofs were released back to the wallet.
pub const MELT_STATE_FAILED: &str = "FAILED";

/// In-memory mint quote state.
struct MintQuoteEntry {
    pub amount: u64,
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
    pub amount_paid: u64,
    pub amount_issued: u64,
    pub updated_at: u64,
}

/// In-memory melt quote state.
struct MeltQuoteEntry {
    pub amount: u64,
    pub fee_reserve: u64,
    #[allow(dead_code)] // Kept for future use (e.g., multi-unit quotes)
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
}

/// Demo Cashu mint with in-memory state.
///
/// The mint is backend-driven: invoice creation, settlement checks, and
/// payments go through the injected [`LightningBackend`], and quote expiry
/// uses the injected [`MintClock`]. [`DemoMint::new`] wires the auto-settling
/// [`FakeWallet`] with zero fees so the demo binaries keep their historical
/// behavior.
///
/// Remaining prototype limits: no persistence, single hardcoded keyset
/// (unit: sat), `fee_reserve` always 0, no PENDING polling for melts.
pub struct DemoMint {
    /// NUT-01/02: the single active keyset.
    keyset: DemoKeyset,
    /// NUT-04: in-memory mint quote table.
    mint_quotes: HashMap<String, MintQuoteEntry>,
    /// NUT-05: in-memory melt quote table.
    melt_quotes: HashMap<String, MeltQuoteEntry>,
    /// NUT-07: in-memory spent proof Y-values (hex-encoded for easy lookup).
    spent_ys: HashSet<String>,
    /// NUT-09: B_ hex → blind signature for every output this mint signed
    /// (session-scoped restore index).
    issued_outputs: HashMap<String, nut00::BlindSignature>,
    /// Monotonic counter for generating quote IDs.
    quote_counter: u64,
    /// Lightning seam for invoices, settlement, and payments.
    backend: Box<dyn LightningBackend + Send>,
    /// Time source for quote expiry.
    clock: Box<dyn MintClock + Send>,
}

impl DemoMint {
    /// Create a new demo mint: auto-settling [`FakeWallet`], real system
    /// clock, and the zero-fee default keyset.
    pub fn new() -> Self {
        Self::with_backend(Box::new(FakeWallet), Box::new(SystemClock), 0)
    }

    /// Create a mint driven by an explicit backend, clock, and keyset fee.
    ///
    /// `input_fee_ppk` sets the keyset's NUT-08 input fee (0 = no fee). The
    /// underlying keys are the standard demo keys regardless of the fee.
    pub fn with_backend(
        backend: Box<dyn LightningBackend + Send>,
        clock: Box<dyn MintClock + Send>,
        input_fee_ppk: u64,
    ) -> Self {
        Self {
            keyset: DemoKeyset::demo_with_fee(input_fee_ppk),
            mint_quotes: HashMap::new(),
            melt_quotes: HashMap::new(),
            spent_ys: HashSet::new(),
            issued_outputs: HashMap::new(),
            quote_counter: 0,
            backend,
            clock,
        }
    }

    /// Get the active keyset ID.
    pub fn keyset_id(&self) -> &str {
        &self.keyset.id
    }

    /// Get the active public keyset (NUT-01).
    pub fn public_keyset(&self) -> nut01::KeySet {
        self.keyset.to_public_keyset()
    }

    fn next_quote_id(&mut self) -> String {
        self.quote_counter += 1;
        format!("{:016x}", self.quote_counter)
    }

    fn quote_expiry(&self) -> Result<u64, CashuError> {
        self.clock
            .now_secs()
            .checked_add(QUOTE_TTL_SECS)
            .ok_or(CashuError::InvalidAmount)
    }

    // ---- NUT-06: Mint Info ----

    /// NUT-06: Return mint information.
    pub fn get_info(&self) -> Result<nut06::MintInfo, CashuError> {
        // Use the first denomination key's public key as the mint pubkey
        let mint_pk = self.keyset.keys[0].2.to_encoded_point(true);
        let pubkey_hex = hex::encode(mint_pk.as_bytes());

        Ok(nut06::MintInfo {
            name: "Micronuts Demo Mint".to_string(),
            pubkey: pubkey_hex,
            version: "micronuts-mint/0.1.0".to_string(),
            description: "In-memory demo Cashu mint for Micronuts development".to_string(),
            contact: vec![],
            nuts: demo_nuts(),
        })
    }

    // ---- NUT-01: Mint Public Keys ----

    /// NUT-01: Return all active keysets with public keys.
    pub fn get_keys(&self) -> Result<nut01::KeysResponse, CashuError> {
        Ok(nut01::KeysResponse {
            keysets: vec![self.keyset.to_public_keyset()],
        })
    }

    // ---- NUT-02: Keysets ----

    /// NUT-02: Return keyset metadata.
    pub fn get_keysets(&self) -> Result<nut02::KeysetsResponse, CashuError> {
        Ok(nut02::KeysetsResponse {
            keysets: vec![self.keyset.to_keyset_info()],
        })
    }

    // ---- NUT-04: Mint Quote + Mint ----

    /// NUT-04: Create a new mint quote in state UNPAID.
    ///
    /// The payment request comes from the Lightning backend; the quote
    /// settles lazily when a lookup observes the invoice as paid.
    pub fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        if request.amount == 0 {
            return Err(CashuError::InvalidAmount);
        }

        let quote_id = self.next_quote_id();
        let invoice = self.backend.create_invoice(request.amount, "micronuts")?;
        let expiry = self.quote_expiry()?;
        let now = self.clock.now_secs();
        let unit = request.unit;

        let entry = MintQuoteEntry {
            amount: request.amount,
            request: invoice.clone(),
            state: nut04::state::UNPAID.to_string(),
            expiry,
            amount_paid: 0,
            amount_issued: 0,
            updated_at: now,
            unit: unit.clone(),
        };
        self.mint_quotes.insert(quote_id.clone(), entry);

        Ok(nut04::MintQuoteResponse {
            quote: quote_id,
            request: invoice,
            paid: false,
            state: nut04::state::UNPAID.to_string(),
            expiry,
            amount: request.amount,
            unit,
            amount_paid: 0,
            amount_issued: 0,
            updated_at: now,
        })
    }

    /// NUT-04: Look up a mint quote, settling it first if the backend
    /// reports the invoice paid.
    pub fn get_mint_quote(
        &mut self,
        quote_id: &str,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.refresh_mint_quote_state(quote_id)?;

        let entry = self
            .mint_quotes
            .get(quote_id)
            .ok_or(CashuError::QuoteNotFound)?;

        Ok(nut04::MintQuoteResponse {
            quote: quote_id.to_string(),
            request: entry.request.clone(),
            paid: entry.state == nut04::state::PAID || entry.state == nut04::state::ISSUED,
            state: entry.state.clone(),
            expiry: entry.expiry,
            amount: entry.amount,
            unit: entry.unit.clone(),
            amount_paid: entry.amount_paid,
            amount_issued: entry.amount_issued,
            updated_at: entry.updated_at,
        })
    }

    /// Lazily settle an UNPAID quote via the backend.
    ///
    /// Simplification: a quote whose expiry has passed stays UNPAID even if
    /// the invoice was paid late — a post_mint on it then fails with
    /// QuoteNotPaid. Rescuing paid-but-expired quotes needs durable invoice
    /// state and is out of prototype scope.
    fn refresh_mint_quote_state(&mut self, quote_id: &str) -> Result<(), CashuError> {
        let invoice = {
            let entry = self
                .mint_quotes
                .get(quote_id)
                .ok_or(CashuError::QuoteNotFound)?;
            if entry.state != nut04::state::UNPAID || self.clock.now_secs() > entry.expiry {
                return Ok(());
            }
            entry.request.clone()
        };

        if self.backend.is_settled(&invoice)? {
            let now = self.clock.now_secs();
            let entry = self
                .mint_quotes
                .get_mut(quote_id)
                .ok_or(CashuError::QuoteNotFound)?;
            entry.state = nut04::state::PAID.to_string();
            entry.amount_paid = entry.amount;
            // NUT-04: updated_at MUST increase monotonically even when the
            // clock resolution does not.
            entry.updated_at = now.max(entry.updated_at + 1);
        }
        Ok(())
    }

    /// NUT-04: Mint ecash tokens by signing blinded outputs.
    ///
    /// Verifies:
    ///   - Quote exists and is PAID (settling it first if the backend paid)
    ///   - Output amounts sum to the quoted amount
    ///   - Each denomination has a known key
    pub fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        self.refresh_mint_quote_state(&request.quote)?;

        let (amount_paid, amount_issued, current_state) = {
            let entry = self
                .mint_quotes
                .get(&request.quote)
                .ok_or(CashuError::QuoteNotFound)?;
            (entry.amount_paid, entry.amount_issued, entry.state.clone())
        };

        if current_state != nut04::state::PAID {
            if current_state == nut04::state::ISSUED {
                return Err(CashuError::QuoteAlreadyIssued);
            }
            // UNPAID: either not settled yet, or expired (see refresh).
            return Err(CashuError::QuoteNotPaid);
        }

        // NUT-04: outputs MUST NOT exceed the currently mintable amount
        // (amount_paid - amount_issued); partial mints are allowed and only
        // increase amount_issued by what was issued.
        let mintable = amount_paid
            .checked_sub(amount_issued)
            .ok_or(CashuError::Protocol(
                "amount_issued exceeded amount_paid".to_string(),
            ))?;
        let output_sum: u64 = request
            .outputs
            .iter()
            .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
            .ok_or(CashuError::InvalidAmount)?;
        if output_sum > mintable {
            return Err(CashuError::AmountMismatch);
        }

        // NUT-00: sign each blinded output: C_ = k * B_
        let signatures = self.sign_outputs(&request.outputs)?;

        {
            let now = self.clock.now_secs();
            let entry = self
                .mint_quotes
                .get_mut(&request.quote)
                .ok_or(CashuError::QuoteNotFound)?;
            entry.amount_issued = amount_issued
                .checked_add(output_sum)
                .ok_or(CashuError::InvalidAmount)?;
            if entry.amount_issued == entry.amount_paid {
                entry.state = nut04::state::ISSUED.to_string();
            }
            entry.updated_at = now.max(entry.updated_at + 1);
        }

        Ok(nut04::MintResponse { signatures })
    }

    // ---- NUT-05: Melt Quote + Melt ----

    /// NUT-05: Create a new melt quote in state UNPAID.
    ///
    /// The amount comes from the backend's invoice lookup. `fee_reserve` is
    /// 0 for now — a real backend provides the expected routing fee with
    /// the quote.
    pub fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let amount = self.backend.lookup_amount(&request.request)?;
        let fee_reserve = 0;

        let quote_id = self.next_quote_id();
        let expiry = self.quote_expiry()?;

        let entry = MeltQuoteEntry {
            amount,
            fee_reserve,
            unit: request.unit.clone(),
            request: request.request.clone(),
            state: nut05::state::UNPAID.to_string(),
            expiry,
        };
        self.melt_quotes.insert(quote_id.clone(), entry);

        let entry_request = request.request;
        let entry_unit = request.unit;
        Ok(nut05::MeltQuoteResponse {
            quote: quote_id,
            amount,
            fee_reserve,
            paid: false,
            state: nut05::state::UNPAID.to_string(),
            expiry,
            request: entry_request,
            unit: entry_unit,
        })
    }

    /// NUT-05: Look up a melt quote. No refresh: melt settlement is
    /// synchronous inside [`DemoMint::post_melt`] in this prototype.
    pub fn get_melt_quote(&self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let entry = self
            .melt_quotes
            .get(quote_id)
            .ok_or(CashuError::QuoteNotFound)?;

        Ok(nut05::MeltQuoteResponse {
            quote: quote_id.to_string(),
            amount: entry.amount,
            fee_reserve: entry.fee_reserve,
            paid: entry.state == nut05::state::PAID,
            state: entry.state.clone(),
            expiry: entry.expiry,
            request: entry.request.clone(),
            unit: entry.unit.clone(),
        })
    }

    /// NUT-05: Execute a melt (spend proofs to pay a Lightning invoice).
    ///
    /// State machine: UNPAID → PENDING → PAID on payment success, or
    /// → FAILED with the input proofs released (removed from the spent set)
    /// when the backend payment errors.
    pub fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        let (amount, fee_reserve, invoice, state) = {
            let entry = self
                .melt_quotes
                .get(&request.quote)
                .ok_or(CashuError::QuoteNotFound)?;
            (
                entry.amount,
                entry.fee_reserve,
                entry.request.clone(),
                entry.state.clone(),
            )
        };

        match state.as_str() {
            nut05::state::PAID => return Err(CashuError::MeltAlreadyPaid),
            nut05::state::UNPAID => {}
            MELT_STATE_FAILED => return Err(CashuError::PaymentFailed),
            // PENDING is set and resolved within a single post_melt call in
            // this single-threaded mint, so it is never observable at entry.
            _ => {
                return Err(CashuError::Protocol(
                    "melt quote in unexpected state".to_string(),
                ))
            }
        }

        // NUT-05/NUT-08: inputs must cover amount + fee_reserve + input fee.
        let input_sum = self.verify_proofs(&request.inputs)?;
        let fee = self.input_fee_total(&request.inputs)?;
        let required = amount
            .checked_add(fee_reserve)
            .and_then(|v| v.checked_add(fee))
            .ok_or(CashuError::InvalidAmount)?;
        if input_sum < required {
            return Err(CashuError::InsufficientInputs);
        }

        // NUT-08 fee return: change comes either as explicit outputs whose
        // sum must equal the overpay exactly, or as blank outputs (amount 0)
        // onto which the mint imprints a power-of-two decomposition of the
        // overpay. Without outputs, the overpay is burned.
        let change_outputs: Option<Vec<nut00::BlindedMessage>> = match &request.outputs {
            Some(outputs) => {
                let explicit_sum: u64 = outputs
                    .iter()
                    .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
                    .ok_or(CashuError::InvalidAmount)?;
                let overpay = input_sum - required;
                let blank_count = outputs.iter().filter(|o| o.amount == 0).count();
                let remainder = overpay - explicit_sum;
                // Blanks with zero remainder are tolerated (signed back as
                // nothing); only explicit change must consume the overpay.
                if explicit_sum > overpay || (blank_count == 0 && remainder != 0) {
                    return Err(CashuError::AmountMismatch);
                }
                self.check_outputs_signable(outputs)?;
                let mut filled = outputs.clone();
                if blank_count > 0 {
                    self.imprint_blank_outputs(&mut filled, remainder, blank_count)?;
                }
                Some(filled)
            }
            None => None,
        };

        // Atomically claim all inputs before touching the backend.
        self.claim_proofs(&request.inputs)?;

        // State transition: UNPAID → PENDING while the payment is in flight.
        self.set_melt_state(&request.quote, nut05::state::PENDING)?;

        match self.backend.pay_invoice(&invoice, amount) {
            Ok(preimage) => {
                // Unfilled blanks (fewer decomposition pieces than blanks)
                // are dropped — only imprinted outputs are signed.
                let change = match &change_outputs {
                    Some(outputs) => {
                        let filled: Vec<nut00::BlindedMessage> =
                            outputs.iter().filter(|o| o.amount > 0).cloned().collect();
                        if filled.is_empty() {
                            None
                        } else {
                            Some(self.sign_outputs(&filled)?)
                        }
                    }
                    None => None,
                };
                // State transition: PENDING → PAID
                self.set_melt_state(&request.quote, nut05::state::PAID)?;
                let entry = self
                    .melt_quotes
                    .get(&request.quote)
                    .ok_or(CashuError::QuoteNotFound)?;
                Ok(nut05::MeltResponse {
                    paid: true,
                    state: nut05::state::PAID.to_string(),
                    payment_preimage: Some(preimage),
                    change,
                    quote: request.quote.clone(),
                    amount: entry.amount,
                    fee_reserve: entry.fee_reserve,
                    unit: entry.unit.clone(),
                    expiry: entry.expiry,
                    request: entry.request.clone(),
                })
            }
            Err(_) => {
                // Payment failed: release the claimed proofs so the wallet
                // can spend them again, and park the quote in FAILED.
                for proof in &request.inputs {
                    if let Ok(y_hex) = proof_y_hex(&proof.secret) {
                        self.spent_ys.remove(&y_hex);
                    }
                }
                self.set_melt_state(&request.quote, MELT_STATE_FAILED)?;
                Err(CashuError::PaymentFailed)
            }
        }
    }

    // ---- NUT-03: Swap ----

    /// NUT-03: Swap proofs for new blinded outputs.
    ///
    /// Requires `input_sum == output_sum + mint_fee(inputs)` exactly
    /// (NUT-08); both under- and over-funded swaps are rejected. With
    /// `input_fee_ppk = 0` this is the previous exact-equality check.
    pub fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        // Verify input proofs (keyset, spent-set, signature)
        let input_sum = self.verify_proofs(&request.inputs)?;

        let output_sum: u64 = request
            .outputs
            .iter()
            .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
            .ok_or(CashuError::InvalidAmount)?;
        let fee = self.input_fee_total(&request.inputs)?;
        let required = output_sum
            .checked_add(fee)
            .ok_or(CashuError::InvalidAmount)?;
        if input_sum != required {
            return Err(CashuError::AmountMismatch);
        }

        self.check_outputs_signable(&request.outputs)?;

        // Atomically mark old proofs as spent
        self.claim_proofs(&request.inputs)?;

        // NUT-00: sign new outputs
        let signatures = self.sign_outputs(&request.outputs)?;

        Ok(nut03::SwapResponse { signatures })
    }

    // ---- NUT-07: Check State ----

    /// NUT-07: Check the spent state of proofs.
    ///
    /// Demo shortcut: only checks the in-memory spent set. No durable state.
    pub fn post_check_state(
        &self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        let states = request
            .ys
            .iter()
            .map(|y| {
                let y_hex = hex::encode(y.to_encoded_point(true).as_bytes());
                let state = if self.spent_ys.contains(&y_hex) {
                    nut07::state::SPENT
                } else {
                    nut07::state::UNSPENT
                };
                nut07::ProofState {
                    y: *y,
                    state: state.to_string(),
                    witness: None,
                }
            })
            .collect();

        Ok(nut07::CheckStateResponse { states })
    }

    // ---- NUT-09: Restore ----

    /// NUT-09: Restore signatures for previously signed outputs.
    ///
    /// Session-scoped: every output the mint signs (mint, swap, melt change)
    /// is recorded by its `B_` value; restore returns the stored signature
    /// for each known `B_` and skips unknown ones. Nothing survives a mint
    /// restart.
    pub fn post_restore(
        &self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        let outputs = request
            .outputs
            .iter()
            .filter_map(|b| {
                let b_hex = hex::encode(b.to_encoded_point(true).as_bytes());
                self.issued_outputs
                    .get(&b_hex)
                    .map(|signature| nut09::RestoreOutput {
                        y: *b,
                        signature: signature.clone(),
                    })
            })
            .collect();

        Ok(nut09::RestoreResponse { outputs })
    }

    // ---- Internal helpers ----

    fn set_melt_state(&mut self, quote_id: &str, state: &str) -> Result<(), CashuError> {
        let entry = self
            .melt_quotes
            .get_mut(quote_id)
            .ok_or(CashuError::QuoteNotFound)?;
        entry.state = state.to_string();
        Ok(())
    }

    /// NUT-08 input fee for a set of inputs: `(sum_ppk + 999) / 1000` where
    /// each input contributes its keyset's `input_fee_ppk`. All inputs are
    /// verified against the single active keyset before this is called.
    fn input_fee_total(&self, inputs: &[nut00::Proof]) -> Result<u64, CashuError> {
        let sum_ppk = self
            .keyset
            .input_fee_ppk
            .checked_mul(inputs.len() as u64)
            .ok_or(CashuError::InvalidAmount)?;
        sum_ppk
            .checked_add(999)
            .map(|v| v / 1000)
            .ok_or(CashuError::InvalidAmount)
    }

    /// Reject outputs the mint could not sign (unknown denomination) before
    /// any state is claimed. Blank outputs (amount 0) are skipped — they are
    /// imprinted with real denominations before signing.
    fn check_outputs_signable(&self, outputs: &[nut00::BlindedMessage]) -> Result<(), CashuError> {
        for output in outputs {
            if output.amount != 0 && self.keyset.get_secret_key(output.amount).is_none() {
                return Err(CashuError::KeysetNotFound);
            }
        }
        Ok(())
    }

    /// NUT-08: assign `remainder` sats to the blank (amount 0) outputs,
    /// largest denomination first, so the imprinted amounts sum to
    /// `remainder` using at most `blank_count` outputs.
    fn imprint_blank_outputs(
        &self,
        outputs: &mut [nut00::BlindedMessage],
        remainder: u64,
        blank_count: usize,
    ) -> Result<(), CashuError> {
        let mut pieces = Vec::with_capacity(blank_count);
        let mut left = remainder;
        while left > 0 {
            let denom = self
                .keyset
                .keys
                .iter()
                .rev()
                .find(|k| k.0 <= left)
                .map(|k| k.0)
                .ok_or(CashuError::InvalidAmount)?;
            pieces.push(denom);
            left -= denom;
        }
        if pieces.len() > blank_count {
            return Err(CashuError::AmountMismatch);
        }
        let mut next = pieces.into_iter();
        for output in outputs.iter_mut() {
            if output.amount == 0 {
                if let Some(amount) = next.next() {
                    output.amount = amount;
                }
            }
        }
        Ok(())
    }

    /// NUT-00: Sign a set of blinded outputs.
    ///
    /// For each output, looks up the mint's private key for that denomination
    /// and computes `C_ = k * B_` via the upstream `cashu::dhke` primitive.
    /// Every signature is also recorded for NUT-09 restore.
    fn sign_outputs(
        &mut self,
        outputs: &[nut00::BlindedMessage],
    ) -> Result<Vec<nut00::BlindSignature>, CashuError> {
        let mut signatures = Vec::with_capacity(outputs.len());
        for output in outputs {
            let sk = self
                .keyset
                .get_secret_key(output.amount)
                .ok_or(CashuError::KeysetNotFound)?;

            // NUT-00: C_ = k * B_ — cross the cashu-core-lite → cashu boundary,
            // delegate to `cashu::dhke::sign_message`, then convert back.
            let cashu_sk = lite_sk_to_cashu(sk);
            let cashu_blinded = lite_pk_to_cashu(&output.b);
            let cashu_c_prime = cashu_sign_message(&cashu_sk, &cashu_blinded)
                .map_err(|_| CashuError::Crypto("cashu::dhke::sign_message failed".to_string()))?;
            let c_prime = cashu_pk_to_lite(&cashu_c_prime);

            let dleq = {
                let cashu_bs = cashu::nuts::nut00::BlindSignature::new(
                    cashu::Amount::from(output.amount),
                    cashu_c_prime,
                    cashu::nuts::nut02::Id::from_str(&self.keyset.id)
                        .map_err(|e| CashuError::Crypto(format!("invalid keyset id: {e}")))?,
                    &cashu_blinded,
                    cashu_sk,
                )
                .map_err(|e| CashuError::Crypto(format!("DLEQ construction failed: {e:?}")))?;

                let dleq_ref = cashu_bs.dleq.as_ref().ok_or_else(|| {
                    CashuError::Crypto("BlindSignature::new did not produce DLEQ".to_string())
                })?;
                cashu_core_lite::nuts::nut12::BlindSignatureDleq {
                    e: cashu_sk_to_lite(&dleq_ref.e),
                    s: cashu_sk_to_lite(&dleq_ref.s),
                }
            };

            let signature = nut00::BlindSignature {
                amount: output.amount,
                id: self.keyset.id.clone(),
                c: c_prime,
                dleq: Some(dleq),
            };

            // NUT-09: index the signature by its B_ for later restore.
            let b_hex = hex::encode(output.b.to_encoded_point(true).as_bytes());
            self.issued_outputs.insert(b_hex, signature.clone());

            signatures.push(signature);
        }
        Ok(signatures)
    }

    /// Verify a set of proofs against mint keys.
    ///
    /// Checks that `k * hash_to_curve(secret) == C` for each proof via the
    /// upstream `cashu::dhke::verify_message` primitive (mint holds the
    /// private key, so the privkey verification path is used), plus two
    /// defenses before any state change: every proof must reference the
    /// active keyset, and no proof may already be in the spent set.
    /// Returns the total amount of verified proofs.
    fn verify_proofs(&self, proofs: &[nut00::Proof]) -> Result<u64, CashuError> {
        let mut total = 0u64;
        for proof in proofs {
            if proof.id != self.keyset.id {
                return Err(CashuError::KeysetNotFound);
            }
            if self.spent_ys.contains(&proof_y_hex(&proof.secret)?) {
                return Err(CashuError::TokensAlreadySpent);
            }

            let sk = self
                .keyset
                .get_secret_key(proof.amount)
                .ok_or(CashuError::KeysetNotFound)?;

            // NUT-00: verify k * hash_to_curve(secret) == C using the mint's
            // private key. `cashu::dhke::verify_message` returns Ok iff valid.
            // Per DHKE spec, hash_to_curve operates on the secret STRING bytes.
            let cashu_sk = lite_sk_to_cashu(sk);
            let cashu_c = lite_pk_to_cashu(&proof.c);
            cashu_verify_message(&cashu_sk, cashu_c, proof.secret.as_bytes())
                .map_err(|_| CashuError::Crypto("verify_message failed".to_string()))?;

            total = total
                .checked_add(proof.amount)
                .ok_or(CashuError::InvalidAmount)?;
        }
        Ok(total)
    }

    /// Atomically mark all input proofs as spent.
    ///
    /// Computes `Y = hash_to_curve(secret)` for every proof and rejects the
    /// whole batch — leaving the spent set untouched — if any Y is already
    /// spent or duplicated within the batch. All Ys are inserted only after
    /// the full batch validates; since the mint is single-threaded
    /// in-process (`&mut self`), this is atomic.
    fn claim_proofs(&mut self, proofs: &[nut00::Proof]) -> Result<(), CashuError> {
        let ys = proofs
            .iter()
            .map(|p| proof_y_hex(&p.secret))
            .collect::<Result<Vec<_>, _>>()?;

        let mut batch = HashSet::with_capacity(ys.len());
        for y in &ys {
            if self.spent_ys.contains(y) || !batch.insert(y.clone()) {
                return Err(CashuError::TokensAlreadySpent);
            }
        }
        for y in &ys {
            self.spent_ys.insert(y.clone());
        }
        Ok(())
    }
}

/// Hex-encoded `Y = hash_to_curve(secret)` for a proof secret, matching the
/// encoding used by the spent set and NUT-07 checkstate.
fn proof_y_hex(secret: &str) -> Result<String, CashuError> {
    let cashu_y = cashu_hash_to_curve(secret.as_bytes())
        .map_err(|_| CashuError::Crypto("hash_to_curve failed".to_string()))?;
    let y = cashu_pk_to_lite(&cashu_y);
    Ok(hex::encode(y.to_encoded_point(true).as_bytes()))
}

/// NUT-06 advertisement: which NUTs this mint supports and their settings.
/// NUTs 4 and 5 accept bolt11 invoices in sat; the others need no settings.
fn demo_nuts() -> Vec<(String, nut06::NutSettings)> {
    let bolt11_sat = || {
        vec![nut06::PaymentMethod {
            method: "bolt11".to_string(),
            unit: "sat".to_string(),
        }]
    };
    vec![
        ("3".to_string(), nut06::NutSettings { methods: vec![] }),
        (
            "4".to_string(),
            nut06::NutSettings {
                methods: bolt11_sat(),
            },
        ),
        (
            "5".to_string(),
            nut06::NutSettings {
                methods: bolt11_sat(),
            },
        ),
        ("6".to_string(), nut06::NutSettings { methods: vec![] }),
        ("7".to_string(), nut06::NutSettings { methods: vec![] }),
        ("9".to_string(), nut06::NutSettings { methods: vec![] }),
    ]
}

impl Default for DemoMint {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_mint_info() {
        let mint = DemoMint::new();
        let info = mint.get_info().unwrap();
        assert_eq!(info.name, "Micronuts Demo Mint");
        assert!(!info.pubkey.is_empty());
    }

    #[test]
    fn test_demo_mint_info_nuts_shape() {
        let mint = DemoMint::new();
        let info = mint.get_info().unwrap();
        let nuts = info.nuts;
        let advertised: Vec<&str> = nuts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(advertised, ["3", "4", "5", "6", "7", "9"]);

        for (nut, settings) in &nuts {
            match nut.as_str() {
                "4" | "5" => {
                    assert_eq!(settings.methods.len(), 1, "nut {nut} advertises bolt11");
                    assert_eq!(settings.methods[0].method, "bolt11");
                    assert_eq!(settings.methods[0].unit, "sat");
                }
                _ => assert!(
                    settings.methods.is_empty(),
                    "nut {nut} has no method settings"
                ),
            }
        }
    }

    #[test]
    fn test_demo_mint_keys() {
        let mint = DemoMint::new();
        let keys = mint.get_keys().unwrap();
        assert_eq!(keys.keysets.len(), 1);
        assert_eq!(keys.keysets[0].unit, "sat");
    }

    #[test]
    fn test_demo_mint_keysets() {
        let mint = DemoMint::new();
        let keysets = mint.get_keysets().unwrap();
        assert_eq!(keysets.keysets.len(), 1);
        assert!(keysets.keysets[0].active);
        assert_eq!(keysets.keysets[0].input_fee_ppk, 0);
    }

    #[test]
    fn test_mint_quote_settles_via_backend_on_lookup() {
        let mut mint = DemoMint::new();
        let resp = mint
            .post_mint_quote(nut04::MintQuoteRequest {
                amount: 100,
                unit: "sat".to_string(),
            })
            .unwrap();
        assert!(!resp.paid);
        assert_eq!(resp.state, nut04::state::UNPAID);

        // FakeWallet settles instantly, so the lookup flips the state.
        let checked = mint.get_mint_quote(&resp.quote).unwrap();
        assert!(checked.paid);
        assert_eq!(checked.state, nut04::state::PAID);
    }

    #[test]
    fn test_invalid_melt_quote_invoice_rejected() {
        let mut mint = DemoMint::new();
        let result = mint.post_melt_quote(nut05::MeltQuoteRequest {
            request: "garbage".to_string(),
            unit: "sat".to_string(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_restore_empty_request_returns_empty() {
        let mint = DemoMint::new();
        let request = nut09::RestoreRequest { outputs: vec![] };
        let response = mint.post_restore(request).unwrap();
        assert_eq!(response.outputs.len(), 0);
    }
}
