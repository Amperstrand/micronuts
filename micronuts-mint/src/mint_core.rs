//! Demo mint core state machine.
//!
//! Implements the Cashu mint API methods as direct function calls.
//! All state is in-memory; nothing survives a restart.

use std::collections::HashMap;

use cashu_core_lite::crypto::{hash_to_curve, sign_message, verify_signature};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07};

use crate::keyset::DemoKeyset;

/// In-memory mint quote state.
#[allow(dead_code)] // Fields kept for future use (e.g., quote lookup by unit)
struct MintQuoteEntry {
    pub amount: u64,
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
}

/// In-memory melt quote state.
#[allow(dead_code)] // Fields kept for future use (e.g., invoice replay check)
struct MeltQuoteEntry {
    pub amount: u64,
    pub fee_reserve: u64,
    pub unit: String,
    pub request: String,
    pub state: String,
    pub expiry: u64,
}

/// Demo Cashu mint with in-memory state.
///
/// Demo shortcuts:
/// - Mint quotes are auto-paid (state transitions UNPAID → PAID immediately).
/// - Melt quotes auto-succeed.
/// - No durable spent-proof set — double-spend allowed within session.
/// - Single hardcoded keyset (unit: sat).
pub struct DemoMint {
    /// NUT-01/02: the single active keyset.
    keyset: DemoKeyset,
    /// NUT-04: in-memory mint quote table.
    mint_quotes: HashMap<String, MintQuoteEntry>,
    /// NUT-05: in-memory melt quote table.
    melt_quotes: HashMap<String, MeltQuoteEntry>,
    /// NUT-07: in-memory spent proof Y-values (hex-encoded for easy lookup).
    /// Demo shortcut: not durable, lost on restart.
    spent_ys: std::collections::HashSet<String>,
    /// Monotonic counter for generating quote IDs.
    quote_counter: u64,
}

