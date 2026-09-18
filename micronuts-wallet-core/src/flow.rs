//! Semantic wallet flow states — the UX contract as code.
//!
//! Pure state machines + user copy: no I/O, no Slint, fully testable on
//! the host. The engine drives transitions (its results map onto phases);
//! the UI renders `user_line()` strings. Money states that matter
//! (`PaidNotIssued`, `AlreadySpent`, `ForeignMint`) are modeled here
//! instead of living inside ad-hoc status strings.
//!
//! CDK alignment: quote-state polling maps onto phases the way CDK's
//! `check_mint_quote_status` drives its transaction recording; proof
//! Unspent/Pending/Spent surface as inspection counts feeding Review.

use cashu_core_lite::error::CashuError;

/// Failure taxonomy from the UX contract. Every variant's `user_line`
/// says what happened, whether money is safe, and the next action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowFailure {
    /// Retry sensible; money has not moved.
    Recoverable(String),
    /// Mint unreachable — money safe, try again later.
    Offline,
    /// Malformed token / payload. Terminal for this input.
    InvalidToken,
    /// The ecash was already redeemed. Terminal.
    AlreadySpent,
    /// Token belongs to a mint the wallet is not connected to.
    ForeignMint { mint: String },
    /// Balance too low for the requested operation.
    InsufficientFunds,
    /// Invoice or quote expired. Terminal for this attempt.
    Expired,
    /// A NUT-12/DLEQ check failed — the mint's answer did not prove out.
    /// Terminal for this operation; nothing unverified was credited.
    UntrustedSignature,
}

impl FlowFailure {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            FlowFailure::InvalidToken
                | FlowFailure::AlreadySpent
                | FlowFailure::ForeignMint { .. }
                | FlowFailure::Expired
                | FlowFailure::UntrustedSignature
        )
    }

    /// User-facing line per the contract: what happened, money-safety,
    /// next action. Never exposes protocol jargon.
    pub fn user_line(&self) -> String {
        match self {
            FlowFailure::Recoverable(detail) => {
                format!("That didn't complete. Your ecash is safe. Try again. ({detail})")
            }
            FlowFailure::Offline => String::from(
                "Can't reach the mint. Your ecash is safe — check the connection and try again.",
            ),
            FlowFailure::InvalidToken => {
                String::from("That doesn't look like valid ecash. Nothing was received.")
            }
            FlowFailure::AlreadySpent => {
                String::from("This ecash was already received by someone. Nothing was received.")
            }
            FlowFailure::ForeignMint { mint } => format!(
                "This ecash is from a different mint ({mint}). Add that mint to receive it."
            ),
            FlowFailure::InsufficientFunds => String::from("Not enough ecash for this payment."),
            FlowFailure::Expired => {
                String::from("This invoice has expired. Create a new one to try again.")
            }
            FlowFailure::UntrustedSignature => String::from(
                "The mint's answer failed verification. Nothing was received. Try again.",
            ),
        }
    }
}

/// Map engine/mint errors onto the taxonomy. Context-free errors keep
/// their detail; offline is the transport class.
pub fn classify(err: &CashuError) -> FlowFailure {
    match err {
        CashuError::Transport(_) => FlowFailure::Offline,
        CashuError::InsufficientInputs => FlowFailure::InsufficientFunds,
        CashuError::QuoteNotPaid => {
            FlowFailure::Recoverable(String::from("payment not detected yet"))
        }
        CashuError::QuoteNotFound => {
            FlowFailure::Recoverable(String::from("quote unknown to the mint"))
        }
        CashuError::Crypto(detail) if detail.contains("DLEQ") => FlowFailure::UntrustedSignature,
        CashuError::Protocol(detail) if detail.contains("invalid token") => {
            FlowFailure::InvalidToken
        }
        CashuError::Protocol(detail) if detail.contains("different mint") => {
            let mint = detail
                .split("different mint:")
                .nth(1)
                .unwrap_or("")
                .split(" (")
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            FlowFailure::ForeignMint { mint }
        }
        other => FlowFailure::Recoverable(format!("{other:?}")),
    }
}

/// What a scanned/pasted token contains, for the Review step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSummary {
    pub mint: String,
    pub unit: String,
    pub memo: Option<String>,
    pub amount: u64,
    pub proof_count: usize,
}

/// Inspect result: the summary plus per-proof health from the mint
/// (NUT-07) and the prospective receive fee (swap input fee). `spent`/
/// `pending` counts feed warnings and the AlreadySpent verdict without
/// redeeming anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInspection {
    pub summary: TokenSummary,
    pub spent: usize,
    pub pending: usize,
    /// Exact swap fee the receive would pay (fee_ppk × inputs / 1000).
    pub fee: u64,
}

impl TokenInspection {
    /// True when every proof is already spent — receiving is pointless.
    pub fn all_spent(&self) -> bool {
        self.spent == self.summary.proof_count
    }

