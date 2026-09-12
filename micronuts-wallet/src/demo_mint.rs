//! Embedded demo mint for the browser build: a complete in-process
//! `MintClient` (sign + DLEQ-prove + spent tracking + restore) with zero
//! network, so the wasm wallet runs every flow client-side. Also usable
//! on native for tests. Keys are deterministic per instance seed; the
//! DLEQ prover mirrors `cashu-core-lite`'s NUT-12 verifier transcript
//! (`R1 = t·G`, `R2 = t·B'`, `e = hash_e(R1, R2, A, C')`,
//! `s = t + e·a mod n`).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use cashu_core_lite::crypto::{hash_to_curve, sign_message, verify_signature_with_privkey};
use cashu_core_lite::error::CashuError;
use cashu_core_lite::keypair::{PublicKey, SecretKey};
use cashu_core_lite::nuts::nut12::{hash_e, BlindSignatureDleq};
use cashu_core_lite::nuts::{nut00, nut01, nut02, nut03, nut04, nut05, nut06, nut07, nut09};
use cashu_core_lite::transport::MintClient;
use k256::ProjectivePoint;
use rand_core::RngCore;

pub const DEMO_MINT_URL: &str = "https://demo.micronuts.invalid";
const DEMO_MINT_NAME: &str = "Browser Demo Mint";
const EXPIRY_FAR_FUTURE: u64 = 4_102_444_800;

#[derive(Clone)]
pub struct DemoMintClient {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    keys: nut01::KeySet,
    privkeys: BTreeMap<u64, SecretKey>,
    mint_pubkey: PublicKey,
    quotes: HashMap<String, nut04::MintQuoteResponse>,
    melt_quotes: HashMap<String, nut05::MeltQuoteResponse>,
    spent_ys: HashSet<[u8; 33]>,
    signed: HashMap<[u8; 33], nut00::BlindSignature>,
    next_quote: u64,
}

impl DemoMintClient {
    pub fn new(seed: [u8; 32]) -> Self {
        let mut privkeys = BTreeMap::new();
        let mut keys = Vec::new();
        let mut mint_key_bytes = [0u8; 32];
        mint_key_bytes.copy_from_slice(&blakeish_subkey(&seed, b"browser-demo-mint-identity", 0));
        let mint_key = SecretKey::from_slice(&mint_key_bytes).expect("valid identity scalar");

        for exp in 0..8u32 {
            let amount = 1u64 << exp;
            let material = blakeish_subkey(&seed, b"browser-demo-mint-amount", amount);
            let privkey = SecretKey::from_slice(&material).expect("valid scalar");
            privkeys.insert(amount, privkey.clone());
            keys.push(nut01::KeyPair {
                amount,
                pubkey: privkey.public_key(),
            });
        }
        let pubkeys: Vec<PublicKey> = keys.iter().map(|kp| kp.pubkey).collect();
        let keyset_id = nut02::derive_keyset_id(&pubkeys);
        Self {
            inner: Rc::new(RefCell::new(Inner {
                keys: nut01::KeySet {
                    id: keyset_id,
                    unit: String::from("sat"),
                    keys,
                },
                privkeys,
                mint_pubkey: mint_key.public_key(),
                quotes: HashMap::new(),
                melt_quotes: HashMap::new(),
                spent_ys: HashSet::new(),
                signed: HashMap::new(),
                next_quote: 1,
            })),
        }
    }

    pub fn keyset_id(&self) -> String {
        self.inner.borrow().keys.id.clone()
    }
}

/// Deterministic 32-byte subkey (SHA-256(label || index_be64 || seed)).
fn blakeish_subkey(seed: &[u8; 32], label: &[u8], index: u64) -> [u8; 32] {
    use sha2::digest::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(label);
    hasher.update(index.to_be_bytes());
    hasher.update(seed);
    hasher.finalize().into()
}

