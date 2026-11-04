//! Conversion of account-currency amounts to the micros the Google Ads API takes.

use crate::error::{McpGoogleAdsError, Result};

/// Convert an amount in units of the account currency (e.g. `2.01`) to micros (`2_010_000`).
///
/// Rounds to the nearest micro: a plain `as i64` cast truncates the binary
/// floating-point product (`2.01 * 1e6` is `2009999.9999999998`), which yields an
/// amount Google rejects for not being a multiple of the currency's billable unit.
pub fn to_micros(amount: f64) -> i64 {
    (amount * 1_000_000.0).round() as i64
}

/// The micros of an amount given either in currency units (`amount`) or in micros (`micros`).
///
/// Both are accepted so that callers keep working while tools move to currency
/// amounts; giving both is ambiguous and rejected. `field` names the currency
/// parameter in the error.
pub fn resolve_micros(
    amount: Option<f64>,
    micros: Option<i64>,
    field: &str,
) -> Result<Option<i64>> {
    match (amount, micros) {
        (Some(_), Some(_)) => Err(McpGoogleAdsError::Validation(format!(
            "Pass either {field} (in account currency) or {field}_micros, not both"
        ))),
        (Some(value), None) => Ok(Some(to_micros(value))),
        (None, micros) => Ok(micros),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_instead_of_truncating() {
        assert_eq!(to_micros(2.01), 2_010_000);
        assert_eq!(to_micros(0.29), 290_000);
        assert_eq!(to_micros(50.0), 50_000_000);
        assert_eq!(to_micros(0.5), 500_000);
    }

    #[test]
    fn every_cent_amount_up_to_100k_is_exact() {
        for cents in 1..10_000_000_i64 {
            assert_eq!(
                to_micros(cents as f64 / 100.0),
                cents * 10_000,
                "{cents} cents"
            );
        }
    }

    #[test]
    fn a_currency_amount_or_micros_but_not_both() {
        assert_eq!(
            resolve_micros(Some(1.5), None, "cpc_bid").unwrap(),
            Some(1_500_000)
        );
        assert_eq!(
            resolve_micros(None, Some(2_000_000), "cpc_bid").unwrap(),
            Some(2_000_000)
        );
        assert_eq!(resolve_micros(None, None, "cpc_bid").unwrap(), None);
        assert!(resolve_micros(Some(1.5), Some(2_000_000), "cpc_bid").is_err());
    }
}
