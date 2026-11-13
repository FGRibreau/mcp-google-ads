//! Conversion goals: what a campaign bids on.
//!
//! Google Ads decides which conversions a campaign optimizes for through goals:
//!
//! - `CustomerConversionGoal` — account defaults, one per conversion category ×
//!   origin, each `biddable` or not. Every campaign on account-level goals
//!   follows them. Not written here: changing one changes what every such
//!   campaign bids on.
//! - `CampaignConversionGoal` — the same category × origin goals per campaign.
//!   Updating any of them switches the campaign to campaign-level goals
//!   (`ConversionGoalCampaignConfig.goal_config_level = CAMPAIGN`), after which
//!   account default changes no longer reach it.
//! - `CustomConversionGoal` — a named set of conversion actions. Set on a
//!   campaign's `ConversionGoalCampaignConfig.custom_conversion_goal`, it makes
//!   exactly those actions biddable for that campaign, **including secondary
//!   actions** (`primary_for_goal = false`), which no category goal can do. The
//!   backend switches the campaign to `goal_config_level = CAMPAIGN` itself.
//!
//! A campaign can combine one custom goal with standard goals, so "bid on this
//! one action only" means: the custom goal, plus every standard campaign goal
//! set to `biddable = false`. [`set_campaign_custom_conversion_goal`] does both
//! in one atomic mutate when `exclusive` is on.
//!
//! The goal resources are system-generated or, for custom goals, created here;
//! `CampaignConversionGoal` and `ConversionGoalCampaignConfig` only accept
//! updates.

use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::CampaignConversionGoalInput;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::{validate_id_list, validate_numeric_id, MAX_CRITERIA_PER_PLAN};

/// A standard campaign conversion goal as it exists on a campaign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingCampaignGoal {
    /// `customers/{cid}/campaignConversionGoals/{campaign_id}~{category}~{origin}`.
    pub resource_name: String,
    /// Conversion category, e.g. `SUBMIT_LEAD_FORM`.
    pub category: String,
    /// Conversion origin, e.g. `WEBSITE`.
    pub origin: String,
    /// Whether the campaign currently bids on this goal.
    pub biddable: bool,
}