/// NUT-12 mint-side DLEQ prover over the same transcript the verifier
/// checks: pick nonce t, publish R1 = t·G and R2 = t·B', derive
/// e = hash_e(R1, R2, A, C'), answer s = t + e·a.
fn dleq_prove(
    amount_key: &SecretKey,
    blinded_message: &PublicKey,
    blinded_signature: &PublicKey,
    rng: &mut dyn RngCore,
) -> Option<BlindSignatureDleq> {
    loop {
        let mut nonce_bytes = [0u8; 32];
        rng.fill_bytes(&mut nonce_bytes);
        let Ok(nonce) = SecretKey::from_slice(&nonce_bytes) else {
            continue;
        };
        let t = nonce.to_scalar();
        let r1 = PublicKey::from_affine((ProjectivePoint::GENERATOR * t).into())?;
        let r2 = PublicKey::from_affine((ProjectivePoint::from(blinded_message) * t).into())?;
        let e_bytes = hash_e(&r1, &r2, &amount_key.public_key(), blinded_signature);
        let Ok(e) = SecretKey::from_slice(&e_bytes) else {
            continue;
        };
        let s = t + e.to_scalar() * amount_key.to_scalar();
        let Ok(answer) = SecretKey::from_slice(&s.to_bytes()) else {
            continue;
        };
        return Some(BlindSignatureDleq { e, s: answer });
    }
}

impl Inner {
    fn sign_outputs(
        &mut self,
        outputs: &[nut00::BlindedMessage],
        rng: &mut dyn RngCore,
    ) -> Result<Vec<nut00::BlindSignature>, CashuError> {
        let mut signatures = Vec::with_capacity(outputs.len());
        for output in outputs {
            let privkey = self
                .privkeys
                .get(&output.amount)
                .ok_or(CashuError::KeysetNotFound)?;
            let c = sign_message(privkey, &output.b);
            let dleq = dleq_prove(privkey, &output.b, &c, rng);
            let signature = nut00::BlindSignature {
                amount: output.amount,
                id: self.keys.id.clone(),
                c,
                dleq,
            };
            self.signed.insert(output.b.to_bytes(), signature.clone());
            signatures.push(signature);
        }
        Ok(signatures)
    }

    fn check_inputs(&mut self, inputs: &[nut00::Proof]) -> Result<(), CashuError> {
        for proof in inputs {
            let privkey = self
                .privkeys
                .get(&proof.amount)
                .ok_or(CashuError::KeysetNotFound)?;
            let y = hash_to_curve(proof.secret.as_bytes()).map_err(|_| CashuError::InvalidProof)?;
            if self.spent_ys.contains(&y.to_bytes()) {
                return Err(CashuError::TokensAlreadySpent);
            }
            let valid = verify_signature_with_privkey(proof.secret.as_bytes(), &proof.c, privkey)
                .map_err(|_| CashuError::InvalidProof)?;
            if !valid {
                return Err(CashuError::InvalidProof);
            }
        }
        for proof in inputs {
            let y = hash_to_curve(proof.secret.as_bytes()).map_err(|_| CashuError::InvalidProof)?;
            self.spent_ys.insert(y.to_bytes());
        }
        Ok(())
    }

    fn fresh_quote_id(&mut self) -> String {
        let id = format!("browser-{:04x}", self.next_quote);
        self.next_quote += 1;
        id
    }
}

impl MintClient for DemoMintClient {
    fn get_info(&mut self) -> Result<nut06::MintInfo, CashuError> {
        Ok(nut06::MintInfo {
            name: String::from(DEMO_MINT_NAME),
            pubkey: hex::encode(self.inner.borrow().mint_pubkey.to_bytes()),
            version: String::from("micronuts-wallet/browser-demo"),
            description: String::from("In-browser demo mint (no network, no value)"),
            contact: Vec::new(),
            nuts: vec![(String::from("4"), Default::default())],
        })
    }

    fn get_keys(&mut self) -> Result<nut01::KeysResponse, CashuError> {
        Ok(nut01::KeysResponse {
            keysets: vec![self.inner.borrow().keys.clone()],
        })
    }

    fn get_keysets(&mut self) -> Result<nut02::KeysetsResponse, CashuError> {
        Ok(nut02::KeysetsResponse {
            keysets: vec![nut02::KeysetInfo {
                id: self.inner.borrow().keys.id.clone(),
                unit: String::from("sat"),
                active: true,
                input_fee_ppk: 0,
            }],
        })
    }

