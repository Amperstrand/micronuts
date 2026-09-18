//! Universal scan payload classification — the single place a decoded
//! string becomes a wallet intent (UX contract: scanning may resolve an
//! intent and populate a review state; it never authorizes spending).
//!
//! Recognized: Cashu tokens (bare `cashuB…`/`cashuA…` and `?token=`
//! URLs), BOLT11 invoices (bare and `lightning:`-prefixed), mint URLs.
//! Everything else is Unknown and must not trigger money machinery.

/// What a scanned (or pasted) string resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedPayload {
    /// A Cashu token, ready for receive inspection.
    CashuToken { token: String },
    /// A BOLT11 Lightning invoice — a pay intent, routed to review.
    LightningInvoice { invoice: String },
    /// An http(s) URL that is not carrying a token — a mint-add candidate.
    MintUrl { url: String },
    /// Anything else. Never auto-spends, never errors hard.
    Unknown,
}

/// Classify a decoded payload. Tolerant of whitespace and URI wrappers.
pub fn classify(text: &str) -> ScannedPayload {
    let trimmed = text.trim();

    // lightning:URI wrapper → invoice.
    let unwrapped = trimmed
        .strip_prefix("lightning:")
        .or_else(|| trimmed.strip_prefix("LIGHTNING:"))
        .unwrap_or(trimmed)
        .trim();

    if let Some(token) = extract_token(trimmed) {
        return ScannedPayload::CashuToken { token };
    }
    if unwrapped.starts_with("lnb") {
        return ScannedPayload::LightningInvoice {
            invoice: unwrapped.to_string(),
        };
    }
    if unwrapped.starts_with("http://") || unwrapped.starts_with("https://") {
        return ScannedPayload::MintUrl {
            url: unwrapped.to_string(),
        };
    }
    ScannedPayload::Unknown
}

/// Tokens are `cashuB…`/`cashuA…` (case-insensitive prefix) either bare
/// or as the `token=` query parameter of a URL (cashu.me-style links).
fn extract_token(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("cashub") || lower.starts_with("cashua") {
        return Some(text.to_string());
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let query = text.split('?').nth(1)?;
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;
            if key.eq_ignore_ascii_case("token") {
                let token = value.trim();
                let token_lower = token.to_ascii_lowercase();
                if token_lower.starts_with("cashub") || token_lower.starts_with("cashua") {
                    return Some(token.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_v4_token() {
        assert_eq!(
            classify("cashuBpGFtcGRvaHR0cDovLzEyNy4wLjAuMTozMDMw"),
            ScannedPayload::CashuToken {
                token: String::from("cashuBpGFtcGRvaHR0cDovLzEyNy4wLjAuMTozMDMw")
            }
        );
    }

    #[test]
    fn token_prefix_is_case_insensitive() {
        assert!(matches!(
            classify("CASHUBabc"),
            ScannedPayload::CashuToken { .. }
        ));
    }

    #[test]
    fn token_in_url_query() {
        let scanned = "https://mint.example/send?token=cashuBpGFtcDEyMw&x=1";
        assert_eq!(
            classify(scanned),
            ScannedPayload::CashuToken {
                token: String::from("cashuBpGFtcDEyMw")
            }
        );
    }

    #[test]
    fn url_without_token_is_mint_url() {
        assert_eq!(
            classify("https://testnut.cashu.space/"),
            ScannedPayload::MintUrl {
                url: String::from("https://testnut.cashu.space/")
            }
        );
    }

    #[test]
    fn bolt11_bare_and_wrapped() {
        assert!(matches!(
            classify("lnbcrt10u1psl9wme"),
            ScannedPayload::LightningInvoice { .. }
        ));
        assert!(matches!(
            classify("lightning:lnbcrt10u1psl9wme"),
            ScannedPayload::LightningInvoice { invoice }
            if invoice == "lnbcrt10u1psl9wme"
        ));
    }

    #[test]
    fn whitespace_is_tolerated() {
        assert!(matches!(
            classify("  \n cashuBpGFtcDEyMw \n"),
            ScannedPayload::CashuToken { .. }
        ));
    }

    #[test]
    fn garbage_is_unknown_and_inert() {
        assert_eq!(classify("hello world"), ScannedPayload::Unknown);
        assert_eq!(classify(""), ScannedPayload::Unknown);
        // A URL whose token param isn't Cashu is still a URL.
        assert!(matches!(
            classify("https://x.y?token=notacashu"),
            ScannedPayload::MintUrl { .. }
        ));
    }
}
