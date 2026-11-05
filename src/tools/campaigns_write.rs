use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::McpGoogleAdsError;
use crate::error::Result;
use crate::models::{
    to_micros, AdStatus, GeoTargetType, NextActionHint, TargetImpressionShareLocation,
};
use crate::safety::guards::{
    check_blocked_operation, check_budget_cap, validate_campaign_date_times, validate_final_url,
};
use crate::safety::preview::{store_plan, ChangePlan};

/// The networks of a Search campaign (`Campaign.network_settings`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkSettingsInput {
    /// Google Search.
    pub target_google_search: bool,
    /// Google search partners.
    pub target_search_network: bool,
    /// Display Network expansion.
    pub target_content_network: bool,
}

/// Parameters for drafting a new campaign.
///
/// `status` defaults to [`AdStatus::Paused`] when `None` — newly drafted
/// campaigns ship paused for safety. The chosen status is echoed back in
/// the response as `status_after_apply`; when paused, a `next_action_hint`
/// also describes how to enable the campaign via the `enable_entity` MCP
/// tool (zero UI action required).
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
    /// Lifecycle status to create the campaign in. `None` -> default `PAUSED`.
    pub status: Option<AdStatus>,
    /// Start of delivery, `yyyy-MM-dd HH:mm:ss` in the account time zone. `None` -> Google's default (today).
    pub start_date_time: Option<&'a str>,
    /// End of delivery, `yyyy-MM-dd HH:mm:ss` in the account time zone. `None` -> no end.
    pub end_date_time: Option<&'a str>,
    /// Maximum CPC bid, for TARGET_SPEND / MAXIMIZE_CLICKS or TARGET_IMPRESSION_SHARE.
    pub cpc_bid_ceiling: Option<f64>,
    /// Page location, for TARGET_IMPRESSION_SHARE.
    pub target_impression_share_location: Option<TargetImpressionShareLocation>,
    /// Target impression share in percent, for TARGET_IMPRESSION_SHARE.
    pub target_impression_share_percent: Option<f64>,
    /// Networks of a SEARCH campaign. `None` -> Google Search only.
    pub network_settings: Option<NetworkSettingsInput>,
    /// Whether to create the default ad group (and its keywords) with the campaign.
    /// `false` creates the campaign alone; its ad groups are created with `create_ad_group`.
    pub with_ad_group: bool,
}

