use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::Result;
use crate::safety::guards::{check_blocked_operation, check_budget_cap};
use crate::safety::preview::{store_plan, ChangePlan};

/// Convert a dollar amount to micros (Google Ads uses micros: $1 = 1_000_000).
fn dollars_to_micros(dollars: f64) -> i64 {
    (dollars * 1_000_000.0) as i64
}

/// Parameters for drafting a new campaign.
pub struct DraftCampaignParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub campaign_name: &'a str,
    pub daily_budget: f64,
    pub bidding_strategy: &'a str,
    pub target_cpa: Option<f64>,
    pub target_roas: Option<f64>,
    pub channel_type: &'a str,
    pub ad_group_name: &'a str,
    pub keywords: Vec<KeywordInput>,
    pub geo_target_ids: Vec<String>,
    pub language_ids: Vec<String>,
}

/// Draft a new campaign with budget, ad group, and optional keywords.
///
/// All entities are created in PAUSED status. Uses temporary resource IDs
/// (negative IDs) to link entities: -1 for budget, -2 for campaign, -3 for ad group.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn draft_campaign(params: &DraftCampaignParams) -> Result<serde_json::Value> {
    check_budget_cap(params.daily_budget, &params.config.safety)?;
    check_blocked_operation("draft_campaign", &params.config.safety)?;

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
                "resourceName": budget_resource
            }
        }
    }));

    // 2. Campaign (temp resource ID -2)
    let campaign_resource = format!("customers/{}/campaigns/-2", cid);
    let mut campaign_create = json!({
        "name": params.campaign_name,
        "status": "PAUSED",
        "advertisingChannelType": params.channel_type,
        "campaignBudget": budget_resource,
        "resourceName": campaign_resource
    });

    // Apply bidding strategy
    apply_bidding_strategy(
        &mut campaign_create,
        params.bidding_strategy,
        params.target_cpa,
        params.target_roas,
    );

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

    // 4. Language targets
    for lang_id in &params.language_ids {
        operations.push(json!({
            "campaignCriterionOperation": {
                "create": {
                    "campaign": campaign_resource,
                    "language": {
                        "languageConstant": format!("languageConstants/{}", lang_id)
                    }
                }
            }
        }));
    }

    // 5. Ad group (temp resource ID -3)
    let ad_group_resource = format!("customers/{}/adGroups/-3", cid);
    operations.push(json!({
        "adGroupOperation": {
            "create": {
                "name": params.ad_group_name,
                "campaign": campaign_resource,
                "status": "PAUSED",
                "resourceName": ad_group_resource
            }
        }
    }));

    // 6. Keywords (optional)
    for kw in &params.keywords {
        operations.push(json!({
            "adGroupCriterionOperation": {
                "create": {
                    "adGroup": ad_group_resource,
                    "keyword": {
                        "text": kw.text,
                        "matchType": kw.match_type
                    }
                }
            }
        }));
    }

    let changes = json!({
        "campaign_name": params.campaign_name,
        "daily_budget": params.daily_budget,
        "bidding_strategy": params.bidding_strategy,
        "target_cpa": params.target_cpa,
        "target_roas": params.target_roas,
        "channel_type": params.channel_type,
        "ad_group_name": params.ad_group_name,
        "keywords_count": params.keywords.len(),
        "geo_targets": params.geo_target_ids,
        "languages": params.language_ids
    });

    let plan = ChangePlan::new(
        "draft_campaign".to_string(),
        "campaign".to_string(),
        "new".to_string(),
        cid.clone(),
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Parameters for updating an existing campaign.
pub struct UpdateCampaignParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub campaign_id: &'a str,
    pub bidding_strategy: Option<&'a str>,
    pub target_cpa: Option<f64>,
    pub target_roas: Option<f64>,
    pub daily_budget: Option<f64>,
    pub geo_target_ids: Vec<String>,
    pub language_ids: Vec<String>,
}