/// Draft a custom conversion goal: a named set of conversion actions a
/// campaign can bid on.
///
/// Writes one `customConversionGoalOperation.create` with
/// `conversion_actions = customers/{cid}/conversionActions/{id}` and status
/// `ENABLED`. The goal changes nothing on its own; it takes effect once a
/// campaign uses it (`set_campaign_custom_conversion_goal`). Google requires
/// the goal to live in the account that owns the conversion actions (the
/// conversion customer).
///
/// After apply, the new goal ID is the trailing segment of the returned
/// `customConversionGoalResult.resourceName`.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
pub fn create_custom_conversion_goal(
    config: &Config,
    customer_id: &str,
    name: &str,
    conversion_action_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("create_custom_conversion_goal", &config.safety)?;

    let name = name.trim();
    if name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "name must not be empty".to_string(),
        ));
    }
    let actions = validate_id_list(
        "conversion_action_ids",
        "conversion_action_id",
        conversion_action_ids,
        "name the conversion actions the goal bids on (get_conversion_actions lists them)",
    )?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let action_resources: Vec<String> = actions
        .iter()
        .map(|id| format!("customers/{}/conversionActions/{}", cid, id))
        .collect();

    let operations = vec![json!({
        "customConversionGoalOperation": {
            "create": {
                "name": name,
                "conversionActions": action_resources,
                "status": "ENABLED"
            }
        }
    })];

    let changes = json!({
        "name": name,
        "conversion_action_ids": actions,
        "status": "ENABLED",
        "note": "A custom goal changes nothing until a campaign uses it — next call \
                 set_campaign_custom_conversion_goal with the ID from the returned \
                 customConversionGoalResult.resourceName (customers/{cid}/customConversionGoals/{id}). \
                 Secondary conversion actions in a custom goal are bid on too.",
    });

    let plan = ChangePlan::new(
        "create_custom_conversion_goal".to_string(),
        "custom_conversion_goal".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Read a campaign's standard conversion goals.
pub async fn fetch_campaign_conversion_goals(
    client: &GoogleAdsClient,
    customer_id: &str,
    campaign_id: &str,
) -> Result<Vec<ExistingCampaignGoal>> {
    let campaign = validate_numeric_id("campaign_id", campaign_id)?;
    let query = format!(
        "SELECT campaign_conversion_goal.resource_name, campaign_conversion_goal.category, \
         campaign_conversion_goal.origin, campaign_conversion_goal.biddable \
         FROM campaign_conversion_goal WHERE campaign.id = {}",
        campaign
    );
    let rows = client.search(customer_id, &query).await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let goal = &row["campaignConversionGoal"];
            Some(ExistingCampaignGoal {
                resource_name: goal.get("resourceName")?.as_str()?.to_string(),
                category: goal.get("category")?.as_str()?.to_string(),
                origin: goal.get("origin")?.as_str()?.to_string(),
                // proto3 JSON may leave a false boolean out.
                biddable: goal
                    .get("biddable")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect())
}

/// Validate the IDs for [`set_campaign_custom_conversion_goal`] before any
/// HTTP traffic. Returns `(campaign_id, custom_conversion_goal_id)`.
pub fn prepare_campaign_custom_conversion_goal(
    config: &Config,
    campaign_id: &str,
    custom_conversion_goal_id: &str,
) -> Result<(String, String)> {
    check_blocked_operation("set_campaign_custom_conversion_goal", &config.safety)?;
    Ok((
        validate_numeric_id("campaign_id", campaign_id)?,
        validate_numeric_id("custom_conversion_goal_id", custom_conversion_goal_id)?,
    ))
}

/// Make a campaign bid on a custom conversion goal.
///
/// Writes a `conversionGoalCampaignConfigOperation.update` setting
/// `custom_conversion_goal` under the mask `customConversionGoal`, as the
/// campaign-goals guide prescribes; the backend then sets the campaign's
/// `goal_config_level` to `CAMPAIGN`. No other campaign is touched.
///
/// `exclusive_of` carries the campaign's current standard goals (read with
/// [`fetch_campaign_conversion_goals`]). Every one that is biddable gets a
/// `campaignConversionGoalOperation.update` to `biddable = false` in the same
/// atomic mutate, so the campaign bids on the custom goal's actions only.
/// `None` leaves the standard goals as they are, and the campaign bids on the
/// union.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
pub fn set_campaign_custom_conversion_goal(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    custom_conversion_goal_id: &str,
    exclusive_of: Option<&[ExistingCampaignGoal]>,
) -> Result<serde_json::Value> {
    let (campaign, goal) =
        prepare_campaign_custom_conversion_goal(config, campaign_id, custom_conversion_goal_id)?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let mut operations = vec![json!({
        "conversionGoalCampaignConfigOperation": {
            "update": {
                "resourceName": format!(
                    "customers/{}/conversionGoalCampaignConfigs/{}",
                    cid, campaign
                ),
                "customConversionGoal": format!(
                    "customers/{}/customConversionGoals/{}",
                    cid, goal
                )
            },
            "updateMask": "customConversionGoal"
        }
    })];

    let mut switched_off = Vec::new();
    if let Some(existing) = exclusive_of {
        for standard in existing.iter().filter(|g| g.biddable) {
            operations.push(json!({
                "campaignConversionGoalOperation": {
                    "update": {
                        "resourceName": standard.resource_name,
                        "biddable": false
                    },
                    "updateMask": "biddable"
                }
            }));
            switched_off.push(format!("{}~{}", standard.category, standard.origin));
        }
    }

    let exclusive = exclusive_of.is_some();
    let note = if exclusive {
        "The campaign bids only on the custom goal's conversion actions: every standard goal \
         it bid on is set to biddable=false in the same atomic mutate. Other campaigns and the \
         account defaults are untouched. The campaign now uses campaign-level goals, so later \
         account-default changes no longer reach it."
    } else {
        "The campaign bids on the custom goal's conversion actions AND on the standard goals \
         it already bids on (a campaign can combine one custom goal with standard goals). \
         Other campaigns and the account defaults are untouched."
    };
    let changes = json!({
        "campaign_id": campaign,
        "custom_conversion_goal_id": goal,
        "exclusive": exclusive,
        "standard_goals_set_not_biddable": switched_off,
        "note": note,
    });

    let plan = ChangePlan::new(
        "set_campaign_custom_conversion_goal".to_string(),
        "conversion_goal_campaign_config".to_string(),
        campaign,
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Set which standard (category × origin) conversion goals a campaign bids on.
///
/// Writes one `campaignConversionGoalOperation.update` per goal on
/// `customers/{cid}/campaignConversionGoals/{campaign}~{CATEGORY}~{ORIGIN}`,
/// mask `biddable`. Updating any of them moves the campaign to campaign-level
/// goals. Only goals Google generated for the campaign can be updated — a
/// category × origin with no conversion actions in the account has none, and
/// the update fails with RESOURCE_NOT_FOUND. Only primary actions in a
/// biddable goal drive bidding; to bid on one specific action, or on a
/// secondary one, use a custom goal instead.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
pub fn set_campaign_conversion_goals(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    goals: &[CampaignConversionGoalInput],
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_conversion_goals", &config.safety)?;
    let campaign = validate_numeric_id("campaign_id", campaign_id)?;

    if goals.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "goals must not be empty — pass {category, origin, biddable} for each goal to set"
                .to_string(),
        ));
    }
    if goals.len() > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} goals is over the {} limit",
            goals.len(),
            MAX_CRITERIA_PER_PLAN
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for goal in goals {
        if !seen.insert((goal.category, goal.origin)) {
            return Err(McpGoogleAdsError::Validation(format!(
                "{} / {} is listed twice — give each category × origin once",
                goal.category, goal.origin
            )));
        }
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let operations: Vec<serde_json::Value> = goals
        .iter()
        .map(|goal| {
            json!({
                "campaignConversionGoalOperation": {
                    "update": {
                        "resourceName": format!(
                            "customers/{}/campaignConversionGoals/{}~{}~{}",
                            cid,
                            campaign,
                            goal.category.as_api_str(),
                            goal.origin.as_api_str()
                        ),
                        "biddable": goal.biddable
                    },
                    "updateMask": "biddable"
                }
            })
        })
        .collect();

    let changes = json!({
        "campaign_id": campaign,
        "goals": goals
            .iter()
            .map(|g| json!({
                "category": g.category.as_api_str(),
                "origin": g.origin.as_api_str(),
                "biddable": g.biddable,
            }))
            .collect::<Vec<_>>(),
        "note": "Updating campaign goals switches the campaign to campaign-level goals; \
                 account-default changes stop reaching it. Other campaigns are untouched.",
    });

    let plan = ChangePlan::new(
        "set_campaign_conversion_goals".to_string(),
        "campaign_conversion_goal".to_string(),
        campaign,
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
    use crate::models::{ConversionActionCategory, ConversionOrigin};
    use crate::safety::preview::{get_plan, remove_plan};

    fn ops(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        let plan_id = preview["plan_id"].as_str().expect("plan_id");
        let plan = get_plan(plan_id).expect("stored");
        remove_plan(plan_id);
        plan.mutate_operations
    }

    fn goal(category: &str, origin: &str, biddable: bool) -> ExistingCampaignGoal {
        ExistingCampaignGoal {
            resource_name: format!(
                "customers/1234567890/campaignConversionGoals/555~{}~{}",
                category, origin
            ),
            category: category.to_string(),
            origin: origin.to_string(),
            biddable,
        }
    }

    #[test]
    fn custom_goal_create_shape() {
        let config = Config::default();
        let preview = create_custom_conversion_goal(
            &config,
            "123-456-7890",
            " Quote requested ",
            &["111".to_string(), "111".to_string()],
        )
        .expect("ok");
        let ops = ops(&preview);
        assert_eq!(ops.len(), 1);
        let create = &ops[0]["customConversionGoalOperation"]["create"];
        assert_eq!(create["name"], "Quote requested");
        assert_eq!(
            create["conversionActions"],
            json!(["customers/1234567890/conversionActions/111"])
        );
        assert_eq!(create["status"], "ENABLED");

        assert!(create_custom_conversion_goal(&config, "1", "x", &[]).is_err());
        assert!(create_custom_conversion_goal(&config, "1", " ", &["1".to_string()]).is_err());
        assert!(create_custom_conversion_goal(&config, "1", "x", &["abc".to_string()]).is_err());
    }

    #[test]
    fn exclusive_custom_goal_switches_off_only_biddable_standard_goals() {
        let config = Config::default();
        let existing = [
            goal("SUBMIT_LEAD_FORM", "WEBSITE", true),
            goal("PAGE_VIEW", "WEBSITE", false),
            goal("PURCHASE", "WEBSITE", true),
        ];
        let preview = set_campaign_custom_conversion_goal(
            &config,
            "123-456-7890",
            "555",
            "777",
            Some(&existing),
        )
        .expect("ok");
        let ops = ops(&preview);
        assert_eq!(ops.len(), 3, "config + 2 biddable goals");

        let config_op = &ops[0]["conversionGoalCampaignConfigOperation"];
        assert_eq!(
            config_op["update"]["resourceName"],
            "customers/1234567890/conversionGoalCampaignConfigs/555"
        );
        assert_eq!(
            config_op["update"]["customConversionGoal"],
            "customers/1234567890/customConversionGoals/777"
        );
        assert_eq!(config_op["updateMask"], "customConversionGoal");
        // goal_config_level is set by the backend, not sent.
        assert!(config_op["update"].get("goalConfigLevel").is_none());

        let off = &ops[1]["campaignConversionGoalOperation"];
        assert_eq!(
            off["update"]["resourceName"],
            "customers/1234567890/campaignConversionGoals/555~SUBMIT_LEAD_FORM~WEBSITE"
        );
        assert_eq!(off["update"]["biddable"], false);
        assert_eq!(off["updateMask"], "biddable");
        assert_eq!(
            preview["changes"]["standard_goals_set_not_biddable"],
            json!(["SUBMIT_LEAD_FORM~WEBSITE", "PURCHASE~WEBSITE"])
        );
    }

    #[test]
    fn non_exclusive_custom_goal_only_updates_the_config() {
        let config = Config::default();
        let preview =
            set_campaign_custom_conversion_goal(&config, "1234567890", "555", "777", None)
                .expect("ok");
        assert_eq!(preview["changes"]["exclusive"], false);
        assert_eq!(ops(&preview).len(), 1);

        let bad = set_campaign_custom_conversion_goal(&config, "1", "555", "goal", None);
        assert!(bad.unwrap_err().to_string().contains("numeric"));
    }

    #[test]
    fn campaign_goals_update_biddable_by_category_and_origin() {
        let config = Config::default();
        let preview = set_campaign_conversion_goals(
            &config,
            "1234567890",
            "555",
            &[
                CampaignConversionGoalInput {
                    category: ConversionActionCategory::SubmitLeadForm,
                    origin: ConversionOrigin::Website,
                    biddable: true,
                },
                CampaignConversionGoalInput {
                    category: ConversionActionCategory::PageView,
                    origin: ConversionOrigin::Website,
                    biddable: false,
                },
            ],
        )
        .expect("ok");
        let ops = ops(&preview);
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[1]["campaignConversionGoalOperation"]["update"]["resourceName"],
            "customers/1234567890/campaignConversionGoals/555~PAGE_VIEW~WEBSITE"
        );
        assert_eq!(
            ops[1]["campaignConversionGoalOperation"]["update"]["biddable"],
            false
        );

        let duplicate = set_campaign_conversion_goals(
            &config,
            "1",
            "555",
            &[
                CampaignConversionGoalInput {
                    category: ConversionActionCategory::Purchase,
                    origin: ConversionOrigin::Website,
                    biddable: true,
                },
                CampaignConversionGoalInput {
                    category: ConversionActionCategory::Purchase,
                    origin: ConversionOrigin::Website,
                    biddable: false,
                },
            ],
        );
        assert!(duplicate.unwrap_err().to_string().contains("twice"));
        assert!(set_campaign_conversion_goals(&config, "1", "555", &[]).is_err());
    }
}