impl DraftCampaignParams<'_> {
    fn bidding_values(&self) -> BiddingValues {
        BiddingValues {
            target_cpa: self.target_cpa,
            target_roas: self.target_roas,
            cpc_bid_ceiling: self.cpc_bid_ceiling,
            impression_share_location: self.target_impression_share_location,
            impression_share_percent: self.target_impression_share_percent,
        }
    }
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
    validate_campaign_date_times(params.start_date_time, params.end_date_time)?;
    if params.network_settings.is_some() && params.channel_type != "SEARCH" {
        return Err(crate::error::McpGoogleAdsError::Validation(format!(
            "network_settings only apply to a SEARCH campaign, not {}",
            params.channel_type
        )));
    }
    if !params.with_ad_group && !params.keywords.is_empty() {
        return Err(crate::error::McpGoogleAdsError::Validation(
            "keywords belong to the default ad group: pass with_ad_group=true, or add them \
             with draft_keywords once the ad group is created"
                .to_string(),
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
                "amountMicros": to_micros(params.daily_budget).to_string(),
                "deliveryMethod": "STANDARD",
                // The API defaults explicitly_shared to TRUE, and campaign-level
                // Smart Bidding (e.g. maximizeConversions) is rejected on shared
                // budgets with BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET.
                // This budget is created for exactly one campaign, so make it
                // non-shared.
                "explicitlyShared": false,
                "resourceName": budget_resource
            }
        }
    }));

    // 2. Campaign (temp resource ID -2)
    let campaign_resource = format!("customers/{}/campaigns/-2", cid);
    let campaign_status = params.status.unwrap_or_default();
    let mut campaign_create = json!({
        "name": params.campaign_name,
        "status": campaign_status.as_api_str(),
        "advertisingChannelType": params.channel_type,
        "campaignBudget": budget_resource,
        "resourceName": campaign_resource,
        // Required by EU TTPA regulation (Oct 2025+). Defaults to false; users
        // running political ads must change this in the UI after creation.
        "containsEuPoliticalAdvertising": "DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING"
    });

    if let Some(start) = params.start_date_time {
        campaign_create["startDateTime"] = json!(start);
    }
    if let Some(end) = params.end_date_time {
        campaign_create["endDateTime"] = json!(end);
    }

    let network_settings = match params.network_settings {
        Some(networks) => Some(json!({
            "targetGoogleSearch": networks.target_google_search,
            "targetSearchNetwork": networks.target_search_network,
            "targetContentNetwork": networks.target_content_network,
            "targetPartnerSearchNetwork": false
        })),
        None => network_settings_for_channel(params.channel_type),
    };
    if let Some(network_settings) = network_settings {
        campaign_create
            .as_object_mut()
            .map(|o| o.insert("networkSettings".to_string(), network_settings));
    }

    // Apply bidding strategy
    apply_bidding_strategy(
        &mut campaign_create,
        params.bidding_strategy,
        &params.bidding_values(),
    )?;

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

    if params.with_ad_group {
        // 5. Ad group (temp resource ID -3)
        let ad_group_resource = format!("customers/{}/adGroups/-3", cid);
        let ad_group_type = match params.channel_type {
            "SEARCH" => "SEARCH_STANDARD",
            "DISPLAY" => "DISPLAY_STANDARD",
            _ => "SEARCH_STANDARD",
        };
        // Ad group inherits the campaign-level status — keeping both PAUSED
        // (or both ENABLED) avoids a half-running campaign state.
        operations.push(json!({
            "adGroupOperation": {
                "create": {
                    "name": params.ad_group_name,
                    "campaign": campaign_resource,
                    "status": campaign_status.as_api_str(),
                    "type": ad_group_type,
                    "resourceName": ad_group_resource
                }
            }
        }));

        // 6. Keywords (optional)
        for kw in &params.keywords {
            let mut criterion = json!({
                "adGroup": ad_group_resource,
                "keyword": {
                    "text": kw.text,
                    "matchType": kw.match_type
                }
            });
            if let Some(ref url) = kw.final_url {
                validate_final_url(url)?;
                if let Some(obj) = criterion.as_object_mut() {
                    obj.insert("finalUrls".to_string(), json!([url]));
                }
            }
            operations.push(json!({
                "adGroupCriterionOperation": {
                    "create": criterion
                }
            }));
        }
    }

    let changes = json!({
        "campaign_name": params.campaign_name,
        "daily_budget": params.daily_budget,
        "bidding_strategy": params.bidding_strategy,
        "target_cpa": params.target_cpa,
        "target_roas": params.target_roas,
        "channel_type": params.channel_type,
        "ad_group_name": params.with_ad_group.then_some(params.ad_group_name),
        "keywords_count": params.keywords.len(),
        "network_settings": params.network_settings.map(|n| json!({
            "target_google_search": n.target_google_search,
            "target_search_network": n.target_search_network,
            "target_content_network": n.target_content_network,
        })),
        "geo_targets": params.geo_target_ids,
        "languages": params.language_ids,
        "status": campaign_status.as_api_str(),
        "start_date_time": params.start_date_time,
        "end_date_time": params.end_date_time,
        "cpc_bid_ceiling": params.cpc_bid_ceiling,
        "target_impression_share_location": params.target_impression_share_location.map(|l| l.as_api_str()),
        "target_impression_share_percent": params.target_impression_share_percent
    });

    let mut plan = ChangePlan::new(
        "draft_campaign".to_string(),
        "campaign".to_string(),
        "new".to_string(),
        cid.clone(),
        changes,
        false,
        operations,
    )
    .with_status_after_apply(campaign_status);

    if campaign_status == AdStatus::Paused {
        plan = plan.with_next_action_hint(NextActionHint::enable_parent_campaign_after_create(
            "customers/{cid}/campaigns/-2",
        ));
    }

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
    /// Full resource name of the campaign's budget
    /// (`customers/{cid}/campaignBudgets/{budgetId}`). Required when
    /// `daily_budget` is set — campaign budgets have their own IDs, distinct
    /// from the campaign ID, so the caller must resolve it first via
    /// [`resolve_campaign_budget_resource`].
    pub budget_resource_name: Option<&'a str>,
    pub geo_target_ids: Vec<String>,
    pub language_ids: Vec<String>,
    /// New start of delivery, `yyyy-MM-dd HH:mm:ss` in the account time zone.
    pub start_date_time: Option<&'a str>,
    /// New end of delivery, `yyyy-MM-dd HH:mm:ss` in the account time zone.
    pub end_date_time: Option<&'a str>,
    /// Maximum CPC bid, for TARGET_SPEND / MAXIMIZE_CLICKS or TARGET_IMPRESSION_SHARE.
    pub cpc_bid_ceiling: Option<f64>,
    /// Page location, for TARGET_IMPRESSION_SHARE.
    pub target_impression_share_location: Option<TargetImpressionShareLocation>,
    /// Target impression share in percent, for TARGET_IMPRESSION_SHARE.
    pub target_impression_share_percent: Option<f64>,
}