    pub fn user_line(&self) -> String {
        let amount = crate::format_amount(self.summary.amount);
        let mut line = format!("{amount} from {}", self.summary.mint);
        if let Some(memo) = &self.summary.memo {
            line.push_str(&format!(" — “{memo}”"));
        }
        if self.spent > 0 {
            line.push_str(&format!(" ({} part already received)", self.spent));
        }
        line
    }

    /// Prospective fee wording per the copy contract: "No fee" at zero,
    /// numeric otherwise.
    pub fn fee_line(&self) -> String {
        if self.fee == 0 {
            String::from("No fee")
        } else {
            format!("Fee: {}", crate::format_amount(self.fee))
        }
    }
}

/// Receive ecash: scan/paste → inspect → review → receiving → received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveEcashPhase {
    Input,
    Inspecting,
    Review(TokenInspection),
    Receiving,
    Received { amount: u64 },
    Failed(FlowFailure),
}

impl ReceiveEcashPhase {
    pub fn user_line(&self) -> String {
        match self {
            ReceiveEcashPhase::Input => String::new(),
            ReceiveEcashPhase::Inspecting => String::from("Checking this ecash…"),
            ReceiveEcashPhase::Review(inspection) => inspection.user_line(),
            ReceiveEcashPhase::Receiving => String::from("Receiving ecash…"),
            ReceiveEcashPhase::Received { amount } => {
                format!("Received {}.", crate::format_amount(*amount))
            }
            ReceiveEcashPhase::Failed(failure) => failure.user_line(),
        }
    }
}

/// Receive via Lightning: invoice → waiting → paid → issuing → received.
/// `PaidNotIssued` is the money-safety state: the invoice settled but
/// ecash issuance has not completed — retryable, never a plain failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveLightningPhase {
    Input,
    Invoice {
        amount: u64,
    },
    Waiting {
        amount: u64,
    },
    /// Invoice settled; ecash issuance pending (manual step today,
    /// automatic in a later milestone — the state is modeled now).
    PaidNotIssued {
        amount: u64,
    },
    Issuing {
        amount: u64,
    },
    Received {
        amount: u64,
    },
    Expired,
    Failed(FlowFailure),
}

impl ReceiveLightningPhase {
    /// NUT-04 quote state → phase (mint strings: UNPAID/PENDING/PAID/ISSUED).
    pub fn from_quote_state(state: &str, amount: u64) -> Self {
        match state {
            "PAID" => ReceiveLightningPhase::PaidNotIssued { amount },
            "ISSUED" => ReceiveLightningPhase::Received { amount },
            _ => ReceiveLightningPhase::Waiting { amount },
        }
    }

    pub fn user_line(&self) -> String {
        match self {
            ReceiveLightningPhase::Input => String::new(),
            ReceiveLightningPhase::Invoice { amount } => {
                format!("Invoice for {}.", crate::format_amount(*amount))
            }
            ReceiveLightningPhase::Waiting { .. } => String::from("Waiting for payment…"),
            ReceiveLightningPhase::PaidNotIssued { amount } => format!(
                "Payment received — finishing… ({})",
                crate::format_amount(*amount)
            ),
            ReceiveLightningPhase::Issuing { amount } => {
                format!("Adding {} to your balance…", crate::format_amount(*amount))
            }
            ReceiveLightningPhase::Received { amount } => {
                format!("Received {}.", crate::format_amount(*amount))
            }
            ReceiveLightningPhase::Expired => String::from("This invoice has expired."),
            ReceiveLightningPhase::Failed(failure) => failure.user_line(),
        }
    }
}

/// Pay Lightning: invoice → resolving → review → paying → paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayLightningPhase {
    Input,
    Resolving,
    Review { amount: u64, fee_reserve: u64 },
    Paying,
    Paid,
    Failed,
}

impl PayLightningPhase {
    pub fn user_line(&self) -> String {
        match self {
            PayLightningPhase::Input => String::new(),
            PayLightningPhase::Resolving => String::from("Reading invoice…"),
            PayLightningPhase::Review {
                amount,
                fee_reserve,
            } => {
                if *fee_reserve == 0 {
                    format!("Pay {} — no fee.", crate::format_amount(*amount))
                } else {
                    format!(
                        "Pay {} — fee up to {}.",
                        crate::format_amount(*amount),
                        crate::format_amount(*fee_reserve)
                    )
                }
            }
            PayLightningPhase::Paying => String::from("Paying…"),
            PayLightningPhase::Paid => String::from("Payment sent."),
            PayLightningPhase::Failed => {
                String::from("Payment didn't complete. Your ecash is safe.")
            }
        }
    }
}