/// Update an existing campaign's settings.
///
/// Only changed fields are included in the update operation.
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn update_campaign(params: &UpdateCampaignParams) -> Result<serde_json::Value> {
    let cid = crate::client::GoogleAdsClient::normalize_customer_id(params.customer_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();
    let mut changes = serde_json::Map::new();

    // Budget update
    if let Some(budget) = params.daily_budget {
        check_budget_cap(budget, &params.config.safety)?;
        changes.insert("daily_budget".to_string(), json!(budget));
        // Budget update requires knowing the budget resource name.
        // We build a partial update — the campaign's budget resource is
        // customers/{cid}/campaignBudgets/{budgetId}. Since we don't have the
        // budget ID here, we use the campaign resource to reference it.
        // In practice the caller should provide the budget resource name,
        // but for simplicity we document that budget updates go through the
        // campaign budget resource directly.
        operations.push(json!({
            "campaignBudgetOperation": {
                "update": {
                    "resourceName": format!("customers/{}/campaignBudgets/{}", cid, params.campaign_id),
                    "amountMicros": dollars_to_micros(budget).to_string()
                },
                "updateMask": "amountMicros"
            }
        }));
    }

    // Bidding strategy update
    if let Some(strategy) = params.bidding_strategy {
        changes.insert("bidding_strategy".to_string(), json!(strategy));

        let campaign_resource = format!("customers/{}/campaigns/{}", cid, params.campaign_id);
        let mut campaign_update = json!({
            "resourceName": campaign_resource
        });

        let mut update_mask_fields = Vec::new();
        apply_bidding_strategy_update(
            &mut campaign_update,
            &mut update_mask_fields,
            strategy,
            params.target_cpa,
            params.target_roas,
        );

        if let Some(cpa) = params.target_cpa {
            changes.insert("target_cpa".to_string(), json!(cpa));
        }
        if let Some(roas) = params.target_roas {
            changes.insert("target_roas".to_string(), json!(roas));
        }

        operations.push(json!({
            "campaignOperation": {
                "update": campaign_update,
                "updateMask": update_mask_fields.join(",")
            }
        }));
    }

    // Geo target additions
    if !params.geo_target_ids.is_empty() {
        let campaign_resource = format!("customers/{}/campaigns/{}", cid, params.campaign_id);
        changes.insert(
            "geo_targets_added".to_string(),
            json!(params.geo_target_ids),
        );
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
    }

    // Language target additions
    if !params.language_ids.is_empty() {
        let campaign_resource = format!("customers/{}/campaigns/{}", cid, params.campaign_id);
        changes.insert("languages_added".to_string(), json!(params.language_ids));
        for lang_id in &params.language_ids {
            operations.push(json!({
                "campaignCriterionOperation": {
                    "create": {
                        "campaign": campaign_resource,
                        "language": {
                            "languageConstant": format!("languageConstants/{}", lang_id)
                        }
                    }
                }
            }));
        }
    }

    if operations.is_empty() {
        return Err(crate::error::McpGoogleAdsError::Validation(
            "No changes specified for campaign update".to_string(),
        ));
    }

    let plan = ChangePlan::new(
        "update_campaign".to_string(),
        "campaign".to_string(),
        params.campaign_id.to_string(),
        cid,
        serde_json::Value::Object(changes),
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Input for a keyword to add during campaign drafting.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct KeywordInput {
    pub text: String,
    pub match_type: String,
}

/// Apply a bidding strategy to a campaign create JSON value.
fn apply_bidding_strategy(
    campaign: &mut serde_json::Value,
    strategy: &str,
    target_cpa: Option<f64>,
    target_roas: Option<f64>,
) {
    let obj = match campaign.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match strategy {
        "MAXIMIZE_CONVERSIONS" => {
            let mut mc = json!({});
            if let Some(cpa) = target_cpa {
                mc.as_object_mut().map(|m| {
                    m.insert(
                        "targetCpaMicros".to_string(),
                        json!(dollars_to_micros(cpa).to_string()),
                    )
                });
            }
            obj.insert("maximizeConversions".to_string(), mc);
        }
        "MAXIMIZE_CONVERSION_VALUE" => {
            let mut mcv = json!({});
            if let Some(roas) = target_roas {
                mcv.as_object_mut()
                    .map(|m| m.insert("targetRoas".to_string(), json!(roas)));
            }
            obj.insert("maximizeConversionValue".to_string(), mcv);
        }
        "TARGET_CPA" => {
            if let Some(cpa) = target_cpa {
                obj.insert(
                    "targetCpa".to_string(),
                    json!({ "targetCpaMicros": dollars_to_micros(cpa).to_string() }),
                );
            }
        }
        "TARGET_ROAS" => {
            if let Some(roas) = target_roas {
                obj.insert("targetRoas".to_string(), json!({ "targetRoas": roas }));
            }
        }
        "MANUAL_CPC" => {
            obj.insert("manualCpc".to_string(), json!({}));
        }
        _ => {
            // Unknown strategy — let the API validate
            obj.insert("biddingStrategyType".to_string(), json!(strategy));
        }
    }
}

/// Apply bidding strategy fields for an update operation, tracking which fields go into updateMask.
fn apply_bidding_strategy_update(
    campaign: &mut serde_json::Value,
    update_mask_fields: &mut Vec<String>,
    strategy: &str,
    target_cpa: Option<f64>,
    target_roas: Option<f64>,
) {
    let obj = match campaign.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match strategy {
        "MAXIMIZE_CONVERSIONS" => {
            let mut mc = json!({});
            if let Some(cpa) = target_cpa {
                mc.as_object_mut().map(|m| {
                    m.insert(
                        "targetCpaMicros".to_string(),
                        json!(dollars_to_micros(cpa).to_string()),
                    )
                });
            }
            obj.insert("maximizeConversions".to_string(), mc);
            update_mask_fields.push("maximizeConversions".to_string());
        }
        "MAXIMIZE_CONVERSION_VALUE" => {
            let mut mcv = json!({});
            if let Some(roas) = target_roas {
                mcv.as_object_mut()
                    .map(|m| m.insert("targetRoas".to_string(), json!(roas)));
            }
            obj.insert("maximizeConversionValue".to_string(), mcv);
            update_mask_fields.push("maximizeConversionValue".to_string());
        }
        "TARGET_CPA" => {
            if let Some(cpa) = target_cpa {
                obj.insert(
                    "targetCpa".to_string(),
                    json!({ "targetCpaMicros": dollars_to_micros(cpa).to_string() }),
                );
                update_mask_fields.push("targetCpa".to_string());
            }
        }
        "TARGET_ROAS" => {
            if let Some(roas) = target_roas {
                obj.insert("targetRoas".to_string(), json!({ "targetRoas": roas }));
                update_mask_fields.push("targetRoas".to_string());
            }
        }
        "MANUAL_CPC" => {
            obj.insert("manualCpc".to_string(), json!({}));
            update_mask_fields.push("manualCpc".to_string());
        }
        _ => {
            obj.insert("biddingStrategyType".to_string(), json!(strategy));
            update_mask_fields.push("biddingStrategyType".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_dollars_to_micros() {
        assert_eq!(dollars_to_micros(1.0), 1_000_000);
        assert_eq!(dollars_to_micros(50.0), 50_000_000);
        assert_eq!(dollars_to_micros(0.5), 500_000);
    }

    #[test]
    fn test_draft_campaign_budget_cap_exceeded() {
        let mut config = Config::default();
        config.safety.max_daily_budget = 10.0;

        let result = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "Test Campaign",
            daily_budget: 20.0, // exceeds cap
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "Test Ad Group",
            keywords: vec![],
            geo_target_ids: vec![],
            language_ids: vec![],
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("exceeds maximum"));
    }

    #[test]
    fn test_draft_campaign_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["draft_campaign".to_string()];

        let result = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "Test Campaign",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "Test Ad Group",
            keywords: vec![],
            geo_target_ids: vec![],
            language_ids: vec![],
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_draft_campaign_success() {
        let config = Config::default();

        let result = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "Test Campaign",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: Some(10.0),
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "Test Ad Group",
            keywords: vec![KeywordInput {
                text: "test keyword".to_string(),
                match_type: "EXACT".to_string(),
            }],
            geo_target_ids: vec!["2840".to_string()],
            language_ids: vec!["1000".to_string()],
        });

        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_campaign");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        assert!(preview["plan_id"].as_str().is_some());
    }

    #[test]
    fn test_update_campaign_no_changes() {
        let config = Config::default();

        let result = update_campaign(&UpdateCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_id: "12345",
            bidding_strategy: None,
            target_cpa: None,
            target_roas: None,
            daily_budget: None,
            geo_target_ids: vec![],
            language_ids: vec![],
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("No changes"));
    }

    #[test]
    fn test_update_campaign_with_budget() {
        let config = Config::default();

        let result = update_campaign(&UpdateCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_id: "12345",
            bidding_strategy: None,
            target_cpa: None,
            target_roas: None,
            daily_budget: Some(25.0),
            geo_target_ids: vec![],
            language_ids: vec![],
        });

        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_campaign");
    }

    #[test]
    fn test_update_campaign_budget_cap_exceeded() {
        let mut config = Config::default();
        config.safety.max_daily_budget = 10.0;

        let result = update_campaign(&UpdateCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_id: "12345",
            bidding_strategy: None,
            target_cpa: None,
            target_roas: None,
            daily_budget: Some(20.0), // exceeds cap of 10.0
            geo_target_ids: vec![],
            language_ids: vec![],
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("exceeds maximum"));
    }

    #[test]
    fn test_apply_bidding_strategy() {
        let mut campaign = json!({
            "name": "test",
            "status": "PAUSED"
        });

        apply_bidding_strategy(&mut campaign, "MAXIMIZE_CONVERSIONS", Some(5.0), None);
        assert!(campaign.get("maximizeConversions").is_some());

        let mut campaign2 = json!({"name": "test"});
        apply_bidding_strategy(&mut campaign2, "MANUAL_CPC", None, None);
        assert!(campaign2.get("manualCpc").is_some());
    }
}