impl UpdateCampaignParams<'_> {
    fn bidding_values(&self) -> BiddingValues {
        BiddingValues {
            target_cpa: self.target_cpa,
            target_roas: self.target_roas,
            cpc_bid_ceiling: self.cpc_bid_ceiling,
            impression_share_location: self.target_impression_share_location,
            impression_share_percent: self.target_impression_share_percent,
        }
    }
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

        // A campaign budget has its own resource ID, distinct from the campaign
        // ID. The caller must resolve it (via `resolve_campaign_budget_resource`)
        // and pass it in — reusing the campaign ID here would target a
        // non-existent budget and fail with RESOURCE_NOT_FOUND.
        let budget_resource = params.budget_resource_name.ok_or_else(|| {
            crate::error::McpGoogleAdsError::Validation(
                "daily_budget update requires the campaign's budget resource name; \
                 resolve it first via resolve_campaign_budget_resource"
                    .to_string(),
            )
        })?;

        changes.insert("daily_budget".to_string(), json!(budget));
        // Note: campaign budgets can be shared across campaigns; updating the
        // amount affects every campaign that uses this budget.
        operations.push(json!({
            "campaignBudgetOperation": {
                "update": {
                    "resourceName": budget_resource,
                    "amountMicros": to_micros(budget).to_string()
                },
                "updateMask": "amountMicros"
            }
        }));
    }

    validate_campaign_date_times(params.start_date_time, params.end_date_time)?;

    // Campaign fields (bidding strategy, dates), sent as one update with one mask.
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, params.campaign_id);
    let mut campaign_update = json!({
        "resourceName": campaign_resource
    });
    let mut update_mask_fields: Vec<String> = Vec::new();

    // Bidding strategy update
    if let Some(strategy) = params.bidding_strategy {
        changes.insert("bidding_strategy".to_string(), json!(strategy));

        apply_bidding_strategy_update(
            &mut campaign_update,
            &mut update_mask_fields,
            strategy,
            &params.bidding_values(),
        )?;

        if let Some(cpa) = params.target_cpa {
            changes.insert("target_cpa".to_string(), json!(cpa));
        }
        if let Some(roas) = params.target_roas {
            changes.insert("target_roas".to_string(), json!(roas));
        }
        if let Some(ceiling) = params.cpc_bid_ceiling {
            changes.insert("cpc_bid_ceiling".to_string(), json!(ceiling));
        }
        if let Some(location) = params.target_impression_share_location {
            changes.insert(
                "target_impression_share_location".to_string(),
                json!(location.as_api_str()),
            );
        }
        if let Some(percent) = params.target_impression_share_percent {
            changes.insert(
                "target_impression_share_percent".to_string(),
                json!(percent),
            );
        }
    }

    // Start and end of delivery
    if let Some(start) = params.start_date_time {
        changes.insert("start_date_time".to_string(), json!(start));
        campaign_update["startDateTime"] = json!(start);
        update_mask_fields.push("startDateTime".to_string());
    }
    if let Some(end) = params.end_date_time {
        changes.insert("end_date_time".to_string(), json!(end));
        campaign_update["endDateTime"] = json!(end);
        update_mask_fields.push("endDateTime".to_string());
    }

    if params.bidding_strategy.is_some() || !update_mask_fields.is_empty() {
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

/// Resolve a campaign's budget resource name by querying its `campaign_budget`.
///
/// Campaign budgets have their own resource IDs, distinct from the campaign ID,
/// so a daily-budget update must target the real budget resource. Runs
/// `SELECT campaign.campaign_budget FROM campaign WHERE campaign.id = {id}`
/// and returns the `customers/{cid}/campaignBudgets/{budgetId}` resource name.
pub async fn resolve_campaign_budget_resource(
    client: &GoogleAdsClient,
    customer_id: &str,
    campaign_id: &str,
) -> Result<String> {
    let query = format!(
        "SELECT campaign.campaign_budget FROM campaign WHERE campaign.id = {}",
        campaign_id
    );
    let rows = client.search(customer_id, &query).await?;

    rows.first()
        .and_then(|row| row.get("campaign"))
        .and_then(|c| c.get("campaignBudget"))
        .and_then(|b| b.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            crate::error::McpGoogleAdsError::Validation(format!(
                "Could not resolve a campaign budget for campaign {} — \
                 the campaign may not exist or has no associated budget",
                campaign_id
            ))
        })
}

