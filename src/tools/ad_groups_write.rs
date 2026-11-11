use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{AdGroupType, AdRotationMode, AdStatus, AudienceTargetingMode, NextActionHint};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::validate_numeric_id;

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

/// A change to an ad group's audience targeting mode, together with the
/// target restrictions the ad group has now.
///
/// The update has to carry the whole `target_restrictions` list: Google
/// overwrites the entire list when the field mask names it (the reason the
/// official `update_audience_target_restriction` example reads the ad group
/// first). `current_restrictions` is that read — see
/// [`fetch_ad_group_target_restrictions`] — so the restrictions of the other
/// dimensions survive the update.
#[derive(Debug, Clone, Copy)]
pub struct AudienceTargetingUpdate<'a> {
    pub mode: AudienceTargetingMode,
    /// `TargetRestriction` objects as returned by the API
    /// (`{"targetingDimension": "...", "bidOnly": ...}`).
    pub current_restrictions: &'a [serde_json::Value],
}

/// Read an ad group's current `targeting_setting.target_restrictions`.
///
/// Errors when the ad group does not exist, so an update is never drafted
/// against a typo'd ID.
pub async fn fetch_ad_group_target_restrictions(
    client: &GoogleAdsClient,
    customer_id: &str,
    ad_group_id: &str,
) -> Result<Vec<serde_json::Value>> {
    let ad_group = validate_numeric_id("ad_group_id", ad_group_id)?;
    let query = format!(
        "SELECT ad_group.id, ad_group.targeting_setting.target_restrictions \
         FROM ad_group WHERE ad_group.id = {}",
        ad_group
    );
    let rows = client.search(customer_id, &query).await?;
    let row = rows.first().ok_or_else(|| {
        McpGoogleAdsError::Validation(format!(
            "ad group {} not found in customer {}",
            ad_group, customer_id
        ))
    })?;
    Ok(row
        .pointer("/adGroup/targetingSetting/targetRestrictions")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
}

/// The target restriction list after setting the AUDIENCE dimension to `mode`.
///
/// Every other dimension is kept as it is, in place; the AUDIENCE entry is
/// replaced where it was, or appended when the ad group had none.
pub fn merge_audience_restriction(
    current: &[serde_json::Value],
    mode: AudienceTargetingMode,
) -> Vec<serde_json::Value> {
    let audience = json!({ "targetingDimension": "AUDIENCE", "bidOnly": mode.bid_only() });
    let mut merged = Vec::with_capacity(current.len() + 1);
    let mut replaced = false;
    for restriction in current {
        let dimension = restriction
            .get("targetingDimension")
            .and_then(|d| d.as_str());
        match dimension {
            Some("AUDIENCE") => {
                if !replaced {
                    merged.push(audience.clone());
                    replaced = true;
                }
            }
            Some(other) => {
                let mut kept = json!({ "targetingDimension": other });
                if let Some(bid_only) = restriction.get("bidOnly").and_then(|b| b.as_bool()) {
                    kept["bidOnly"] = json!(bid_only);
                }
                merged.push(kept);
            }
            None => {}
        }
    }
    if !replaced {
        merged.push(audience);
    }
    merged
}

/// Draft updating an existing ad group (name, CPC bid, ad rotation mode,
/// optimized targeting and/or audience targeting mode).
///
/// At least one of `name`, `cpc_bid_micros`, `ad_rotation_mode`,
/// `optimized_targeting_enabled` or `audience_targeting` must be provided.
/// `ad_rotation_mode: ROTATE_FOREVER` is what the Google Ads UI calls "Rotate
/// indefinitely".
///
/// `optimized_targeting_enabled` maps to `AdGroup.optimized_targeting_enabled`.
/// It is on by default for Display ad groups and lets Google serve far beyond
/// the targeted audiences — a remarketing ad group with it on mostly reaches
/// people who never visited the site. Set it to `false` to keep the ad group
/// on its lists.
///
/// `audience_targeting` sets the AUDIENCE target restriction: `TARGETING`
/// (`bid_only = false`, segments narrow reach) or `OBSERVATION`
/// (`bid_only = true`, segments only report and adjust bids). The list written
/// is [`merge_audience_restriction`] of the ad group's current restrictions,
/// under the mask `targetingSetting.targetRestrictions`.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
#[allow(clippy::too_many_arguments)]
pub fn update_ad_group(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    name: Option<&str>,
    cpc_bid_micros: Option<i64>,
    ad_rotation_mode: Option<AdRotationMode>,
    optimized_targeting_enabled: Option<bool>,
    audience_targeting: Option<AudienceTargetingUpdate<'_>>,
) -> Result<serde_json::Value> {
    check_blocked_operation("update_ad_group", &config.safety)?;

    if name.is_none()
        && cpc_bid_micros.is_none()
        && ad_rotation_mode.is_none()
        && optimized_targeting_enabled.is_none()
        && audience_targeting.is_none()
    {
        return Err(McpGoogleAdsError::Validation(
            "At least one of name, cpc_bid_micros, ad_rotation_mode, \
             optimized_targeting_enabled or audience_targeting_mode must be provided"
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
    let merged_restrictions = audience_targeting
        .map(|update| merge_audience_restriction(update.current_restrictions, update.mode));
    if let Some(ref restrictions) = merged_restrictions {
        ad_group["targetingSetting"] = json!({ "targetRestrictions": restrictions });
        update_mask_fields.push("targetingSetting.targetRestrictions");
    }

    let operation = json!({
        "adGroupOperation": {
            "update": ad_group,
            "updateMask": update_mask_fields.join(",")
        }
    });

    let mut changes = json!({
        "ad_group_id": ad_group_id,
        "name": name,
        "cpc_bid_micros": cpc_bid_micros,
        "ad_rotation_mode": ad_rotation_mode.map(|r| r.as_api_str()),
        "optimized_targeting_enabled": optimized_targeting_enabled,
    });
    if let (Some(update), Some(restrictions)) = (audience_targeting, merged_restrictions) {
        changes["audience_targeting_mode"] = json!(update.mode.as_str());
        changes["target_restrictions_before"] = json!(update.current_restrictions);
        changes["target_restrictions_after"] = json!(restrictions);
        changes["audience_targeting_note"] = json!(
            "Google replaces the whole target restriction list under this mask, so the list \
             sent is the one read just now with only the AUDIENCE entry changed. If the ad \
             group's restrictions change before this plan is applied, draft it again."
        );
    }

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
        let result = update_ad_group(&config, "123-456-7890", "555", None, None, None, None, None);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("optimized_targeting_enabled or audience_targeting_mode"));
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
            None,
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
            None,
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

    #[test]
    fn merge_keeps_other_dimensions_and_replaces_audience_in_place() {
        let current = vec![
            json!({"targetingDimension": "TOPIC", "bidOnly": true}),
            json!({"targetingDimension": "AUDIENCE", "bidOnly": true}),
            json!({"targetingDimension": "AGE_RANGE", "bidOnly": true}),
        ];
        let merged = merge_audience_restriction(&current, AudienceTargetingMode::Targeting);
        assert_eq!(
            merged,
            vec![
                json!({"targetingDimension": "TOPIC", "bidOnly": true}),
                json!({"targetingDimension": "AUDIENCE", "bidOnly": false}),
                json!({"targetingDimension": "AGE_RANGE", "bidOnly": true}),
            ]
        );
    }

    #[test]
    fn merge_appends_audience_when_absent_and_keeps_unset_bid_only_unset() {
        let current = vec![json!({"targetingDimension": "TOPIC"})];
        let merged = merge_audience_restriction(&current, AudienceTargetingMode::Observation);
        assert_eq!(
            merged,
            vec![
                json!({"targetingDimension": "TOPIC"}),
                json!({"targetingDimension": "AUDIENCE", "bidOnly": true}),
            ]
        );
        assert_eq!(
            merge_audience_restriction(&[], AudienceTargetingMode::Targeting),
            vec![json!({"targetingDimension": "AUDIENCE", "bidOnly": false})]
        );
    }

    #[test]
    fn audience_targeting_mode_writes_the_merged_list_under_one_mask() {
        let config = Config::default();
        let current = vec![json!({"targetingDimension": "TOPIC", "bidOnly": true})];
        let preview = update_ad_group(
            &config,
            "123-456-7890",
            "555",
            None,
            None,
            None,
            Some(false),
            Some(AudienceTargetingUpdate {
                mode: AudienceTargetingMode::Observation,
                current_restrictions: &current,
            }),
        )
        .unwrap();
        assert_eq!(preview["changes"]["audience_targeting_mode"], "OBSERVATION");
        assert_eq!(
            preview["changes"]["target_restrictions_before"],
            json!(current)
        );

        let op = first_operation(&preview);
        assert_eq!(
            op["adGroupOperation"]["update"]["targetingSetting"]["targetRestrictions"],
            json!([
                {"targetingDimension": "TOPIC", "bidOnly": true},
                {"targetingDimension": "AUDIENCE", "bidOnly": true}
            ])
        );
        assert_eq!(
            op["adGroupOperation"]["updateMask"],
            "optimizedTargetingEnabled,targetingSetting.targetRestrictions"
        );
    }
}
