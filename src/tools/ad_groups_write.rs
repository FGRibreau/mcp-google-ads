use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{AdGroupType, AdRotationMode, AdStatus, NextActionHint};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Draft creating a new ad group in an existing campaign.
///
/// `status` defaults to [`AdStatus::Paused`] when `None`, consistent with
/// the rest of the write tools — newly drafted entities ship paused for
/// safety. When the default `PAUSED` is in effect the response carries a
/// `next_action_hint` pointing at the `enable_entity` MCP tool so the
/// agent can complete the workflow without any UI step.
///
/// `ad_group_type` defaults to [`AdGroupType::SearchStandard`], the only type
/// this tool could create before. It must match the campaign: a Display
/// campaign needs `DISPLAY_STANDARD`, and Google rejects any mismatch.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn create_ad_group(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    ad_group_name: &str,
    cpc_bid_micros: Option<i64>,
    status: Option<AdStatus>,
    ad_group_type: Option<AdGroupType>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_ad_group", &config.safety)?;

    if ad_group_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "ad_group_name must not be empty".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);
    let resolved_status = status.unwrap_or_default();
    let resolved_type = ad_group_type.unwrap_or_default();

    let mut create = json!({
        "campaign": campaign_resource,
        "name": ad_group_name,
        "status": resolved_status.as_api_str(),
        "type": resolved_type.as_api_str()
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
        "status": resolved_status.as_api_str(),
        "ad_group_type": resolved_type.as_api_str(),
    });

    let mut plan = ChangePlan::new(
        "create_ad_group".to_string(),
        "ad_group".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        vec![operation],
    )
    .with_status_after_apply(resolved_status);

    if resolved_status == AdStatus::Paused {
        plan = plan.with_next_action_hint(NextActionHint::enable_ad_group(
            "<resolve ad_group_id from confirm_and_apply response>",
        ));
    }

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Draft updating an existing ad group (name, CPC bid, ad rotation mode and/or
/// optimized targeting).
///
/// At least one of `name`, `cpc_bid_micros`, `ad_rotation_mode` or
/// `optimized_targeting_enabled` must be provided. `ad_rotation_mode:
/// ROTATE_FOREVER` is what the Google Ads UI calls "Rotate indefinitely".
///
/// `optimized_targeting_enabled` maps to `AdGroup.optimized_targeting_enabled`.
/// It is on by default for Display ad groups and lets Google serve far beyond
/// the targeted audiences — a remarketing ad group with it on mostly reaches
/// people who never visited the site. Set it to `false` to keep the ad group
/// on its lists.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn update_ad_group(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    name: Option<&str>,
    cpc_bid_micros: Option<i64>,
    ad_rotation_mode: Option<AdRotationMode>,
    optimized_targeting_enabled: Option<bool>,
) -> Result<serde_json::Value> {
    check_blocked_operation("update_ad_group", &config.safety)?;

    if name.is_none()
        && cpc_bid_micros.is_none()
        && ad_rotation_mode.is_none()
        && optimized_targeting_enabled.is_none()
    {
        return Err(McpGoogleAdsError::Validation(
            "At least one of name, cpc_bid_micros, ad_rotation_mode or \
             optimized_targeting_enabled must be provided"
                .to_string(),
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
    if let Some(rotation) = ad_rotation_mode {
        ad_group["adRotationMode"] = json!(rotation.as_api_str());
        update_mask_fields.push("adRotationMode");
    }
    if let Some(enabled) = optimized_targeting_enabled {
        ad_group["optimizedTargetingEnabled"] = json!(enabled);
        update_mask_fields.push("optimizedTargetingEnabled");
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
        "ad_rotation_mode": ad_rotation_mode.map(|r| r.as_api_str()),
        "optimized_targeting_enabled": optimized_targeting_enabled,
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
        let result = create_ad_group(
            &config,
            "123-456-7890",
            "111",
            "My Ad Group",
            Some(2_000_000),
            None,
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_ad_group");
        assert_eq!(preview["entity_type"], "ad_group");
        assert_eq!(preview["entity_id"], "new");
        assert_eq!(preview["changes"]["ad_group_name"], "My Ad Group");
        assert_eq!(preview["changes"]["campaign_id"], "111");
    }

    #[test]
    fn test_create_ad_group_default_status_paused_with_hint() {
        // Consistency: ad_group default is PAUSED (was ENABLED in v0.2.x, the
        // only outlier among the write tools). Now matches campaigns/ads/pmax.
        let config = Config::default();
        let preview =
            create_ad_group(&config, "123-456-7890", "111", "AG1", None, None, None).unwrap();
        assert_eq!(preview["status_after_apply"], "PAUSED");
        assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
        assert_eq!(
            preview["next_action_hint"]["params"]["entity_type"],
            "ad_group"
        );
    }

    #[test]
    fn test_create_ad_group_explicit_enabled_no_hint() {
        let config = Config::default();
        let preview = create_ad_group(
            &config,
            "123-456-7890",
            "111",
            "AG1",
            None,
            Some(AdStatus::Enabled),
            None,
        )
        .unwrap();
        assert_eq!(preview["status_after_apply"], "ENABLED");
        assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());
    }

    #[test]
    fn test_create_ad_group_without_bid() {
        let config = Config::default();
        let result = create_ad_group(
            &config,
            "123-456-7890",
            "111",
            "My Ad Group",
            None,
            None,
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_ad_group");
        assert!(preview["changes"]["cpc_bid_micros"].is_null());
    }

    #[test]
    fn test_create_ad_group_empty_name() {
        let config = Config::default();
        let result = create_ad_group(&config, "123-456-7890", "111", "", None, None, None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("ad_group_name must not be empty"));
    }

    #[test]
    fn test_create_ad_group_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_ad_group".to_string()];
        let result = create_ad_group(&config, "123-456-7890", "111", "Test", None, None, None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_update_ad_group_name_only() {
        let config = Config::default();
        let result = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            Some("New Name"),
            None,
            None,
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_ad_group");
        assert_eq!(preview["entity_id"], "555");
        assert_eq!(preview["changes"]["name"], "New Name");
    }

    #[test]
    fn test_update_ad_group_bid_only() {
        let config = Config::default();
        let result = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            None,
            Some(3_000_000),
            None,
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_ad_group");
        assert_eq!(preview["changes"]["cpc_bid_micros"], 3_000_000);
    }

    #[test]
    fn test_update_ad_group_both_fields() {
        let config = Config::default();
        let result = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            Some("Updated"),
            Some(1_500_000),
            None,
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["changes"]["name"], "Updated");
        assert_eq!(preview["changes"]["cpc_bid_micros"], 1_500_000);
    }

    #[test]
    fn test_update_ad_group_rotation_only() {
        let config = Config::default();
        let result = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            None,
            None,
            Some(AdRotationMode::RotateForever),
            None,
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_ad_group");
        assert_eq!(preview["changes"]["ad_rotation_mode"], "ROTATE_FOREVER");
        assert!(preview["changes"]["name"].is_null());
        assert!(preview["changes"]["cpc_bid_micros"].is_null());
    }

    #[test]
    fn test_update_ad_group_no_fields() {
        let config = Config::default();
        let result = update_ad_group(&config, "123-456-7890", "555", None, None, None, None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("ad_rotation_mode or optimized_targeting_enabled"));
    }

    #[test]
    fn test_update_ad_group_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["update_ad_group".to_string()];
        let result = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            Some("Test"),
            None,
            None,
            None,
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    fn first_operation(preview: &serde_json::Value) -> serde_json::Value {
        use crate::safety::preview::{get_plan, remove_plan};
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let op = plan.mutate_operations[0].clone();
        remove_plan(plan_id);
        op
    }

    #[test]
    fn test_create_ad_group_defaults_to_search_standard() {
        let config = Config::default();
        let preview =
            create_ad_group(&config, "123-456-7890", "111", "AG1", None, None, None).unwrap();
        assert_eq!(preview["changes"]["ad_group_type"], "SEARCH_STANDARD");
        let op = first_operation(&preview);
        assert_eq!(op["adGroupOperation"]["create"]["type"], "SEARCH_STANDARD");
    }

    #[test]
    fn test_create_ad_group_display_standard() {
        let config = Config::default();
        let preview = create_ad_group(
            &config,
            "123-456-7890",
            "111",
            "Retargeting | Layer 1",
            None,
            None,
            Some(AdGroupType::DisplayStandard),
        )
        .unwrap();
        assert_eq!(preview["changes"]["ad_group_type"], "DISPLAY_STANDARD");
        let op = first_operation(&preview);
        assert_eq!(op["adGroupOperation"]["create"]["type"], "DISPLAY_STANDARD");
        // The type changes nothing about the paused-by-default safety.
        assert_eq!(op["adGroupOperation"]["create"]["status"], "PAUSED");
    }

    #[test]
    fn test_update_ad_group_optimized_targeting_off() {
        let config = Config::default();
        let preview = update_ad_group(
            &config,
            "123-456-7890",
            "9000000105",
            None,
            None,
            None,
            Some(false),
        )
        .unwrap();
        assert_eq!(preview["changes"]["optimized_targeting_enabled"], false);
        let op = first_operation(&preview);
        assert_eq!(
            op["adGroupOperation"]["update"]["optimizedTargetingEnabled"],
            false
        );
        assert_eq!(
            op["adGroupOperation"]["updateMask"],
            "optimizedTargetingEnabled"
        );
    }

    #[test]
    fn test_update_ad_group_optimized_targeting_combines_with_name() {
        let config = Config::default();
        let preview = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            Some("Remarketing only"),
            None,
            None,
            Some(true),
        )
        .unwrap();
        let op = first_operation(&preview);
        assert_eq!(
            op["adGroupOperation"]["update"]["optimizedTargetingEnabled"],
            true
        );
        assert_eq!(
            op["adGroupOperation"]["updateMask"],
            "name,optimizedTargetingEnabled"
        );
    }
}
