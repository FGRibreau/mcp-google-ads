use std::collections::HashSet;

use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{CampaignConversionGoalInput, GoalConfigLevel};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Draft what a campaign optimises on: the account goals, its own goals, or a custom goal.
///
/// Writes the campaign's `ConversionGoalCampaignConfig` (the goal level and the
/// custom goal, cleared when none is given) and, for each goal given, its
/// `CampaignConversionGoal.biddable`. Google creates one campaign conversion goal
/// per (category, origin) of the account: they are only updated, and a goal left
/// out keeps its state — list every goal whose `biddable` must change.
///
/// - `CUSTOMER`: the campaign follows the account default goals; no goal nor custom goal.
/// - `CAMPAIGN` with `custom_conversion_goal_id`: the campaign bids on that existing
///   custom goal; no goal list.
/// - `CAMPAIGN` without it: the campaign bids on its biddable goals; at least one.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn set_campaign_conversion_goals(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    goal_config_level: GoalConfigLevel,
    goals: &[CampaignConversionGoalInput],
    custom_conversion_goal_id: Option<&str>,
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_conversion_goals", &config.safety)?;
    check_goals(goal_config_level, goals, custom_conversion_goal_id)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let custom_goal_resource = custom_conversion_goal_id
        .map(|id| format!("customers/{}/customConversionGoals/{}", cid, id))
        .unwrap_or_default();

    let mut operations = vec![json!({
        "conversionGoalCampaignConfigOperation": {
            "update": {
                "resourceName": format!("customers/{}/conversionGoalCampaignConfigs/{}", cid, campaign_id),
                "goalConfigLevel": goal_config_level.as_api_str(),
                // Empty clears a custom goal the campaign optimised on before.
                "customConversionGoal": custom_goal_resource
            },
            "updateMask": "goalConfigLevel,customConversionGoal"
        }
    })];
    for goal in goals {
        operations.push(json!({
            "campaignConversionGoalOperation": {
                "update": {
                    "resourceName": format!(
                        "customers/{}/campaignConversionGoals/{}~{}~{}",
                        cid,
                        campaign_id,
                        goal.category.as_api_str(),
                        goal.origin.as_api_str()
                    ),
                    "biddable": goal.biddable
                },
                "updateMask": "biddable"
            }
        }));
    }

    let changes = json!({
        "campaign_id": campaign_id,
        "goal_config_level": goal_config_level.as_api_str(),
        "custom_conversion_goal_id": custom_conversion_goal_id,
        "campaign_conversion_goals": goals.iter().map(|g| json!({
            "category": g.category.as_api_str(),
            "origin": g.origin.as_api_str(),
            "biddable": g.biddable,
        })).collect::<Vec<_>>(),
    });

    let plan = ChangePlan::new(
        "set_campaign_conversion_goals".to_string(),
        "campaign".to_string(),
        campaign_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

fn check_goals(
    level: GoalConfigLevel,
    goals: &[CampaignConversionGoalInput],
    custom_conversion_goal_id: Option<&str>,
) -> Result<()> {
    let invalid = |message: &str| Err(McpGoogleAdsError::Validation(message.to_string()));
    let mut seen = HashSet::new();
    for goal in goals {
        if !seen.insert((goal.category, goal.origin)) {
            return Err(McpGoogleAdsError::Validation(format!(
                "conversion goal {}/{} appears twice",
                goal.category.as_api_str(),
                goal.origin.as_api_str()
            )));
        }
    }
    if matches!(custom_conversion_goal_id, Some(id) if id.trim().is_empty()) {
        return invalid("custom_conversion_goal_id must not be empty — omit it for none");
    }
    match (level, custom_conversion_goal_id) {
        (GoalConfigLevel::Customer, Some(_)) => {
            invalid("custom_conversion_goal_id needs goal_config_level CAMPAIGN")
        }
        (GoalConfigLevel::Customer, None) if !goals.is_empty() => invalid(
            "at goal_config_level CUSTOMER the campaign follows the account goals: \
             pass no campaign_conversion_goals",
        ),
        (GoalConfigLevel::Campaign, Some(_)) if !goals.is_empty() => invalid(
            "a campaign optimises on a custom conversion goal or on campaign_conversion_goals, not both",
        ),
        (GoalConfigLevel::Campaign, None) if !goals.iter().any(|g| g.biddable) => invalid(
            "at goal_config_level CAMPAIGN without a custom goal, at least one campaign \
             conversion goal is biddable",
        ),
        _ => Ok(()),
    }
}
