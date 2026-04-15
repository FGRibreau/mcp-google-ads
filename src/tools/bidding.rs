use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::{check_bid_increase, check_blocked_operation};
use crate::safety::preview::{store_plan, ChangePlan};

/// Convert a dollar amount to micros (Google Ads uses micros: $1 = 1_000_000).
fn dollars_to_micros(dollars: f64) -> i64 {
    (dollars * 1_000_000.0) as i64
}

const VALID_STRATEGY_TYPES: &[&str] = &["TARGET_CPA", "TARGET_ROAS", "TARGET_IMPRESSION_SHARE"];

/// Create a portfolio bidding strategy.
///
/// Portfolio strategies can be shared across multiple campaigns.
///
/// - TARGET_CPA requires `target_cpa`
/// - TARGET_ROAS requires `target_roas`
/// - TARGET_IMPRESSION_SHARE uses defaults
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn create_portfolio_bidding_strategy(
    config: &Config,
    customer_id: &str,
    name: &str,
    strategy_type: &str,
    target_cpa: Option<f64>,
    target_roas: Option<f64>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_portfolio_bidding_strategy", &config.safety)?;

    if !VALID_STRATEGY_TYPES.contains(&strategy_type) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid strategy type '{}'. Must be one of: {}",
            strategy_type,
            VALID_STRATEGY_TYPES.join(", ")
        )));
    }

    if name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Strategy name cannot be empty".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let mut strategy = json!({
        "name": name,
        "type": strategy_type
    });

    match strategy_type {
        "TARGET_CPA" => {
            let cpa = target_cpa.ok_or_else(|| {
                McpGoogleAdsError::Validation(
                    "target_cpa is required for TARGET_CPA strategy".to_string(),
                )
            })?;
            strategy.as_object_mut().map(|o| {
                o.insert(
                    "targetCpa".to_string(),
                    json!({"targetCpaMicros": dollars_to_micros(cpa).to_string()}),
                )
            });
        }
        "TARGET_ROAS" => {
            let roas = target_roas.ok_or_else(|| {
                McpGoogleAdsError::Validation(
                    "target_roas is required for TARGET_ROAS strategy".to_string(),
                )
            })?;
            strategy
                .as_object_mut()
                .map(|o| o.insert("targetRoas".to_string(), json!({"targetRoas": roas})));
        }
        "TARGET_IMPRESSION_SHARE" => {
            strategy.as_object_mut().map(|o| {
                o.insert(
                    "targetImpressionShare".to_string(),
                    json!({
                        "location": "ANYWHERE_ON_PAGE",
                        "locationFractionMicros": "500000"
                    }),
                )
            });
        }
        _ => {}
    }

    let operation = json!({
        "biddingStrategyOperation": {
            "create": strategy
        }
    });

    let changes = json!({
        "name": name,
        "strategy_type": strategy_type,
        "target_cpa": target_cpa,
        "target_roas": target_roas
    });

    let plan = ChangePlan::new(
        "create_portfolio_bidding_strategy".to_string(),
        "bidding_strategy".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Update a keyword's CPC bid.
///
/// Checks the bid increase limit from safety config.
/// `current_bid` is the current bid in dollars (for safety check).
/// `new_bid` is the desired bid in dollars.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn update_keyword_bid(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    criterion_id: &str,
    current_bid: f64,
    new_bid: f64,
) -> Result<serde_json::Value> {
    check_blocked_operation("update_keyword_bid", &config.safety)?;
    check_bid_increase(current_bid, new_bid, &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let resource_name = format!(
        "customers/{}/adGroupCriteria/{}~{}",
        cid, ad_group_id, criterion_id
    );

    let operation = json!({
        "adGroupCriterionOperation": {
            "update": {
                "resourceName": resource_name,
                "cpcBidMicros": dollars_to_micros(new_bid).to_string()
            },
            "updateMask": "cpcBidMicros"
        }
    });

    let changes = json!({
        "ad_group_id": ad_group_id,
        "criterion_id": criterion_id,
        "current_bid": current_bid,
        "new_bid": new_bid,
        "increase_pct": if current_bid > 0.0 {
            format!("{:.0}%", ((new_bid - current_bid) / current_bid) * 100.0)
        } else {
            "N/A".to_string()
        }
    });

    let plan = ChangePlan::new(
        "update_keyword_bid".to_string(),
        "ad_group_criterion".to_string(),
        format!("{}~{}", ad_group_id, criterion_id),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_create_portfolio_target_cpa_success() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "My CPA Strategy",
            "TARGET_CPA",
            Some(5.0),
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_portfolio_bidding_strategy");
    }

    #[test]
    fn test_create_portfolio_target_roas_success() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "My ROAS Strategy",
            "TARGET_ROAS",
            None,
            Some(3.0),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_portfolio_target_impression_share_success() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "Impression Strategy",
            "TARGET_IMPRESSION_SHARE",
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_portfolio_invalid_type() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "Bad Strategy",
            "INVALID",
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_portfolio_empty_name() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "",
            "TARGET_CPA",
            Some(5.0),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_portfolio_target_cpa_missing_value() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "CPA Strategy",
            "TARGET_CPA",
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_portfolio_target_roas_missing_value() {
        let config = Config::default();
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "ROAS Strategy",
            "TARGET_ROAS",
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_update_keyword_bid_success() {
        let config = Config::default();
        let result = update_keyword_bid(&config, "123-456-7890", "111", "222", 1.0, 1.5);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_keyword_bid");
    }

    #[test]
    fn test_update_keyword_bid_exceeds_limit() {
        let config = Config::default(); // max_bid_increase_pct = 100
        let result = update_keyword_bid(&config, "123-456-7890", "111", "222", 1.0, 2.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_keyword_bid_zero_current() {
        let config = Config::default();
        let result = update_keyword_bid(&config, "123-456-7890", "111", "222", 0.0, 5.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_keyword_bid_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["update_keyword_bid".to_string()];
        let result = update_keyword_bid(&config, "123-456-7890", "111", "222", 1.0, 1.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_portfolio_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_portfolio_bidding_strategy".to_string()];
        let result = create_portfolio_bidding_strategy(
            &config,
            "123-456-7890",
            "My Strategy",
            "TARGET_CPA",
            Some(5.0),
            None,
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
