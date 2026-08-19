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

/// Maximum length of a Performance Max search theme, per the Google Ads API.
const MAX_SEARCH_THEME_LEN: usize = 80;

/// Add signals to a Performance Max asset group.
///
/// PMax does NOT take audiences as campaign criteria — `add_audience_targeting`
/// writes a `campaignCriterion`, which the API rejects for a PMax campaign.
/// Audience signals and search themes belong on `asset_group_signal`, and they
/// are what PMax uses to seed targeting before it has conversion history.
///
/// Supply `search_themes`, `audience_ids`, or both. Audience IDs reference
/// `customers/{cid}/audiences/{id}` — an Audience resource, not a user list.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn add_asset_group_signal(
    config: &Config,
    customer_id: &str,
    asset_group_id: &str,
    search_themes: &[String],
    audience_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("add_asset_group_signal", &config.safety)?;

    if asset_group_id.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Asset group ID cannot be empty".to_string(),
        ));
    }

    if search_themes.is_empty() && audience_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one search theme or audience ID is required".to_string(),
        ));
    }

    for theme in search_themes {
        if theme.trim().is_empty() {
            return Err(McpGoogleAdsError::Validation(
                "Search theme cannot be empty".to_string(),
            ));
        }
        if theme.chars().count() > MAX_SEARCH_THEME_LEN {
            return Err(McpGoogleAdsError::Validation(format!(
                "Search theme '{}' is {} chars, exceeds the {} char limit",
                theme,
                theme.chars().count(),
                MAX_SEARCH_THEME_LEN
            )));
        }
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let asset_group_resource = format!("customers/{}/assetGroups/{}", cid, asset_group_id);

    let mut operations: Vec<serde_json::Value> = Vec::new();

    for theme in search_themes {
        operations.push(json!({
            "assetGroupSignalOperation": {
                "create": {
                    "assetGroup": asset_group_resource,
                    "searchTheme": { "text": theme }
                }
            }
        }));
    }

    for audience_id in audience_ids {
        operations.push(json!({
            "assetGroupSignalOperation": {
                "create": {
                    "assetGroup": asset_group_resource,
                    "audience": {
                        "audience": format!("customers/{}/audiences/{}", cid, audience_id)
                    }
                }
            }
        }));
    }

    let changes = json!({
        "asset_group_id": asset_group_id,
        "search_themes": search_themes,
        "audience_ids": audience_ids,
        "signal_count": operations.len()
    });

    let plan = ChangePlan::new(
        "add_asset_group_signal".to_string(),
        "asset_group_signal".to_string(),
        asset_group_id.to_string(),
        cid,
        changes,
        false,
        operations,
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

    #[test]
    fn test_add_asset_group_signal_search_themes() {
        let config = Config::default();
        let themes = vec!["web design sydney".to_string(), "online shops".to_string()];
        let result = add_asset_group_signal(&config, "123-456-7890", "6738426770", &themes, &[]);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "add_asset_group_signal");
        assert_eq!(preview["changes"]["signal_count"], 2);
    }

    #[test]
    fn test_add_asset_group_signal_combines_themes_and_audiences() {
        let config = Config::default();
        let themes = vec!["web design sydney".to_string()];
        let audiences = vec!["111".to_string(), "222".to_string()];
        let result =
            add_asset_group_signal(&config, "123-456-7890", "6738426770", &themes, &audiences);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["changes"]["signal_count"], 3);
    }

    #[test]
    fn test_add_asset_group_signal_requires_at_least_one_signal() {
        let config = Config::default();
        let result = add_asset_group_signal(&config, "123-456-7890", "6738426770", &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_asset_group_signal_rejects_overlong_theme() {
        let config = Config::default();
        let themes = vec!["a".repeat(MAX_SEARCH_THEME_LEN + 1)];
        let result = add_asset_group_signal(&config, "123-456-7890", "123", &themes, &[]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn test_add_asset_group_signal_rejects_empty_theme() {
        let config = Config::default();
        let themes = vec!["   ".to_string()];
        let result = add_asset_group_signal(&config, "123-456-7890", "123", &themes, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_asset_group_signal_empty_asset_group_id() {
        let config = Config::default();
        let themes = vec!["web design".to_string()];
        let result = add_asset_group_signal(&config, "123-456-7890", "", &themes, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_add_asset_group_signal_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["add_asset_group_signal".to_string()];
        let themes = vec!["web design".to_string()];
        let result = add_asset_group_signal(&config, "123-456-7890", "123", &themes, &[]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