/// Send ecash: amount → review → prepare → ready (QR) → awaiting claim.
/// `AwaitingClaim` and the reclaim path are Milestone-4 domain work; the
/// state exists so history and UI never treat "token generated" as
/// "payment complete".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendEcashPhase {
    Amount,
    Review { amount: u64, fee: u64 },
    Preparing,
    ReadyForTransfer,
    AwaitingClaim,
    Failed,
}

impl SendEcashPhase {
    pub fn user_line(&self) -> String {
        match self {
            SendEcashPhase::Amount => String::new(),
            SendEcashPhase::Review { amount, fee } => {
                if *fee == 0 {
                    format!("Send {} — no fee.", crate::format_amount(*amount))
                } else {
                    format!(
                        "Send {} — fee up to {}.",
                        crate::format_amount(*amount),
                        crate::format_amount(*fee)
                    )
                }
            }
            SendEcashPhase::Preparing => String::from("Preparing ecash…"),
            SendEcashPhase::ReadyForTransfer => String::from("Ready — show this QR to send it."),
            SendEcashPhase::AwaitingClaim => String::from("Waiting for recipient…"),
            SendEcashPhase::Failed => {
                String::from("Couldn't prepare this send. Your ecash is safe.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_classifies_offline() {
        assert_eq!(
            classify(&CashuError::Transport(String::from("conn refused"))),
            FlowFailure::Offline
        );
    }

    #[test]
    fn insufficient_inputs_classifies_insufficient_funds() {
        assert_eq!(
            classify(&CashuError::InsufficientInputs),
            FlowFailure::InsufficientFunds
        );
    }

    #[test]
    fn foreign_mint_error_classifies_with_mint_name() {
        let err = CashuError::Protocol(String::from(
            "token is from a different mint: https://other.mint (active: https://this.mint)",
        ));
        assert_eq!(
            classify(&err),
            FlowFailure::ForeignMint {
                mint: String::from("https://other.mint")
            }
        );
    }

    #[test]
    fn invalid_token_classifies_terminal() {
        let err = CashuError::Protocol(String::from("invalid token: truncated"));
        assert!(matches!(classify(&err), FlowFailure::InvalidToken));
        assert!(classify(&err).is_terminal());
    }

    #[test]
    fn dleq_failure_classifies_untrusted() {
        let err = CashuError::Crypto(String::from("NUT-12 DLEQ verification failed"));
        assert!(matches!(classify(&err), FlowFailure::UntrustedSignature));
    }

    #[test]
    fn quote_states_map_to_phases() {
        use ReceiveLightningPhase as P;
        assert_eq!(P::from_quote_state("UNPAID", 21), P::Waiting { amount: 21 });
        assert_eq!(
            P::from_quote_state("PENDING", 21),
            P::Waiting { amount: 21 }
        );
        assert_eq!(
            P::from_quote_state("PAID", 21),
            P::PaidNotIssued { amount: 21 }
        );
        assert_eq!(
            P::from_quote_state("ISSUED", 21),
            P::Received { amount: 21 }
        );
    }

    #[test]
    fn paid_not_issued_is_not_a_plain_failure() {
        // The money-safety state must be distinguishable from Failed.
        let phase = ReceiveLightningPhase::PaidNotIssued { amount: 21 };
        assert!(!phase.user_line().to_lowercase().contains("fail"));
        assert!(phase.user_line().contains("Payment received"));
    }

    #[test]
    fn user_lines_never_expose_protocol_jargon() {
        let failures = [
            FlowFailure::Recoverable(String::from("x")),
            FlowFailure::Offline,
            FlowFailure::InvalidToken,
            FlowFailure::AlreadySpent,
            FlowFailure::ForeignMint {
                mint: String::from("m"),
            },
            FlowFailure::InsufficientFunds,
            FlowFailure::Expired,
            FlowFailure::UntrustedSignature,
        ];
        for f in &failures {
            let line = f.user_line().to_lowercase();
            for jargon in ["nut-", "proof", "quote", "dleq", "blind", "swap"] {
                assert!(!line.contains(jargon), "{line} contains {jargon}");
            }
        }
    }

    #[test]
    fn zero_fee_review_says_no_fee_not_zero() {
        // cashubtc/wallet copy contract: prospective zero fee = "No fee".
        let line = PayLightningPhase::Review {
            amount: 21,
            fee_reserve: 0,
        }
        .user_line();
        assert!(line.contains("no fee"), "{line}");
        assert!(!line.contains("0 sat"), "{line}");
    }

    #[test]
    fn inspection_all_spent_verdict() {
        let inspection = TokenInspection {
            summary: TokenSummary {
                mint: String::from("m"),
                unit: String::from("sat"),
                memo: None,
                amount: 21,
                proof_count: 3,
            },
            spent: 3,
            pending: 0,
            fee: 0,
        };
        assert!(inspection.all_spent());
        assert!(inspection.user_line().contains("already received"));
    }
}