impl DemoMint {
    /// Create a new demo mint with the default deterministic keyset.
    pub fn new() -> Self {
        Self {
            keyset: DemoKeyset::demo_default(),
            mint_quotes: HashMap::new(),
            melt_quotes: HashMap::new(),
            spent_ys: std::collections::HashSet::new(),
            quote_counter: 0,
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
            nuts: nut06::NutSupport {
                // NUTs implemented in this demo
                supported: vec![0, 1, 2, 3, 4, 5, 6, 7],
            },
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

    /// NUT-04: Create a new mint quote.
    ///
    /// Demo shortcut: quote is immediately marked as PAID (no real Lightning invoice).
    pub fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        if request.amount == 0 {
            return Err(CashuError::InvalidAmount);
        }

        let quote_id = self.next_quote_id();

        // Demo shortcut: generate a dummy Lightning invoice string
        let dummy_invoice = format!("lnbcdemo{}sat1micronuts", request.amount);

        // Demo shortcut: immediately mark as PAID (auto-approve)
        let entry = MintQuoteEntry {
            amount: request.amount,
            unit: request.unit.clone(),
            request: dummy_invoice.clone(),
            state: nut04::state::PAID.to_string(),
            expiry: u64::MAX, // Demo shortcut: never expires
        };

        self.mint_quotes.insert(quote_id.clone(), entry);

        Ok(nut04::MintQuoteResponse {
            quote: quote_id,
            request: dummy_invoice,
            paid: true,
            state: nut04::state::PAID.to_string(),
            expiry: u64::MAX,
        })
    }

    /// NUT-04: Look up a mint quote.
    pub fn get_mint_quote(&self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
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
        })
    }

    /// NUT-04: Mint ecash tokens by signing blinded outputs.
    ///
    /// Verifies:
    ///   - Quote exists and is PAID
    ///   - Output amounts sum to the quoted amount
    ///   - Each output uses the active keyset ID
    ///   - Each denomination has a known key
    pub fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        // Look up quote (immutable first to check state and amount)
        let (quoted_amount, current_state) = {
            let entry = self
                .mint_quotes
                .get(&request.quote)
                .ok_or(CashuError::QuoteNotFound)?;
            (entry.amount, entry.state.clone())
        };

        if current_state != nut04::state::PAID {
            if current_state == nut04::state::ISSUED {
                return Err(CashuError::QuoteAlreadyIssued);
            }
            return Err(CashuError::QuoteNotPaid);
        }

        // Verify output amounts sum to quoted amount
        let output_sum: u64 = request.outputs.iter().map(|o| o.amount).sum();
        if output_sum != quoted_amount {
            return Err(CashuError::AmountMismatch);
        }

        // NUT-00: sign each blinded output: C_ = k * B_
        let signatures = self.sign_outputs(&request.outputs)?;

        // Mark quote as ISSUED
        if let Some(entry) = self.mint_quotes.get_mut(&request.quote) {
            entry.state = nut04::state::ISSUED.to_string();
        }

        Ok(nut04::MintResponse { signatures })
    }

    // ---- NUT-05: Melt Quote + Melt ----

    /// NUT-05: Create a new melt quote.
    ///
    /// Demo shortcut: the "invoice" can be any string; amount is parsed from it
    /// or defaulted. Fee reserve is always 0.
    pub fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let quote_id = self.next_quote_id();

        // Demo shortcut: extract amount from dummy invoice or default to 0.
        // In a real mint, the amount comes from decoding the bolt11 invoice.
        let amount = parse_demo_invoice_amount(&request.request)
            .ok_or_else(|| CashuError::Protocol("invalid demo invoice amount".to_string()))?;

        let entry = MeltQuoteEntry {
            amount,
            fee_reserve: 0, // Demo shortcut: no fees
            unit: request.unit.clone(),
            request: request.request.clone(),
            state: nut05::state::UNPAID.to_string(),
            expiry: u64::MAX,
        };

        self.melt_quotes.insert(quote_id.clone(), entry);

        Ok(nut05::MeltQuoteResponse {
            quote: quote_id,
            amount,
            fee_reserve: 0,
            paid: false,
            state: nut05::state::UNPAID.to_string(),
            expiry: u64::MAX,
        })
    }

    /// NUT-05: Look up a melt quote.
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
        })
    }

    /// NUT-05: Execute a melt (spend proofs to "pay" a Lightning invoice).
    ///
    /// Demo shortcut: no real Lightning payment occurs. The proofs are verified
    /// and the quote is marked as PAID immediately. A dummy preimage is returned.
    pub fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        // Look up quote
        let entry = self
            .melt_quotes
            .get(&request.quote)
            .ok_or(CashuError::QuoteNotFound)?;

        let required_amount = entry.amount + entry.fee_reserve;

        // Verify input proofs
        let input_sum = self.verify_proofs(&request.inputs)?;
        if input_sum < required_amount {
            return Err(CashuError::InsufficientInputs);
        }

        // Mark proofs as spent (NUT-07)
        self.mark_spent(&request.inputs)?;

        // Sign any change outputs
        let change = if let Some(outputs) = &request.outputs {
            Some(self.sign_outputs(outputs)?)
        } else {
            None
        };

        // Demo shortcut: auto-pay, mark quote as PAID
        let entry = self
            .melt_quotes
            .get_mut(&request.quote)
            .ok_or(CashuError::QuoteNotFound)?;
        entry.state = nut05::state::PAID.to_string();

        // Demo shortcut: dummy payment preimage (32 zero bytes hex)
        let dummy_preimage_hex = "00".repeat(32);

        Ok(nut05::MeltResponse {
            paid: true,
            state: nut05::state::PAID.to_string(),
            payment_preimage: Some(dummy_preimage_hex),
            change,
        })
    }

    // ---- NUT-03: Swap ----

    /// NUT-03: Swap proofs for new blinded outputs.
    ///
    /// Verifies input proofs, checks that input sum equals output sum
    /// (minus fees, which are 0 in demo), and signs the outputs.
    pub fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        // Verify input proofs
        let input_sum = self.verify_proofs(&request.inputs)?;

        // Check amounts balance (NUT-03: inputs must equal outputs + fees)
        let output_sum: u64 = request.outputs.iter().map(|o| o.amount).sum();
        // Demo shortcut: fees are 0, so exact match required
        if input_sum != output_sum {
            return Err(CashuError::AmountMismatch);
        }

        // Mark old proofs as spent
        self.mark_spent(&request.inputs)?;

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
                    y: y.clone(),
                    state: state.to_string(),
                    witness: None,
                }
            })
            .collect();

        Ok(nut07::CheckStateResponse { states })
    }

    // ---- Internal helpers ----

    /// NUT-00: Sign a set of blinded outputs.
    ///
    /// For each output, looks up the mint's private key for that denomination
    /// and computes `C_ = k * B_`.
    fn sign_outputs(
        &self,
        outputs: &[nut00::BlindedMessage],
    ) -> Result<Vec<nut00::BlindSignature>, CashuError> {
        let mut signatures = Vec::with_capacity(outputs.len());
        for output in outputs {
            let sk = self
                .keyset
                .get_secret_key(output.amount)
                .ok_or(CashuError::KeysetNotFound)?;

            // NUT-00: C_ = k * B_
            let c_prime = sign_message(sk, &output.b);

            signatures.push(nut00::BlindSignature {
                amount: output.amount,
                id: self.keyset.id.clone(),
                c: c_prime,
            });
        }
        Ok(signatures)
    }

    /// Verify a set of proofs against mint keys.
    ///
    /// Checks that `k * hash_to_curve(secret) == C` for each proof.
    /// Returns the total amount of verified proofs.
    fn verify_proofs(&self, proofs: &[nut00::Proof]) -> Result<u64, CashuError> {
        let mut total = 0u64;
        for proof in proofs {
            let sk = self
                .keyset
                .get_secret_key(proof.amount)
                .ok_or(CashuError::KeysetNotFound)?;

            // Decode hex secret to bytes
            let secret_bytes =
                hex::decode(&proof.secret).map_err(|_| CashuError::InvalidProof)?;

            // NUT-00: verify k * hash_to_curve(secret) == C
            let valid = verify_signature(&secret_bytes, &proof.c, sk)
                .map_err(|_| CashuError::Crypto("verify_signature failed".to_string()))?;

            if !valid {
                return Err(CashuError::InvalidProof);
            }

            total = total
                .checked_add(proof.amount)
                .ok_or(CashuError::InvalidAmount)?;
        }
        Ok(total)
    }

    /// Mark proofs as spent in the in-memory set.
    ///
    /// Demo shortcut: this set is not durable. Double-spending is possible
    /// across mint restarts, but within a session proofs are tracked.
    fn mark_spent(&mut self, proofs: &[nut00::Proof]) -> Result<(), CashuError> {
        for proof in proofs {
            let secret_bytes =
                hex::decode(&proof.secret).map_err(|_| CashuError::InvalidProof)?;
            let y = hash_to_curve(&secret_bytes)
                .map_err(|_| CashuError::Crypto("hash_to_curve failed".to_string()))?;
            let y_hex = hex::encode(y.to_encoded_point(true).as_bytes());
            // Demo shortcut: we allow double-spending for now (no error if already spent)
            self.spent_ys.insert(y_hex);
        }
        Ok(())
    }
}

impl Default for DemoMint {
    fn default() -> Self {
        Self::new()
    }
}

/// Demo shortcut: attempt to parse amount from a dummy invoice string.
///
/// Expects format like "lnbcdemo{amount}sat1micronuts".
/// Falls back to 1 if parsing fails. A real mint would decode the bolt11.
fn parse_demo_invoice_amount(invoice: &str) -> Option<u64> {
    let stripped = invoice.strip_prefix("lnbcdemo")?;
    let end = stripped.find("sat")?;
    stripped[..end].parse().ok()
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
    fn test_mint_quote_auto_paid() {
        let mut mint = DemoMint::new();
        let resp = mint
            .post_mint_quote(nut04::MintQuoteRequest {
                amount: 100,
                unit: "sat".to_string(),
            })
            .unwrap();
        assert!(resp.paid);
        assert_eq!(resp.state, nut04::state::PAID);
    }

    #[test]
    fn test_parse_demo_invoice() {
        assert_eq!(parse_demo_invoice_amount("lnbcdemo100sat1micronuts"), Some(100));
        assert_eq!(parse_demo_invoice_amount("lnbcdemo1sat1micronuts"), Some(1));
        assert_eq!(parse_demo_invoice_amount("garbage"), None);
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
}
