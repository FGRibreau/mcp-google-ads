use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Draft creating a new ad group in an existing campaign.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn create_ad_group(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    ad_group_name: &str,
    cpc_bid_micros: Option<i64>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_ad_group", &config.safety)?;

    if ad_group_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "ad_group_name must not be empty".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let mut create = json!({
        "campaign": campaign_resource,
        "name": ad_group_name,
        "status": "ENABLED",
        "type": "SEARCH_STANDARD"
    });

    if let Some(bid) = cpc_bid_micros {
        create["cpcBidMicros"] = json!(bid.to_string());
    }

    let operation = json!({
        "adGroupOperation": {
            "create": create
        }
    });

    let changes = json!({
        "campaign_id": campaign_id,
        "ad_group_name": ad_group_name,
        "cpc_bid_micros": cpc_bid_micros,
    });

    let plan = ChangePlan::new(
        "create_ad_group".to_string(),
        "ad_group".to_string(),
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

/// Draft updating an existing ad group (name and/or CPC bid).
///
/// At least one of `name` or `cpc_bid_micros` must be provided.
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn update_ad_group(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    name: Option<&str>,
    cpc_bid_micros: Option<i64>,
) -> Result<serde_json::Value> {
    check_blocked_operation("update_ad_group", &config.safety)?;

    if name.is_none() && cpc_bid_micros.is_none() {
        return Err(McpGoogleAdsError::Validation(
            "At least one of name or cpc_bid_micros must be provided".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let resource = format!("customers/{}/adGroups/{}", cid, ad_group_id);

    let mut update_mask_fields = Vec::new();
    let mut ad_group = json!({"resourceName": resource});

    if let Some(n) = name {
        ad_group["name"] = json!(n);
        update_mask_fields.push("name");
    }
    if let Some(bid) = cpc_bid_micros {
        ad_group["cpcBidMicros"] = json!(bid.to_string());
        update_mask_fields.push("cpcBidMicros");
    }

    let operation = json!({
        "adGroupOperation": {
            "update": ad_group,
            "updateMask": update_mask_fields.join(",")
        }
    });

    let changes = json!({
        "ad_group_id": ad_group_id,
        "name": name,
        "cpc_bid_micros": cpc_bid_micros,
    });

    let plan = ChangePlan::new(
        "update_ad_group".to_string(),
        "ad_group".to_string(),
        ad_group_id.to_string(),
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
    fn test_create_ad_group_success() {
        let config = Config::default();
        let result = create_ad_group(&config, "123-456-7890", "111", "My Ad Group", Some(2_000_000));
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_ad_group");
        assert_eq!(preview["entity_type"], "ad_group");
        assert_eq!(preview["entity_id"], "new");
        assert_eq!(preview["changes"]["ad_group_name"], "My Ad Group");
        assert_eq!(preview["changes"]["campaign_id"], "111");
    }

    #[test]
    fn test_create_ad_group_without_bid() {
        let config = Config::default();
        let result = create_ad_group(&config, "123-456-7890", "111", "My Ad Group", None);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_ad_group");
        assert!(preview["changes"]["cpc_bid_micros"].is_null());
    }

    #[test]
    fn test_create_ad_group_empty_name() {
        let config = Config::default();
        let result = create_ad_group(&config, "123-456-7890", "111", "", None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("ad_group_name must not be empty"));
    }

    #[test]
    fn test_create_ad_group_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_ad_group".to_string()];
        let result = create_ad_group(&config, "123-456-7890", "111", "Test", None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_update_ad_group_name_only() {
        let config = Config::default();
        let result = update_ad_group(&config, "123-456-7890", "555", Some("New Name"), None);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_ad_group");
        assert_eq!(preview["entity_id"], "555");
        assert_eq!(preview["changes"]["name"], "New Name");
    }

    #[test]
    fn test_update_ad_group_bid_only() {
        let config = Config::default();
        let result = update_ad_group(&config, "123-456-7890", "555", None, Some(3_000_000));
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_ad_group");
        assert_eq!(preview["changes"]["cpc_bid_micros"], 3_000_000);
    }

    #[test]
    fn test_update_ad_group_both_fields() {
        let config = Config::default();
        let result = update_ad_group(&config, "123-456-7890", "555", Some("Updated"), Some(1_500_000));
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["changes"]["name"], "Updated");
        assert_eq!(preview["changes"]["cpc_bid_micros"], 1_500_000);
    }

    #[test]
    fn test_update_ad_group_no_fields() {
        let config = Config::default();
        let result = update_ad_group(&config, "123-456-7890", "555", None, None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("At least one of name or cpc_bid_micros must be provided"));
    }

    #[test]
    fn test_update_ad_group_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["update_ad_group".to_string()];
        let result = update_ad_group(&config, "123-456-7890", "555", Some("Test"), None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