/// Set a campaign's geo target type setting (`campaign.geo_target_type_setting`).
///
/// Sets `positive_geo_target_type` and/or `negative_geo_target_type` via a
/// `campaignOperation.update` with a matching field mask. At least one of the
/// two must be provided; otherwise the call fails with a validation error
/// (mirroring `update_campaign`'s "no changes" guard).
///
/// The common use is switching a campaign to `PRESENCE` so ads serve only to
/// people physically in (or regularly in) the targeted locations, instead of
/// the API default `PRESENCE_OR_INTEREST` which also serves people who merely
/// show interest in them.
///
/// Returns a [`ChangePlan`] preview to confirm via `confirm_and_apply`.
pub fn set_campaign_geo_target_type(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    positive_geo_target_type: Option<GeoTargetType>,
    negative_geo_target_type: Option<GeoTargetType>,
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_geo_target_type", &config.safety)?;

    if positive_geo_target_type.is_none() && negative_geo_target_type.is_none() {
        return Err(crate::error::McpGoogleAdsError::Validation(
            "No geo target type specified; set positive_geo_target_type and/or \
             negative_geo_target_type"
                .to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let mut setting = serde_json::Map::new();
    let mut update_mask_fields: Vec<String> = Vec::new();
    let mut changes = serde_json::Map::new();

    if let Some(pos) = positive_geo_target_type {
        setting.insert("positiveGeoTargetType".to_string(), json!(pos.as_api_str()));
        update_mask_fields.push("geoTargetTypeSetting.positiveGeoTargetType".to_string());
        changes.insert(
            "positive_geo_target_type".to_string(),
            json!(pos.as_api_str()),
        );
    }
    if let Some(neg) = negative_geo_target_type {
        setting.insert("negativeGeoTargetType".to_string(), json!(neg.as_api_str()));
        update_mask_fields.push("geoTargetTypeSetting.negativeGeoTargetType".to_string());
        changes.insert(
            "negative_geo_target_type".to_string(),
            json!(neg.as_api_str()),
        );
    }

    let operations = vec![json!({
        "campaignOperation": {
            "update": {
                "resourceName": campaign_resource,
                "geoTargetTypeSetting": serde_json::Value::Object(setting)
            },
            "updateMask": update_mask_fields.join(",")
        }
    })];

    let plan = ChangePlan::new(
        "set_campaign_geo_target_type".to_string(),
        "campaign".to_string(),
        campaign_id.to_string(),
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
    /// Optional keyword-level final URL override. When set, clicks on this
    /// keyword route to this landing page (`ad_group_criterion.final_urls`)
    /// instead of inheriting the ad's final URL. Must be an absolute http(s)
    /// URL. Omit to inherit the ad's final URL.
    pub final_url: Option<String>,
}

/// Draft changing the networks of a Search campaign (`Campaign.network_settings`).
///
/// Each network given is set and masked; a network left out keeps its state. At
/// least one must be given.
///
/// Returns a [`ChangePlan`] preview to confirm via `confirm_and_apply`.
pub fn set_campaign_network_settings(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    target_google_search: Option<bool>,
    target_search_network: Option<bool>,
    target_content_network: Option<bool>,
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_network_settings", &config.safety)?;

    let networks = [
        (
            "targetGoogleSearch",
            "target_google_search",
            target_google_search,
        ),
        (
            "targetSearchNetwork",
            "target_search_network",
            target_search_network,
        ),
        (
            "targetContentNetwork",
            "target_content_network",
            target_content_network,
        ),
    ];
    let mut settings = serde_json::Map::new();
    let mut update_mask_fields: Vec<String> = Vec::new();
    let mut changes = serde_json::Map::new();
    for (api_field, param, value) in networks {
        if let Some(value) = value {
            settings.insert(api_field.to_string(), json!(value));
            update_mask_fields.push(format!("networkSettings.{api_field}"));
            changes.insert(param.to_string(), json!(value));
        }
    }
    if update_mask_fields.is_empty() {
        return Err(crate::error::McpGoogleAdsError::Validation(
            "No network specified; set target_google_search, target_search_network and/or \
             target_content_network"
                .to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let operations = vec![json!({
        "campaignOperation": {
            "update": {
                "resourceName": format!("customers/{}/campaigns/{}", cid, campaign_id),
                "networkSettings": serde_json::Value::Object(settings)
            },
            "updateMask": update_mask_fields.join(",")
        }
    })];

    let plan = ChangePlan::new(
        "set_campaign_network_settings".to_string(),
        "campaign".to_string(),
        campaign_id.to_string(),
        cid,
        serde_json::Value::Object(changes),
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Build the `networkSettings` payload appropriate for an advertising channel.
///
/// The settings must match the channel: a DISPLAY campaign with
/// `targetGoogleSearch: true` is rejected with
/// OPERATION_NOT_PERMITTED_FOR_CONTEXT on `network_settings.target_google_search`.
///
/// - SEARCH: Google Search only. Search partners and Display expansion are
///   deliberately off — both default to opt-in spend that most advertisers
///   don't want; they can be enabled later via the UI or an update.
/// - DISPLAY: Display Network only.
/// - Anything else (VIDEO, PERFORMANCE_MAX, SHOPPING, …): `None` — omit the
///   field and let the API infer the valid networks for the channel.
fn network_settings_for_channel(channel_type: &str) -> Option<serde_json::Value> {
    match channel_type {
        "SEARCH" => Some(json!({
            "targetGoogleSearch": true,
            "targetSearchNetwork": false,
            "targetContentNetwork": false,
            "targetPartnerSearchNetwork": false
        })),
        "DISPLAY" => Some(json!({
            "targetGoogleSearch": false,
            "targetSearchNetwork": false,
            "targetContentNetwork": true,
            "targetPartnerSearchNetwork": false
        })),
        _ => None,
    }
}

/// The values a campaign bidding strategy may carry, all in account currency or natural units.
#[derive(Debug, Clone, Copy, Default)]
pub struct BiddingValues {
    /// Target CPA, for TARGET_CPA (required) or MAXIMIZE_CONVERSIONS (optional).
    pub target_cpa: Option<f64>,
    /// Target ROAS as a ratio, for TARGET_ROAS (required) or MAXIMIZE_CONVERSION_VALUE (optional).
    pub target_roas: Option<f64>,
    /// Maximum CPC bid, for TARGET_SPEND / MAXIMIZE_CLICKS or TARGET_IMPRESSION_SHARE (optional).
    pub cpc_bid_ceiling: Option<f64>,
    /// Page location, for TARGET_IMPRESSION_SHARE (required).
    pub impression_share_location: Option<TargetImpressionShareLocation>,
    /// Target impression share in percent (e.g. 80.0), for TARGET_IMPRESSION_SHARE (required).
    pub impression_share_percent: Option<f64>,
}

/// The values each strategy requires, and those it may carry.
fn strategy_values(strategy: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match strategy {
        "MAXIMIZE_CONVERSIONS" => (&[], &["target_cpa"]),
        "MAXIMIZE_CONVERSION_VALUE" => (&[], &["target_roas"]),
        "TARGET_CPA" => (&["target_cpa"], &[]),
        "TARGET_ROAS" => (&["target_roas"], &[]),
        "TARGET_SPEND" | "MAXIMIZE_CLICKS" => (&[], &["cpc_bid_ceiling"]),
        "TARGET_IMPRESSION_SHARE" => (
            &[
                "target_impression_share_location",
                "target_impression_share_percent",
            ],
            &["cpc_bid_ceiling"],
        ),
        "MANUAL_CPC" | "PERCENT_CPC" => (&[], &[]),
        _ => return None,
    })
}

/// Check that `values` carries exactly what `strategy` takes: its required values, and no value it ignores.
///
/// A value the strategy does not use used to be dropped silently; it is rejected
/// so that a caller never believes a target was written when it was not.
pub fn check_bidding_values(strategy: &str, values: &BiddingValues) -> Result<()> {
    let (required, optional) = strategy_values(strategy).ok_or_else(|| {
        McpGoogleAdsError::Validation(format!(
            "Unknown bidding strategy '{strategy}': expected MAXIMIZE_CONVERSIONS, \
             MAXIMIZE_CONVERSION_VALUE, TARGET_CPA, TARGET_ROAS, TARGET_SPEND (MAXIMIZE_CLICKS), \
             TARGET_IMPRESSION_SHARE, MANUAL_CPC or PERCENT_CPC"
        ))
    })?;
    let given = [
        ("target_cpa", values.target_cpa.is_some()),
        ("target_roas", values.target_roas.is_some()),
        ("cpc_bid_ceiling", values.cpc_bid_ceiling.is_some()),
        (
            "target_impression_share_location",
            values.impression_share_location.is_some(),
        ),
        (
            "target_impression_share_percent",
            values.impression_share_percent.is_some(),
        ),
    ];
    for (name, is_set) in given {
        if required.contains(&name) && !is_set {
            return Err(McpGoogleAdsError::Validation(format!(
                "{strategy} requires {name}"
            )));
        }
        if is_set && !required.contains(&name) && !optional.contains(&name) {
            return Err(McpGoogleAdsError::Validation(format!(
                "{strategy} takes no {name}"
            )));
        }
    }
    if let Some(percent) = values.impression_share_percent {
        if !(percent > 0.0 && percent <= 100.0) {
            return Err(McpGoogleAdsError::Validation(format!(
                "target_impression_share_percent must be in (0, 100], got {percent}"
            )));
        }
    }
    for (name, amount) in [
        ("target_cpa", values.target_cpa),
        ("target_roas", values.target_roas),
        ("cpc_bid_ceiling", values.cpc_bid_ceiling),
    ] {
        if matches!(amount, Some(v) if v <= 0.0) {
            return Err(McpGoogleAdsError::Validation(format!(
                "{name} must be greater than 0"
            )));
        }
    }
    Ok(())
}

/// `locationFractionMicros` of a percentage: 1 % is 10,000 micros.
fn percent_to_fraction_micros(percent: f64) -> i64 {
    (percent * 10_000.0).round() as i64
}

/// The bidding scheme field of `strategy` and its JSON value, with every value given set.
///
/// A leaf that is not given is left out at creation, and sent as `0` (its API
/// "unset" value) on update, so that masking it clears a previous value.
fn bidding_scheme(
    strategy: &str,
    values: &BiddingValues,
    for_update: bool,
) -> (&'static str, serde_json::Value) {
    let micros = |amount: Option<f64>| -> Option<String> {
        match amount {
            Some(v) => Some(to_micros(v).to_string()),
            None if for_update => Some("0".to_string()),
            None => None,
        }
    };
    let mut scheme = serde_json::Map::new();
    let field = match strategy {
        "MAXIMIZE_CONVERSIONS" => {
            if let Some(cpa) = micros(values.target_cpa) {
                scheme.insert("targetCpaMicros".to_string(), json!(cpa));
            }
            "maximizeConversions"
        }
        "MAXIMIZE_CONVERSION_VALUE" => {
            match values.target_roas {
                Some(roas) => scheme.insert("targetRoas".to_string(), json!(roas)),
                None if for_update => scheme.insert("targetRoas".to_string(), json!(0.0)),
                None => None,
            };
            "maximizeConversionValue"
        }
        "TARGET_CPA" => {
            if let Some(cpa) = micros(values.target_cpa) {
                scheme.insert("targetCpaMicros".to_string(), json!(cpa));
            }
            "targetCpa"
        }
        "TARGET_ROAS" => {
            if let Some(roas) = values.target_roas {
                scheme.insert("targetRoas".to_string(), json!(roas));
            }
            "targetRoas"
        }
        "TARGET_SPEND" | "MAXIMIZE_CLICKS" => {
            if let Some(ceiling) = micros(values.cpc_bid_ceiling) {
                scheme.insert("cpcBidCeilingMicros".to_string(), json!(ceiling));
            }
            "targetSpend"
        }
        "TARGET_IMPRESSION_SHARE" => {
            if let Some(location) = values.impression_share_location {
                scheme.insert("location".to_string(), json!(location.as_api_str()));
            }
            if let Some(percent) = values.impression_share_percent {
                scheme.insert(
                    "locationFractionMicros".to_string(),
                    json!(percent_to_fraction_micros(percent).to_string()),
                );
            }
            if let Some(ceiling) = micros(values.cpc_bid_ceiling) {
                scheme.insert("cpcBidCeilingMicros".to_string(), json!(ceiling));
            }
            "targetImpressionShare"
        }
        "MANUAL_CPC" => {
            if for_update {
                // Enhanced CPC is sunset; false is the neutral value, and masking it
                // is what puts the campaign on Manual CPC.
                scheme.insert("enhancedCpcEnabled".to_string(), json!(false));
            }
            "manualCpc"
        }
        // PERCENT_CPC, the only other strategy `check_bidding_values` accepts.
        _ => "percentCpc",
    };
    (field, serde_json::Value::Object(scheme))
}

/// Apply a bidding strategy to a campaign create JSON value.
///
/// Note: in Google Ads API v25, `bidding_strategy_type` on Campaign is
/// OUTPUT_ONLY. To use a standard bidding strategy at create time, set the
/// corresponding bidding sub-field (e.g. `manualCpc`, `targetSpend`,
/// `maximizeConversions`). The enum is then computed by the server.
fn apply_bidding_strategy(
    campaign: &mut serde_json::Value,
    strategy: &str,
    values: &BiddingValues,
) -> Result<()> {
    check_bidding_values(strategy, values)?;
    let (field, scheme) = bidding_scheme(strategy, values, false);
    campaign[field] = scheme;
    Ok(())
}

/// Apply bidding strategy fields for an update operation, tracking which fields go into updateMask.
///
/// The mask must name *leaf* fields. Every bidding scheme (`manualCpc`,
/// `maximizeConversions`, ...) is a message with subfields, and masking the
/// message itself is rejected with `FIELD_HAS_SUBFIELDS`. So each leaf of the
/// scheme is masked — that is enough for the server to switch the campaign onto
/// that scheme, and an optional leaf left out is sent as `0`, which clears it
/// (e.g. MAXIMIZE_CONVERSIONS with no `target_cpa` means "no target CPA").
fn apply_bidding_strategy_update(
    campaign: &mut serde_json::Value,
    update_mask_fields: &mut Vec<String>,
    strategy: &str,
    values: &BiddingValues,
) -> Result<()> {
    check_bidding_values(strategy, values)?;
    let (field, scheme) = bidding_scheme(strategy, values, true);
    if let Some(leaves) = scheme.as_object() {
        update_mask_fields.extend(leaves.keys().map(|leaf| format!("{field}.{leaf}")));
    }
    campaign[field] = scheme;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
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
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
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
                final_url: None,
            }],
            geo_target_ids: vec!["2840".to_string()],
            language_ids: vec!["1000".to_string()],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
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
            budget_resource_name: None,
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
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
            budget_resource_name: Some("customers/1234567890/campaignBudgets/987654321"),
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
        });

        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "update_campaign");

        // The budget operation must target the resolved budget resource — NOT a
        // resource built from the campaign ID (issue #5 regression guard).
        let plan_id = preview["plan_id"].as_str().unwrap_or_default();
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");
        let budget_op = plan
            .mutate_operations
            .iter()
            .find(|op| op.get("campaignBudgetOperation").is_some())
            .expect("budget operation present");
        let resource_name = budget_op["campaignBudgetOperation"]["update"]["resourceName"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(
            resource_name,
            "customers/1234567890/campaignBudgets/987654321"
        );
        assert_ne!(resource_name, "customers/1234567890/campaignBudgets/12345");
    }

    #[test]
    fn test_update_campaign_budget_requires_resolved_resource_name() {
        // Setting daily_budget without resolving the budget resource name must
        // fail loudly instead of silently building a wrong resource (issue #5).
        let config = Config::default();

        let result = update_campaign(&UpdateCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_id: "12345",
            bidding_strategy: None,
            target_cpa: None,
            target_roas: None,
            daily_budget: Some(25.0),
            budget_resource_name: None,
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("budget resource name"));
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
            budget_resource_name: Some("customers/1234567890/campaignBudgets/987654321"),
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
        });

        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("exceeds maximum"));
    }

    #[test]
    fn test_draft_campaign_default_status_paused_with_hint() {
        let config = Config::default();
        let preview = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "C1",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "AG1",
            keywords: vec![],
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
        })
        .unwrap();
        assert_eq!(preview["status_after_apply"], "PAUSED");
        assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
    }

    #[test]
    fn test_draft_campaign_explicit_enabled_no_hint() {
        let config = Config::default();
        let preview = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "C1",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "AG1",
            keywords: vec![],
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: Some(AdStatus::Enabled),
        })
        .unwrap();
        assert_eq!(preview["status_after_apply"], "ENABLED");
        assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());
    }

    /// Draft a campaign with the given channel type and return the stored
    /// plan's mutate operations for payload inspection.
    fn draft_ops_for_channel(channel_type: &str) -> Vec<serde_json::Value> {
        let config = Config::default();
        let preview = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "123-456-7890",
            campaign_name: "Payload Test",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type,
            ad_group_name: "AG",
            keywords: vec![],
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
        })
        .unwrap();
        let plan_id = preview["plan_id"].as_str().unwrap_or_default();
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");
        plan.mutate_operations.clone()
    }

    #[test]
    fn test_draft_campaign_budget_not_shared() {
        // The API defaults explicitly_shared to TRUE, which breaks campaign-level
        // Smart Bidding (BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET).
        let ops = draft_ops_for_channel("SEARCH");
        let budget_create = ops
            .iter()
            .find_map(|op| op.pointer("/campaignBudgetOperation/create"))
            .expect("budget operation present");
        assert_eq!(budget_create["explicitlyShared"], json!(false));
    }

    #[test]
    fn test_draft_campaign_search_network_settings() {
        let ops = draft_ops_for_channel("SEARCH");
        let ns = ops
            .iter()
            .find_map(|op| op.pointer("/campaignOperation/create/networkSettings"))
            .expect("networkSettings present for SEARCH");
        assert_eq!(ns["targetGoogleSearch"], json!(true));
        assert_eq!(ns["targetContentNetwork"], json!(false));
    }

    #[test]
    fn test_draft_campaign_display_network_settings() {
        // A DISPLAY campaign with targetGoogleSearch=true is rejected with
        // OPERATION_NOT_PERMITTED_FOR_CONTEXT on network_settings.target_google_search.
        let ops = draft_ops_for_channel("DISPLAY");
        let ns = ops
            .iter()
            .find_map(|op| op.pointer("/campaignOperation/create/networkSettings"))
            .expect("networkSettings present for DISPLAY");
        assert_eq!(ns["targetGoogleSearch"], json!(false));
        assert_eq!(ns["targetSearchNetwork"], json!(false));
        assert_eq!(ns["targetContentNetwork"], json!(true));
    }

    #[test]
    fn test_draft_campaign_other_channels_omit_network_settings() {
        for channel in ["VIDEO", "PERFORMANCE_MAX", "SHOPPING"] {
            let ops = draft_ops_for_channel(channel);
            let create = ops
                .iter()
                .find_map(|op| op.pointer("/campaignOperation/create"))
                .expect("campaign operation present");
            assert!(
                create.get("networkSettings").is_none(),
                "networkSettings should be omitted for {}",
                channel
            );
        }
    }

    #[test]
    fn test_network_settings_for_channel() {
        assert!(network_settings_for_channel("SEARCH").is_some());
        assert!(network_settings_for_channel("DISPLAY").is_some());
        assert!(network_settings_for_channel("VIDEO").is_none());
        assert!(network_settings_for_channel("PERFORMANCE_MAX").is_none());
    }

    #[test]
    fn test_apply_bidding_strategy() {
        let mut campaign = json!({
            "name": "test",
            "status": "PAUSED"
        });

        apply_bidding_strategy(
            &mut campaign,
            "MAXIMIZE_CONVERSIONS",
            &BiddingValues {
                target_cpa: Some(5.0),
                target_roas: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(campaign.get("maximizeConversions").is_some());

        let mut campaign2 = json!({"name": "test"});
        apply_bidding_strategy(
            &mut campaign2,
            "MANUAL_CPC",
            &BiddingValues {
                target_cpa: None,
                target_roas: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(campaign2.get("manualCpc").is_some());
    }

    /// Every bidding scheme is a message with subfields, so the updateMask must
    /// name a leaf. Masking the scheme itself is rejected by the API with
    /// `FIELD_HAS_SUBFIELDS`.
    #[test]
    fn test_bidding_strategy_update_masks_are_leaf_fields() {
        let cases = [
            ("MANUAL_CPC", None, None, "manualCpc.enhancedCpcEnabled"),
            (
                "MAXIMIZE_CONVERSIONS",
                None,
                None,
                "maximizeConversions.targetCpaMicros",
            ),
            (
                "MAXIMIZE_CONVERSIONS",
                Some(18.0),
                None,
                "maximizeConversions.targetCpaMicros",
            ),
            (
                "MAXIMIZE_CONVERSION_VALUE",
                None,
                None,
                "maximizeConversionValue.targetRoas",
            ),
            ("TARGET_CPA", Some(18.0), None, "targetCpa.targetCpaMicros"),
            ("TARGET_ROAS", None, Some(3.0), "targetRoas.targetRoas"),
            (
                "TARGET_SPEND",
                None,
                None,
                "targetSpend.cpcBidCeilingMicros",
            ),
        ];

        for (strategy, cpa, roas, expected_mask) in cases {
            let mut campaign = json!({"resourceName": "customers/1/campaigns/2"});
            let mut mask = Vec::new();
            apply_bidding_strategy_update(
                &mut campaign,
                &mut mask,
                strategy,
                &BiddingValues {
                    target_cpa: cpa,
                    target_roas: roas,
                    ..Default::default()
                },
            )
            .unwrap();

            assert_eq!(mask, vec![expected_mask.to_string()], "strategy {strategy}");
            assert!(
                mask.iter().all(|f| f.contains('.')),
                "{strategy}: mask must name a leaf field, got {mask:?}"
            );
        }
    }

    /// Manual CPC must not be sent as a bare `{}` — the masked leaf needs a value.
    #[test]
    fn test_manual_cpc_update_sets_masked_leaf() {
        let mut campaign = json!({"resourceName": "customers/1/campaigns/2"});
        let mut mask = Vec::new();
        apply_bidding_strategy_update(
            &mut campaign,
            &mut mask,
            "MANUAL_CPC",
            &BiddingValues {
                target_cpa: None,
                target_roas: None,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(campaign["manualCpc"]["enhancedCpcEnabled"], json!(false));
        assert!(campaign.get("biddingStrategyType").is_none());
    }

    /// The masked leaf must carry the caller's value, not merely exist.
    #[test]
    fn test_target_roas_update_sets_masked_value() {
        let mut campaign = json!({"resourceName": "customers/1/campaigns/2"});
        let mut mask = Vec::new();
        apply_bidding_strategy_update(
            &mut campaign,
            &mut mask,
            "TARGET_ROAS",
            &BiddingValues {
                target_roas: Some(17.0),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(campaign["targetRoas"]["targetRoas"], json!(17.0));
        assert_eq!(mask, vec!["targetRoas.targetRoas".to_string()]);
    }

    /// Maximize Conversions with no target still sends a leaf to mask (0 = no target).
    #[test]
    fn test_maximize_conversions_without_target_sends_zero_leaf() {
        let mut campaign = json!({"resourceName": "customers/1/campaigns/2"});
        let mut mask = Vec::new();
        apply_bidding_strategy_update(
            &mut campaign,
            &mut mask,
            "MAXIMIZE_CONVERSIONS",
            &BiddingValues {
                target_cpa: None,
                target_roas: None,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            campaign["maximizeConversions"]["targetCpaMicros"],
            json!("0")
        );
        assert_eq!(
            mask,
            vec!["maximizeConversions.targetCpaMicros".to_string()]
        );
    }

    /// Return the sole `campaignOperation` from a stored plan for inspection.
    fn campaign_update_op(preview: &serde_json::Value) -> serde_json::Value {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");
        plan.mutate_operations
            .iter()
            .find(|op| op.get("campaignOperation").is_some())
            .expect("campaignOperation present")
            .clone()
    }

    #[test]
    fn test_set_geo_target_type_positive_presence() {
        let config = Config::default();
        let preview = set_campaign_geo_target_type(
            &config,
            "123-456-7890",
            "12345",
            Some(GeoTargetType::Presence),
            None,
        )
        .expect("ok");

        assert_eq!(preview["operation"], "set_campaign_geo_target_type");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        assert_eq!(preview["requires_double_confirm"], false);

        let op = campaign_update_op(&preview);
        assert_eq!(
            op["campaignOperation"]["update"]["resourceName"],
            "customers/1234567890/campaigns/12345"
        );
        assert_eq!(
            op["campaignOperation"]["update"]["geoTargetTypeSetting"]["positiveGeoTargetType"],
            "PRESENCE"
        );
        // Only the positive field was set: the negative must be absent.
        assert!(op["campaignOperation"]["update"]["geoTargetTypeSetting"]
            .get("negativeGeoTargetType")
            .is_none());
        assert_eq!(
            op["campaignOperation"]["updateMask"],
            "geoTargetTypeSetting.positiveGeoTargetType"
        );
    }

    #[test]
    fn test_set_geo_target_type_both_fields() {
        let config = Config::default();
        let preview = set_campaign_geo_target_type(
            &config,
            "1234567890",
            "999",
            Some(GeoTargetType::Presence),
            Some(GeoTargetType::PresenceOrInterest),
        )
        .expect("ok");

        let op = campaign_update_op(&preview);
        let setting = &op["campaignOperation"]["update"]["geoTargetTypeSetting"];
        assert_eq!(setting["positiveGeoTargetType"], "PRESENCE");
        assert_eq!(setting["negativeGeoTargetType"], "PRESENCE_OR_INTEREST");
        assert_eq!(
            op["campaignOperation"]["updateMask"],
            "geoTargetTypeSetting.positiveGeoTargetType,geoTargetTypeSetting.negativeGeoTargetType"
        );
    }

    #[test]
    fn test_set_geo_target_type_requires_at_least_one() {
        let config = Config::default();
        let err = set_campaign_geo_target_type(&config, "1234567890", "999", None, None)
            .expect_err("both None must be rejected");
        assert!(err.to_string().contains("No geo target type specified"));
    }

    #[test]
    fn test_set_geo_target_type_blocked_operation() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["set_campaign_geo_target_type".to_string()];
        let err = set_campaign_geo_target_type(
            &config,
            "1234567890",
            "999",
            Some(GeoTargetType::Presence),
            None,
        )
        .expect_err("blocked operation rejected");
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_draft_campaign_keyword_final_url_sets_final_urls() {
        let config = Config::default();
        let preview = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "1234567890",
            campaign_name: "C",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "AG",
            keywords: vec![
                KeywordInput {
                    text: "routed".to_string(),
                    match_type: "EXACT".to_string(),
                    final_url: Some("https://example.com/routed".to_string()),
                },
                KeywordInput {
                    text: "inherit".to_string(),
                    match_type: "PHRASE".to_string(),
                    final_url: None,
                },
            ],
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
        })
        .expect("ok");

        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");
        let criteria: Vec<&serde_json::Value> = plan
            .mutate_operations
            .iter()
            .filter_map(|op| op.pointer("/adGroupCriterionOperation/create"))
            .collect();
        assert_eq!(criteria.len(), 2);

        let routed = criteria
            .iter()
            .find(|c| c["keyword"]["text"] == "routed")
            .expect("routed keyword present");
        assert_eq!(routed["finalUrls"], json!(["https://example.com/routed"]));

        let inherit = criteria
            .iter()
            .find(|c| c["keyword"]["text"] == "inherit")
            .expect("inherit keyword present");
        assert!(inherit.get("finalUrls").is_none());
    }

    #[test]
    fn test_draft_campaign_keyword_invalid_final_url_rejected() {
        let config = Config::default();
        let err = draft_campaign(&DraftCampaignParams {
            config: &config,
            customer_id: "1234567890",
            campaign_name: "C",
            daily_budget: 5.0,
            bidding_strategy: "MAXIMIZE_CONVERSIONS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "AG",
            keywords: vec![KeywordInput {
                text: "bad".to_string(),
                match_type: "EXACT".to_string(),
                final_url: Some("not-a-url".to_string()),
            }],
            geo_target_ids: vec![],
            language_ids: vec![],
            start_date_time: None,
            end_date_time: None,
            cpc_bid_ceiling: None,
            target_impression_share_location: None,
            target_impression_share_percent: None,
            network_settings: None,
            with_ad_group: true,
            status: None,
        })
        .expect_err("invalid final_url rejected");
        assert!(err.to_string().contains("http(s) URL"));
    }
}