    fn post_mint_quote(
        &mut self,
        request: nut04::MintQuoteRequest,
    ) -> Result<nut04::MintQuoteResponse, CashuError> {
        let quote = {
            let mut inner = self.inner.borrow_mut();
            let quote = nut04::MintQuoteResponse {
                quote: inner.fresh_quote_id(),
                request: format!("lnbcdemo{}sat1browser-demo", request.amount),
                paid: true,
                state: String::from("PAID"),
                expiry: EXPIRY_FAR_FUTURE,
                amount: request.amount,
                unit: request.unit,
                amount_paid: request.amount,
                amount_issued: 0,
                updated_at: 0,
                method: String::from("bolt11"),
                pubkey: None,
            };
            inner.quotes.insert(quote.quote.clone(), quote.clone());
            quote
        };
        Ok(quote)
    }

    fn get_mint_quote(&mut self, quote_id: &str) -> Result<nut04::MintQuoteResponse, CashuError> {
        self.inner
            .borrow()
            .quotes
            .get(quote_id)
            .cloned()
            .ok_or(CashuError::QuoteNotFound)
    }

    fn post_mint(
        &mut self,
        request: nut04::MintRequest,
    ) -> Result<nut04::MintResponse, CashuError> {
        let mut inner = self.inner.borrow_mut();
        let Some(quote) = inner.quotes.get(&request.quote) else {
            return Err(CashuError::QuoteNotFound);
        };
        if quote.state != "PAID" {
            return Err(CashuError::QuoteNotPaid);
        }
        let outputs_total: u64 = request.outputs.iter().map(|o| o.amount).sum();
        if outputs_total > quote.amount {
            return Err(CashuError::AmountMismatch);
        }
        let signatures = inner.sign_outputs(&request.outputs, &mut rand_core::OsRng)?;
        Ok(nut04::MintResponse { signatures })
    }

    fn post_melt_quote(
        &mut self,
        request: nut05::MeltQuoteRequest,
    ) -> Result<nut05::MeltQuoteResponse, CashuError> {
        let amount = request
            .request
            .strip_prefix("lnbcdemo")
            .and_then(|rest| rest.split("sat").next())
            .and_then(|digits| digits.parse::<u64>().ok())
            .unwrap_or(1);
        let mut inner = self.inner.borrow_mut();
        let quote = nut05::MeltQuoteResponse {
            quote: inner.fresh_quote_id(),
            amount,
            fee_reserve: 0,
            paid: false,
            state: String::from("UNPAID"),
            expiry: EXPIRY_FAR_FUTURE,
            request: request.request,
            unit: request.unit,
            method: String::from("bolt11"),
        };
        inner.melt_quotes.insert(quote.quote.clone(), quote.clone());
        Ok(quote)
    }

    fn get_melt_quote(&mut self, quote_id: &str) -> Result<nut05::MeltQuoteResponse, CashuError> {
        self.inner
            .borrow()
            .melt_quotes
            .get(quote_id)
            .cloned()
            .ok_or(CashuError::QuoteNotFound)
    }

    fn post_melt(
        &mut self,
        request: nut05::MeltRequest,
    ) -> Result<nut05::MeltResponse, CashuError> {
        let mut inner = self.inner.borrow_mut();
        if !inner.melt_quotes.contains_key(&request.quote) {
            return Err(CashuError::QuoteNotFound);
        }
        inner.check_inputs(&request.inputs)?;
        Ok(nut05::MeltResponse {
            paid: true,
            state: String::from("PAID"),
            payment_preimage: Some(format!("{:064x}", request.quote.len())),
            change: None,
            quote: request.quote,
            amount: 0,
            fee_reserve: 0,
            unit: String::from("sat"),
            expiry: EXPIRY_FAR_FUTURE,
            request: String::new(),
            method: String::from("bolt11"),
        })
    }

