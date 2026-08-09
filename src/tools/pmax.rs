use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{AdStatus, NextActionHint};
use crate::safety::guards::{
    check_blocked_operation, check_budget_cap, validate_description, validate_headline,
};
use crate::safety::preview::{store_plan, ChangePlan};

/// Convert a dollar amount to micros (Google Ads uses micros: $1 = 1_000_000).
fn dollars_to_micros(dollars: f64) -> i64 {
    (dollars * 1_000_000.0) as i64
}

/// Parameters for creating a Performance Max campaign.
pub struct CreatePmaxCampaignParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub campaign_name: &'a str,
    pub daily_budget: f64,
    pub bidding_strategy: &'a str,
    pub final_urls: Vec<String>,
    pub headlines: Vec<String>,
    pub long_headlines: Vec<String>,
    pub descriptions: Vec<String>,
    pub business_name: &'a str,
    pub geo_target_ids: Vec<String>,
    pub start_paused: bool,
}

/// Create a Performance Max campaign as an atomic batch.
///
/// Uses temporary resource IDs: -1 for budget, -2 for campaign, -3 for asset group.
/// Text assets (headlines, long headlines, descriptions) are created and linked to the asset group.
/// Image assets require separate upload via `upload_image_asset`.
///
/// Validations:
/// - 3-15 headlines (max 30 chars each)
/// - 1-5 long headlines (max 90 chars each)
/// - 2-5 descriptions (max 90 chars each)
/// - business_name max 25 chars
/// - budget cap check
/// - at least one final URL
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn create_pmax_campaign(params: &CreatePmaxCampaignParams) -> Result<serde_json::Value> {
    check_blocked_operation("create_pmax_campaign", &params.config.safety)?;
    check_budget_cap(params.daily_budget, &params.config.safety)?;

    // Validate headline count
    if params.headlines.len() < 3 || params.headlines.len() > 15 {
        return Err(McpGoogleAdsError::Validation(format!(
            "PMax requires 3-15 headlines, got {}",
            params.headlines.len()
        )));
    }

    // Validate long headline count
    if params.long_headlines.is_empty() || params.long_headlines.len() > 5 {
        return Err(McpGoogleAdsError::Validation(format!(
            "PMax requires 1-5 long headlines, got {}",
            params.long_headlines.len()
        )));
    }

    // Validate description count
    if params.descriptions.len() < 2 || params.descriptions.len() > 5 {
        return Err(McpGoogleAdsError::Validation(format!(
            "PMax requires 2-5 descriptions, got {}",
            params.descriptions.len()
        )));
    }

    // Validate individual headline lengths (max 30 chars)
    for headline in &params.headlines {
        validate_headline(headline)?;
    }

    // Validate long headlines (max 90 chars)
    for lh in &params.long_headlines {
        validate_description(lh)?;
    }

    // Validate descriptions (max 90 chars)
    for desc in &params.descriptions {
        validate_description(desc)?;
    }

    // Validate business name (max 25 chars)
    if params.business_name.len() > 25 {
        return Err(McpGoogleAdsError::Validation(format!(
            "Business name exceeds 25 character limit ({} chars)",
            params.business_name.len()
        )));
    }

    if params.final_urls.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one final URL is required".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(params.customer_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();

    // 1. Campaign budget (temp resource ID -1)
    let budget_resource = format!("customers/{}/campaignBudgets/-1", cid);
    operations.push(json!({
        "campaignBudgetOperation": {
            "create": {
                "name": format!("{} Budget", params.campaign_name),
                "amountMicros": dollars_to_micros(params.daily_budget).to_string(),
                "deliveryMethod": "STANDARD",
                // Must be explicit: budgets default to shared, and Google rejects
                // a shared budget on a Performance Max campaign with
                // BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET.
                "explicitlyShared": false,
                "resourceName": budget_resource
            }
        }
    }));

    // 2. Campaign (temp resource ID -2)
    let campaign_resource = format!("customers/{}/campaigns/-2", cid);
    let resolved_status = if params.start_paused {
        AdStatus::Paused
    } else {
        AdStatus::Enabled
    };

    let mut campaign_create = json!({
        "name": params.campaign_name,
        "status": resolved_status.as_api_str(),
        "advertisingChannelType": "PERFORMANCE_MAX",
        "campaignBudget": budget_resource,
        // Required on every campaign create since the EU political ads
        // regulation; omitting it fails with fieldError=REQUIRED.
        "containsEuPoliticalAdvertising": "DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING",
        "resourceName": campaign_resource
    });

    // Apply bidding strategy
    match params.bidding_strategy {
        "MAXIMIZE_CONVERSIONS" => {
            campaign_create
                .as_object_mut()
                .map(|o| o.insert("maximizeConversions".to_string(), json!({})));
        }
        "MAXIMIZE_CONVERSION_VALUE" => {
            campaign_create
                .as_object_mut()
                .map(|o| o.insert("maximizeConversionValue".to_string(), json!({})));
        }
        "TARGET_CPA" => {
            campaign_create
                .as_object_mut()
                .map(|o| o.insert("maximizeConversions".to_string(), json!({})));
        }
        _ => {
            campaign_create
                .as_object_mut()
                .map(|o| o.insert("maximizeConversions".to_string(), json!({})));
        }
    }

    operations.push(json!({
        "campaignOperation": {
            "create": campaign_create
        }
    }));

    // 3. Geo targets
    for geo_id in &params.geo_target_ids {
        operations.push(json!({
            "campaignCriterionOperation": {
                "create": {
                    "campaign": campaign_resource,
                    "location": {
                        "geoTargetConstant": format!("geoTargetConstants/{}", geo_id)
                    }
                }
            }
        }));
    }

    // 4. Asset group (temp resource ID -3) — inherits the campaign-level
    // status so a PMax campaign that opted into ENABLED doesn't ship with
    // a paused asset group blocking traffic.
    let asset_group_resource = format!("customers/{}/assetGroups/-3", cid);
    operations.push(json!({
        "assetGroupOperation": {
            "create": {
                "name": format!("{} Asset Group", params.campaign_name),
                "campaign": campaign_resource,
                "finalUrls": params.final_urls,
                "status": resolved_status.as_api_str(),
                "resourceName": asset_group_resource
            }
        }
    }));

    // 5. Text assets — headlines
    let mut temp_asset_id: i64 = -100;
    for headline in &params.headlines {
        let asset_resource = format!("customers/{}/assets/{}", cid, temp_asset_id);
        operations.push(json!({
            "assetOperation": {
                "create": {
                    "resourceName": asset_resource,
                    "textAsset": {
                        "text": headline
                    }
                }
            }
        }));
        operations.push(json!({
            "assetGroupAssetOperation": {
                "create": {
                    "assetGroup": asset_group_resource,
                    "asset": asset_resource,
                    "fieldType": "HEADLINE"
                }
            }
        }));
        temp_asset_id -= 1;
    }

    // 6. Text assets — long headlines
    for lh in &params.long_headlines {
        let asset_resource = format!("customers/{}/assets/{}", cid, temp_asset_id);
        operations.push(json!({
            "assetOperation": {
                "create": {
                    "resourceName": asset_resource,
                    "textAsset": {
                        "text": lh
                    }
                }
            }
        }));
        operations.push(json!({
            "assetGroupAssetOperation": {
                "create": {
                    "assetGroup": asset_group_resource,
                    "asset": asset_resource,
                    "fieldType": "LONG_HEADLINE"
                }
            }
        }));
        temp_asset_id -= 1;
    }

    // 7. Text assets — descriptions
    for desc in &params.descriptions {
        let asset_resource = format!("customers/{}/assets/{}", cid, temp_asset_id);
        operations.push(json!({
            "assetOperation": {
                "create": {
                    "resourceName": asset_resource,
                    "textAsset": {
                        "text": desc
                    }
                }
            }
        }));
        operations.push(json!({
            "assetGroupAssetOperation": {
                "create": {
                    "assetGroup": asset_group_resource,
                    "asset": asset_resource,
                    "fieldType": "DESCRIPTION"
                }
            }
        }));
        temp_asset_id -= 1;
    }

    // 8. Business name asset
    let biz_asset_resource = format!("customers/{}/assets/{}", cid, temp_asset_id);
    operations.push(json!({
        "assetOperation": {
            "create": {
                "resourceName": biz_asset_resource,
                "textAsset": {
                    "text": params.business_name
                }
            }
        }
    }));
    operations.push(json!({
        "assetGroupAssetOperation": {
            "create": {
                "assetGroup": asset_group_resource,
                "asset": biz_asset_resource,
                "fieldType": "BUSINESS_NAME"
            }
        }
    }));

    let changes = json!({
        "campaign_name": params.campaign_name,
        "daily_budget": params.daily_budget,
        "bidding_strategy": params.bidding_strategy,
        "channel_type": "PERFORMANCE_MAX",
        "headlines_count": params.headlines.len(),
        "long_headlines_count": params.long_headlines.len(),
        "descriptions_count": params.descriptions.len(),
        "business_name": params.business_name,
        "final_urls": params.final_urls,
        "geo_targets": params.geo_target_ids,
        "start_paused": params.start_paused,
        "note": "Image assets require separate upload via upload_image_asset"
    });

    let mut plan = ChangePlan::new(
        "create_pmax_campaign".to_string(),
        "campaign".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        operations,
    )
    .with_status_after_apply(resolved_status);

    if resolved_status == AdStatus::Paused {
        plan = plan.with_next_action_hint(NextActionHint::enable_parent_campaign_after_create(
            "customers/{cid}/campaigns/-2",
        ));
    }

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn default_params(config: &Config) -> CreatePmaxCampaignParams<'_> {
        CreatePmaxCampaignParams {
            config,
            customer_id: "123-456-7890",
            campaign_name: "Test PMax",
            daily_budget: 10.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            final_urls: vec!["https://example.com".to_string()],
            headlines: vec![
                "Headline 1".to_string(),
                "Headline 2".to_string(),
                "Headline 3".to_string(),
            ],
            long_headlines: vec!["Long Headline 1".to_string()],
            descriptions: vec!["Description 1".to_string(), "Description 2".to_string()],
            business_name: "Test Business",
            geo_target_ids: vec!["2840".to_string()],
            start_paused: true,
        }
    }

    #[test]
    fn test_create_pmax_success() {
        let config = Config::default();
        let params = default_params(&config);
        let result = create_pmax_campaign(&params);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_pmax_campaign");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_create_pmax_budget_cap_exceeded() {
        let mut config = Config::default();
        config.safety.max_daily_budget = 5.0;
        let mut params = default_params(&config);
        params.daily_budget = 10.0;
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_too_few_headlines() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.headlines = vec!["H1".to_string(), "H2".to_string()];
        let result = create_pmax_campaign(&params);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("3-15 headlines"));
    }

    #[test]
    fn test_create_pmax_too_many_headlines() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.headlines = (0..16).map(|i| format!("H{}", i)).collect();
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_no_long_headlines() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.long_headlines = vec![];
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_too_few_descriptions() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.descriptions = vec!["D1".to_string()];
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_headline_too_long() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.headlines[0] = "A".repeat(31);
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_business_name_too_long() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.business_name = Box::leak("A".repeat(26).into_boxed_str());
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_no_final_urls() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.final_urls = vec![];
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_pmax_campaign".to_string()];
        let params = default_params(&config);
        assert!(create_pmax_campaign(&params).is_err());
    }

    #[test]
    fn test_create_pmax_start_paused_true_sets_paused_in_payload() {
        let config = Config::default();
        let mut params = default_params(&config);
        params.start_paused = true;
        let preview = create_pmax_campaign(&params).unwrap();
        assert_eq!(preview["status_after_apply"], "PAUSED");
    }

    #[test]
    fn test_create_pmax_start_paused_false_sets_enabled_in_payload() {
        // Regression for the v0.2.x bug: `start_paused=false` was ignored and
        // the campaign always shipped PAUSED. The fix wires start_paused into
        // the actual mutate operation status.
        let config = Config::default();
        let mut params = default_params(&config);
        params.start_paused = false;
        let preview = create_pmax_campaign(&params).unwrap();
        assert_eq!(preview["status_after_apply"], "ENABLED");
        // No hint when ENABLED — the workflow is complete.
        assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());
    }

    /// Pull the raw mutate operations back off the stored plan.
    fn pmax_ops() -> Vec<serde_json::Value> {
        let config = Config::default();
        let params = default_params(&config);
        let preview = create_pmax_campaign(&params).unwrap();
        let plan_id = preview["plan_id"].as_str().unwrap_or_default();
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");
        plan.mutate_operations.clone()
    }

    #[test]
    fn test_create_pmax_budget_not_shared() {
        // Budgets default to shared server-side, and PMax rejects a shared budget
        // with BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET. draft_campaign
        // got this fix in 5670d2d; create_pmax_campaign was missed.
        let ops = pmax_ops();
        let budget_create = ops
            .iter()
            .find_map(|op| op.pointer("/campaignBudgetOperation/create"))
            .expect("budget operation present");
        assert_eq!(budget_create["explicitlyShared"], json!(false));
    }

    #[test]
    fn test_create_pmax_sets_eu_political_advertising() {
        // Required on every campaign create; without it the whole mutate fails
        // with fieldError=REQUIRED on contains_eu_political_advertising.
        let ops = pmax_ops();
        let campaign_create = ops
            .iter()
            .find_map(|op| op.pointer("/campaignOperation/create"))
            .expect("campaign operation present");
        assert_eq!(
            campaign_create["containsEuPoliticalAdvertising"],
            json!("DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING")
        );
    }
}
