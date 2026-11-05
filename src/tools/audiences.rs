use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::AudienceKind;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

const VALID_TARGETING_MODES: &[&str] = &["TARGETING", "OBSERVATION"];

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

/// One audience of an ad group: the audience it references, targeted or excluded.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdGroupAudienceInput {
    /// USER_LIST, USER_INTEREST (affinity or in-market segment), CUSTOM_AUDIENCE or COMBINED_AUDIENCE.
    pub kind: AudienceKind,
    /// The numeric ID of the user list, user interest, custom audience or combined audience.
    pub id: String,
    /// true excludes the audience from the ad group. Defaults to false.
    #[serde(default)]
    pub negative: bool,
}

/// Draft audiences on an ad group, and how its targeted audiences apply.
///
/// `targeting_mode`, when given, sets the ad group's AUDIENCE target restriction
/// (`targeting_setting.target_restrictions`): `TARGETING` restricts delivery to
/// the targeted audiences (`bid_only` false), `OBSERVATION` only observes them
/// (`bid_only` true). Google replaces the whole restriction list on update, and a
/// Search ad group also holds restrictions for other dimensions (age range,
/// gender, parental status, income range): `existing_restrictions`, the ad
/// group's current list (read with [`resolve_ad_group_target_restrictions`]), is
/// sent back with only its AUDIENCE entry replaced. Each audience is created as
/// an ad group criterion, targeted or excluded; remove one with `remove_keywords`,
/// which removes any ad group criterion by its ID.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn add_ad_group_audiences(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    audiences: &[AdGroupAudienceInput],
    targeting_mode: Option<&str>,
    existing_restrictions: &[serde_json::Value],
) -> Result<serde_json::Value> {
    check_blocked_operation("add_ad_group_audiences", &config.safety)?;

    if audiences.is_empty() && targeting_mode.is_none() {
        return Err(McpGoogleAdsError::Validation(
            "Pass audiences, a targeting_mode, or both".to_string(),
        ));
    }
    if let Some(mode) = targeting_mode {
        if !VALID_TARGETING_MODES.contains(&mode) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Invalid targeting mode '{}'. Must be one of: {}",
                mode,
                VALID_TARGETING_MODES.join(", ")
            )));
        }
    }
    let mut seen = HashSet::new();
    for audience in audiences {
        if audience.id.is_empty() || !audience.id.chars().all(|c| c.is_ascii_digit()) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Audience id must be numeric, got '{}'",
                audience.id
            )));
        }
        if !seen.insert((audience.kind, audience.id.clone())) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Audience {} {} appears twice: an audience is either targeted or excluded",
                audience.kind.as_str(),
                audience.id
            )));
        }
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let ad_group_resource = format!("customers/{}/adGroups/{}", cid, ad_group_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();

    if let Some(mode) = targeting_mode {
        let mut restrictions: Vec<serde_json::Value> = existing_restrictions
            .iter()
            .filter(|r| r.get("targetingDimension").and_then(|d| d.as_str()) != Some("AUDIENCE"))
            .cloned()
            .collect();
        restrictions
            .push(json!({"targetingDimension": "AUDIENCE", "bidOnly": mode == "OBSERVATION"}));
        operations.push(json!({
            "adGroupOperation": {
                "update": {
                    "resourceName": ad_group_resource,
                    "targetingSetting": {
                        "targetRestrictions": restrictions
                    }
                },
                "updateMask": "targetingSetting.targetRestrictions"
            }
        }));
    }
    for audience in audiences {
        let (criterion_field, info_field, collection) = audience.kind.api_fields();
        operations.push(json!({
            "adGroupCriterionOperation": {
                "create": {
                    "adGroup": ad_group_resource,
                    "negative": audience.negative,
                    criterion_field: {
                        info_field: format!("customers/{}/{}/{}", cid, collection, audience.id)
                    }
                }
            }
        }));
    }

    let changes = json!({
        "ad_group_id": ad_group_id,
        "targeting_mode": targeting_mode,
        "audiences": audiences.iter().map(|a| json!({
            "kind": a.kind.as_str(),
            "id": a.id,
            "negative": a.negative,
        })).collect::<Vec<_>>(),
    });

    let plan = ChangePlan::new(
        "add_ad_group_audiences".to_string(),
        "ad_group_criterion".to_string(),
        ad_group_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// The current target restrictions of an ad group (`ad_group.targeting_setting.target_restrictions`).
///
/// An ad group without any restriction yields an empty list.
pub async fn resolve_ad_group_target_restrictions(
    client: &crate::client::GoogleAdsClient,
    customer_id: &str,
    ad_group_id: &str,
) -> Result<Vec<serde_json::Value>> {
    if ad_group_id.is_empty() || !ad_group_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "ad_group_id must be numeric, got '{ad_group_id}'"
        )));
    }
    let query = format!(
        "SELECT ad_group.targeting_setting.target_restrictions FROM ad_group WHERE ad_group.id = {}",
        ad_group_id
    );
    let rows = client.search(customer_id, &query).await?;
    let row = rows.first().ok_or_else(|| {
        McpGoogleAdsError::Validation(format!("Ad group {ad_group_id} was not found"))
    })?;
    Ok(row
        .pointer("/adGroup/targetingSetting/targetRestrictions")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
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