    fn post_swap(
        &mut self,
        request: nut03::SwapRequest,
    ) -> Result<nut03::SwapResponse, CashuError> {
        let mut inner = self.inner.borrow_mut();
        inner.check_inputs(&request.inputs)?;
        let input_total: u64 = request.inputs.iter().map(|p| p.amount).sum();
        let output_total: u64 = request.outputs.iter().map(|o| o.amount).sum();
        if output_total > input_total {
            return Err(CashuError::AmountMismatch);
        }
        let signatures = inner.sign_outputs(&request.outputs, &mut rand_core::OsRng)?;
        Ok(nut03::SwapResponse { signatures })
    }

    fn post_check_state(
        &mut self,
        request: nut07::CheckStateRequest,
    ) -> Result<nut07::CheckStateResponse, CashuError> {
        let inner = self.inner.borrow();
        let states = request
            .ys
            .iter()
            .map(|y| nut07::ProofState {
                y: *y,
                state: if inner.spent_ys.contains(&y.to_bytes()) {
                    String::from("SPENT")
                } else {
                    String::from("UNSPENT")
                },
                witness: None,
            })
            .collect();
        Ok(nut07::CheckStateResponse { states })
    }

    fn post_restore(
        &mut self,
        request: nut09::RestoreRequest,
    ) -> Result<nut09::RestoreResponse, CashuError> {
        let inner = self.inner.borrow();
        let outputs = request
            .outputs
            .iter()
            .filter_map(|y| {
                inner
                    .signed
                    .get(&y.to_bytes())
                    .map(|signature| nut09::RestoreOutput {
                        y: *y,
                        signature: signature.clone(),
                    })
            })
            .collect();
        Ok(nut09::RestoreResponse { outputs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{HistoryKind, WalletEngine};
    use cashu_core_lite::store::MemoryStore;
    use rand_core::RngCore;

    #[test]
    fn browser_mint_full_cycle_with_dleq_verification() {
        let mut seed = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut seed);
        let client = DemoMintClient::new(seed);
        let mut engine = WalletEngine::new(
            DEMO_MINT_URL,
            client.clone(),
            MemoryStore::new(),
            [0x31; 32],
            Vec::new(),
        )
        .unwrap();
        engine.connect().unwrap();
        assert_eq!(engine.mint_name(), DEMO_MINT_NAME);
        assert_eq!(engine.fee_ppk(), 0);

        let quote = engine.mint_via_invoice(100).unwrap();
        assert_eq!(quote.state, "PAID");
        let minted = engine.mint_paid_quote(&quote.quote, 100).unwrap();
        assert_eq!(minted, 100, "DLEQ proofs from the demo mint must verify");

        let token = engine.send_token(21, None).unwrap();
        assert_eq!(engine.balance(), 79);
        let received = engine.receive_token(&token).unwrap();
        assert_eq!(received, 21);
        assert_eq!(engine.balance(), 100);

        let (_quote, outcome) = engine.melt("lnbcdemo30sat1demo").unwrap();
        assert!(outcome.paid);
        assert_eq!(engine.balance(), 70);

        let kinds: Vec<HistoryKind> = engine.history().iter().map(|h| h.kind).collect();
        assert_eq!(
            kinds,
            vec![
                HistoryKind::Mint,
                HistoryKind::Send,
                HistoryKind::Receive,
                HistoryKind::Melt
            ]
        );
    }

    #[test]
    fn browser_mint_rejects_double_spend() {
        let mut seed = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut seed);
        let client = DemoMintClient::new(seed);
        let mut wallet = WalletEngine::new(
            DEMO_MINT_URL,
            client.clone(),
            MemoryStore::new(),
            [0x41; 32],
            Vec::new(),
        )
        .unwrap();
        wallet.connect().unwrap();
        let quote = wallet.mint_via_invoice(8).unwrap();
        wallet.mint_paid_quote(&quote.quote, 8).unwrap();

        let token = wallet.send_token(8, None).unwrap();
        assert_eq!(wallet.receive_token(&token).unwrap(), 8);
        assert!(
            wallet.receive_token(&token).is_err(),
            "redeeming the same token twice must fail"
        );
    }
}
