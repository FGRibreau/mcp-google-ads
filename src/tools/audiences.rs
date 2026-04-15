use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

const VALID_AUDIENCE_TYPES: &[&str] = &["WEBSITE_VISITORS", "CUSTOMER_MATCH"];
const VALID_TARGETING_MODES: &[&str] = &["TARGETING", "OBSERVATION"];

/// Create a custom audience.
///
/// For WEBSITE_VISITORS: `urls_or_rules` are URL-contains patterns for the remarketing list.
/// For CUSTOMER_MATCH: `urls_or_rules` describe the matching rules.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn create_custom_audience(
    config: &Config,
    customer_id: &str,
    audience_name: &str,
    audience_type: &str,
    urls_or_rules: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_custom_audience", &config.safety)?;

    if !VALID_AUDIENCE_TYPES.contains(&audience_type) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid audience type '{}'. Must be one of: {}",
            audience_type,
            VALID_AUDIENCE_TYPES.join(", ")
        )));
    }

    if audience_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Audience name cannot be empty".to_string(),
        ));
    }

    if urls_or_rules.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one URL pattern or rule is required".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let members: Vec<serde_json::Value> = urls_or_rules
        .iter()
        .map(|rule| {
            json!({
                "keyword": {
                    "value": rule
                }
            })
        })
        .collect();

    let operation = json!({
        "customAudienceOperation": {
            "create": {
                "name": audience_name,
                "type": audience_type,
                "members": members
            }
        }
    });

    let changes = json!({
        "audience_name": audience_name,
        "audience_type": audience_type,
        "rules_count": urls_or_rules.len(),
        "rules": urls_or_rules
    });

    let plan = ChangePlan::new(
        "create_custom_audience".to_string(),
        "custom_audience".to_string(),
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

/// Add audience targeting to a campaign.
///
/// `targeting_mode`: "TARGETING" limits to the audience, "OBSERVATION" monitors without limiting.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn add_audience_targeting(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    audience_id: &str,
    targeting_mode: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("add_audience_targeting", &config.safety)?;

    if !VALID_TARGETING_MODES.contains(&targeting_mode) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid targeting mode '{}'. Must be one of: {}",
            targeting_mode,
            VALID_TARGETING_MODES.join(", ")
        )));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let bid_modifier = match targeting_mode {
        "OBSERVATION" => Some("bidModifier"),
        _ => None,
    };

    let mut criterion = json!({
        "campaign": campaign_resource,
        "userList": {
            "userList": format!("customers/{}/userLists/{}", cid, audience_id)
        }
    });

    if bid_modifier.is_some() {
        criterion
            .as_object_mut()
            .map(|o| o.insert("bidModifier".to_string(), json!(1.0)));
    }

    let operation = json!({
        "campaignCriterionOperation": {
            "create": criterion
        }
    });

    let changes = json!({
        "campaign_id": campaign_id,
        "audience_id": audience_id,
        "targeting_mode": targeting_mode
    });

    let plan = ChangePlan::new(
        "add_audience_targeting".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
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
    fn test_create_custom_audience_success() {
        let config = Config::default();
        let result = create_custom_audience(
            &config,
            "123-456-7890",
            "My Audience",
            "WEBSITE_VISITORS",
            vec!["example.com/products".to_string()],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_custom_audience");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_create_custom_audience_invalid_type() {
        let config = Config::default();
        let result = create_custom_audience(
            &config,
            "123-456-7890",
            "My Audience",
            "INVALID_TYPE",
            vec!["rule".to_string()],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("Invalid audience type"));
    }

    #[test]
    fn test_create_custom_audience_empty_name() {
        let config = Config::default();
        let result = create_custom_audience(
            &config,
            "123-456-7890",
            "",
            "WEBSITE_VISITORS",
            vec!["rule".to_string()],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_create_custom_audience_no_rules() {
        let config = Config::default();
        let result = create_custom_audience(
            &config,
            "123-456-7890",
            "My Audience",
            "WEBSITE_VISITORS",
            vec![],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_add_audience_targeting_success() {
        let config = Config::default();
        let result = add_audience_targeting(&config, "123-456-7890", "555", "999", "TARGETING");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "add_audience_targeting");
    }

    #[test]
    fn test_add_audience_targeting_observation() {
        let config = Config::default();
        let result = add_audience_targeting(&config, "123-456-7890", "555", "999", "OBSERVATION");
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_audience_targeting_invalid_mode() {
        let config = Config::default();
        let result = add_audience_targeting(&config, "123-456-7890", "555", "999", "INVALID");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_custom_audience_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_custom_audience".to_string()];
        let result = create_custom_audience(
            &config,
            "123-456-7890",
            "My Audience",
            "WEBSITE_VISITORS",
            vec!["rule".to_string()],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_add_audience_targeting_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["add_audience_targeting".to_string()];
        let result = add_audience_targeting(&config, "123-456-7890", "555", "999", "TARGETING");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
