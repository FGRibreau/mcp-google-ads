use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::Device;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Google accepts a bid modifier between -90 % and +900 % (0.1 to 10.0).
pub const BID_MODIFIER_MIN: f64 = 0.1;
pub const BID_MODIFIER_MAX: f64 = 10.0;

/// One bid adjustment of a campaign criterion: a device, or an existing criterion by its ID.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BidModifierInput {
    /// The device to adjust (DESKTOP, MOBILE or TABLET). Exclusive with criterion_id.
    pub device: Option<Device>,
    /// The ID of an existing location or ad schedule criterion of the campaign
    /// (`campaign_criterion.criterion_id`; for a location, its geo target constant ID).
    /// Exclusive with device.
    pub criterion_id: Option<String>,
    /// The bid modifier: 1.2 for +20 %, 0.8 for -20 %, between 0.1 and 10.0;
    /// 0 excludes a device (-100 %).
    pub bid_modifier: f64,
}

/// Draft bid adjustments on a campaign's criteria: devices, locations, ad schedules.
///
/// Each adjustment updates the `bid_modifier` of an existing campaign criterion,
/// `campaignCriteria/{campaign}~{criterion}`: a location or an ad schedule
/// criterion the campaign already targets, or the criterion Google keeps for each
/// device. Whether the campaign's bidding strategy applies the adjustment is left
/// to Google (Smart Bidding ignores all but a device exclusion).
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn set_campaign_bid_modifiers(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    modifiers: &[BidModifierInput],
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_bid_modifiers", &config.safety)?;
    if modifiers.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one bid modifier is required".to_string(),
        ));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let mut criteria = HashSet::new();
    let mut operations = Vec::new();
    let mut summary = Vec::new();
    for modifier in modifiers {
        let criterion = match (&modifier.device, &modifier.criterion_id) {
            (Some(device), None) => device.criterion_id().to_string(),
            (None, Some(id)) if !id.trim().is_empty() && id.chars().all(|c| c.is_ascii_digit()) => {
                id.clone()
            }
            (None, Some(id)) => {
                return Err(McpGoogleAdsError::Validation(format!(
                    "criterion_id must be the numeric ID of a campaign criterion, got '{id}'"
                )))
            }
            _ => {
                return Err(McpGoogleAdsError::Validation(
                    "Each bid modifier names exactly one of device or criterion_id".to_string(),
                ))
            }
        };
        let excludes_device = modifier.device.is_some() && modifier.bid_modifier == 0.0;
        if !excludes_device
            && !(BID_MODIFIER_MIN..=BID_MODIFIER_MAX).contains(&modifier.bid_modifier)
        {
            return Err(McpGoogleAdsError::Validation(format!(
                "bid_modifier {} is out of [{BID_MODIFIER_MIN}, {BID_MODIFIER_MAX}] (0 only excludes a device)",
                modifier.bid_modifier
            )));
        }
        if !criteria.insert(criterion.clone()) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Criterion {criterion} is adjusted twice"
            )));
        }
        operations.push(json!({
            "campaignCriterionOperation": {
                "update": {
                    "resourceName": format!("customers/{}/campaignCriteria/{}~{}", cid, campaign_id, criterion),
                    "bidModifier": modifier.bid_modifier
                },
                "updateMask": "bidModifier"
            }
        }));
        summary.push(json!({
            "device": modifier.device.map(|d| d.as_api_str()),
            "criterion_id": criterion,
            "bid_modifier": modifier.bid_modifier,
        }));
    }

    let plan = ChangePlan::new(
        "set_campaign_bid_modifiers".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
        cid,
        json!({"campaign_id": campaign_id, "bid_modifiers": summary}),
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Draft removing a targeted language from a campaign.
///
/// A language criterion's ID is its language constant ID, so the criterion is
/// `campaignCriteria/{campaign}~{language}`. `language_id` accepts the bare
/// numeric ID ("1002") or the resource name ("languageConstants/1002"). Like the
/// other removals it requires double confirmation.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn remove_language_target(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    language_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_language_target", &config.safety)?;

    let language = language_id.trim().trim_start_matches("languageConstants/");
    if language.is_empty() || !language.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "language_id must be a numeric language constant ID (e.g. '1002' or \
             'languageConstants/1002'), got '{language_id}'"
        )));
    }
    let cid = GoogleAdsClient::normalize_customer_id(customer_id);

    let operations = vec![json!({
        "campaignCriterionOperation": {
            "remove": format!("customers/{}/campaignCriteria/{}~{}", cid, campaign_id, language)
        }
    })];
    let changes = json!({
        "campaign_id": campaign_id,
        "language_id": language,
        "removed_criterion": format!("{}~{}", campaign_id, language),
        "warning": "Removes the campaign's language target. Reversible by re-adding it.",
    });

    let plan = ChangePlan::new(
        "remove_language_target".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}
