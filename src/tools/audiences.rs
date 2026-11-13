use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::{dedupe_preserving_order, validate_numeric_id, MAX_CRITERIA_PER_PLAN};

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

/// Validate and deduplicate a list of user list IDs.
fn user_list_ids(audience_ids: &[String]) -> Result<Vec<String>> {
    if audience_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "audience_ids must not be empty — pass the numeric user list IDs \
             (SELECT user_list.id, user_list.name FROM user_list)"
                .to_string(),
        ));
    }
    let ids = audience_ids
        .iter()
        .map(|id| validate_numeric_id("audience_id", id))
        .collect::<Result<Vec<_>>>()?;
    Ok(dedupe_preserving_order(&ids))
}

/// Validate and deduplicate a list of ad group IDs.
fn ad_group_id_list(ad_group_ids: &[String]) -> Result<Vec<String>> {
    let ids = ad_group_ids
        .iter()
        .map(|id| validate_numeric_id("ad_group_id", id))
        .collect::<Result<Vec<_>>>()?;
    Ok(dedupe_preserving_order(&ids))
}

/// Refuse a plan that would write more criteria than one batch should carry.
fn check_criteria_count(ad_groups: usize, lists: usize) -> Result<usize> {
    let total = ad_groups * lists;
    if total > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "plan would write {} criteria ({} ad groups × {} lists), over the {} limit — \
             split it into smaller batches",
            total, ad_groups, lists, MAX_CRITERIA_PER_PLAN
        )));
    }
    Ok(total)
}

/// Add user lists as positive audience criteria on one or more ad groups.
///
/// Writes one `adGroupCriterionOperation.create` per (ad group × list). This
/// is the ad-group-scoped counterpart of [`add_audience_targeting`], and the
/// one a Display remarketing setup needs: a Display ad group's audience
/// targeting lives on the ad group, and that is where the Google Ads UI puts
/// it too.
///
/// Whether the lists *restrict* reach or only *inform* bids is not decided
/// here but by the ad group's `targeting_setting` for the `AUDIENCE` dimension
/// (`bid_only = false` is "Targeting", `true` is "Observation"). Even in
/// Targeting mode, optimized targeting can still serve far beyond the lists:
/// for a remarketing-only ad group, also call `update_ad_group` with
/// `optimized_targeting_enabled = false`.
///
/// A list that is already a *negative* criterion on the same ad group collides
/// on the shared resource name `adGroupCriteria/{ad_group}~{criterion_id}`;
/// Google rejects the create, so remove the exclusion first.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
/// Reversible (remove the resulting criteria), so no double confirmation.
pub fn add_ad_group_audiences(
    config: &Config,
    customer_id: &str,
    ad_group_ids: &[String],
    audience_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("add_ad_group_audiences", &config.safety)?;

    if ad_group_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "ad_group_ids must not be empty — name every ad group the lists should target"
                .to_string(),
        ));
    }
    let ad_groups = ad_group_id_list(ad_group_ids)?;
    let lists = user_list_ids(audience_ids)?;
    let total = check_criteria_count(ad_groups.len(), lists.len())?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let mut operations = Vec::with_capacity(total);
    for ad_group_id in &ad_groups {
        for list_id in &lists {
            operations.push(json!({
                "adGroupCriterionOperation": {
                    "create": {
                        "adGroup": format!("customers/{}/adGroups/{}", cid, ad_group_id),
                        "userList": {
                            "userList": format!("customers/{}/userLists/{}", cid, list_id)
                        }
                    }
                }
            }));
        }
    }

    let changes = json!({
        "ad_group_ids": ad_groups,
        "audience_ids": lists,
        "total_criteria": total,
        "negative": false,
        "note": "Whether these lists restrict reach (Targeting) or only inform bids \
                 (Observation) follows the ad group's AUDIENCE target restriction. \
                 Optimized targeting can still serve beyond the lists — turn it off with \
                 update_ad_group(optimized_targeting_enabled=false) for pure remarketing.",
    });

    let plan = ChangePlan::new(
        "add_ad_group_audiences".to_string(),
        "ad_group_criterion".to_string(),
        ad_groups
            .first()
            .cloned()
            .unwrap_or_else(|| "new".to_string()),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Exclude user lists from a campaign, or from specific ad groups.
///
/// Writes a NEGATIVE criterion per list: a `campaignCriterionOperation` when
/// `campaign_id` is given (the exclusion covers every ad group in the
/// campaign, including ones added later), or an `adGroupCriterionOperation`
/// per (ad group × list) when `ad_group_ids` is. Exactly one of the two scopes
/// must be supplied. The usual case is keeping converters and customers out
/// of a remarketing campaign.
///
/// Excluded users never see the ads — this is absolute, not a bid adjustment.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
/// Reversible (remove the resulting criteria), so no double confirmation.
pub fn exclude_audiences(
    config: &Config,
    customer_id: &str,
    campaign_id: Option<&str>,
    ad_group_ids: &[String],
    audience_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("exclude_audiences", &config.safety)?;

    let campaign_id = campaign_id.filter(|c| !c.trim().is_empty());
    match (campaign_id.is_some(), ad_group_ids.is_empty()) {
        (true, false) => {
            return Err(McpGoogleAdsError::Validation(
                "pass campaign_id OR ad_group_ids, not both — a campaign-level exclusion \
                 already covers every ad group in the campaign"
                    .to_string(),
            ))
        }
        (false, true) => {
            return Err(McpGoogleAdsError::Validation(
                "pass campaign_id (exclude from the whole campaign) or ad_group_ids \
                 (exclude from those ad groups only)"
                    .to_string(),
            ))
        }
        _ => {}
    }

    let lists = user_list_ids(audience_ids)?;
    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let list_resource = |id: &str| format!("customers/{}/userLists/{}", cid, id);

    let (entity_type, entity_id, operations, scope) = if let Some(campaign) = campaign_id {
        let campaign = validate_numeric_id("campaign_id", campaign)?;
        check_criteria_count(1, lists.len())?;
        let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign);
        let operations: Vec<serde_json::Value> = lists
            .iter()
            .map(|list_id| {
                json!({
                    "campaignCriterionOperation": {
                        "create": {
                            "campaign": campaign_resource,
                            "negative": true,
                            "userList": { "userList": list_resource(list_id) }
                        }
                    }
                })
            })
            .collect();
        (
            "campaign_criterion",
            campaign.clone(),
            operations,
            json!({ "campaign_id": campaign }),
        )
    } else {
        let ad_groups = ad_group_id_list(ad_group_ids)?;
        let total = check_criteria_count(ad_groups.len(), lists.len())?;
        let mut operations = Vec::with_capacity(total);
        for ad_group_id in &ad_groups {
            for list_id in &lists {
                operations.push(json!({
                    "adGroupCriterionOperation": {
                        "create": {
                            "adGroup": format!("customers/{}/adGroups/{}", cid, ad_group_id),
                            "negative": true,
                            "userList": { "userList": list_resource(list_id) }
                        }
                    }
                }));
            }
        }
        (
            "ad_group_criterion",
            ad_groups.first().cloned().unwrap_or_default(),
            operations,
            json!({ "ad_group_ids": ad_groups }),
        )
    };

    let changes = json!({
        "scope": scope,
        "audience_ids": lists,
        "total_criteria": operations.len(),
        "negative": true,
        "note": "Excluded users never see the ads — absolute, not a bid adjustment.",
    });

    let plan = ChangePlan::new(
        "exclude_audiences".to_string(),
        entity_type.to_string(),
        entity_id,
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
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

    /// Pull the stored mutate operations out of a preview.
    fn operations_of(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        use crate::safety::preview::{get_plan, remove_plan};
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let ops = plan.mutate_operations.clone();
        remove_plan(plan_id);
        ops
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn add_ad_group_audiences_writes_one_positive_criterion_per_pair() {
        let config = Config::default();
        let preview = add_ad_group_audiences(
            &config,
            "123-456-7890",
            &ids(&["111", "222"]),
            &ids(&["901", "902"]),
        )
        .expect("ok");

        assert_eq!(preview["operation"], "add_ad_group_audiences");
        assert_eq!(preview["entity_type"], "ad_group_criterion");
        assert_eq!(preview["requires_double_confirm"], false);
        assert_eq!(preview["changes"]["total_criteria"], 4);

        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 4, "2 ad groups × 2 lists");
        let first = &ops[0]["adGroupCriterionOperation"]["create"];
        assert_eq!(first["adGroup"], "customers/1234567890/adGroups/111");
        assert_eq!(
            first["userList"]["userList"],
            "customers/1234567890/userLists/901"
        );
        // Positive criteria carry no negative flag at all.
        assert!(first["negative"].is_null());
        assert_eq!(
            ops[3]["adGroupCriterionOperation"]["create"]["adGroup"],
            "customers/1234567890/adGroups/222"
        );
    }

    #[test]
    fn add_ad_group_audiences_dedupes_and_validates() {
        let config = Config::default();
        let preview = add_ad_group_audiences(
            &config,
            "1234567890",
            &ids(&["111", "111"]),
            &ids(&["9", "9"]),
        )
        .expect("ok");
        assert_eq!(operations_of(&preview).len(), 1);

        let err = add_ad_group_audiences(&config, "1234567890", &[], &ids(&["9"]))
            .expect_err("no ad groups");
        assert!(err.to_string().contains("ad_group_ids must not be empty"));

        let err = add_ad_group_audiences(&config, "1234567890", &ids(&["111"]), &[])
            .expect_err("no lists");
        assert!(err.to_string().contains("audience_ids must not be empty"));

        let err = add_ad_group_audiences(
            &config,
            "1234567890",
            &ids(&["111"]),
            &ids(&["customers/1234567890/userLists/9"]),
        )
        .expect_err("resource name instead of id");
        assert!(err.to_string().contains("numeric"));
    }

    #[test]
    fn add_ad_group_audiences_over_limit_rejected() {
        let config = Config::default();
        let many: Vec<String> = (1..=251).map(|i| i.to_string()).collect();
        let err = add_ad_group_audiences(&config, "1234567890", &many, &ids(&["1", "2"]))
            .expect_err("over the limit");
        assert!(err.to_string().contains("502"));
    }

    #[test]
    fn exclude_audiences_campaign_scope_writes_negative_campaign_criteria() {
        let config = Config::default();
        let preview = exclude_audiences(
            &config,
            "123-456-7890",
            Some("9000000002"),
            &[],
            &ids(&["9000002001", "9000002002"]),
        )
        .expect("ok");

        assert_eq!(preview["operation"], "exclude_audiences");
        assert_eq!(preview["entity_type"], "campaign_criterion");
        assert_eq!(preview["entity_id"], "9000000002");
        assert_eq!(preview["changes"]["negative"], true);

        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 2);
        let first = &ops[0]["campaignCriterionOperation"]["create"];
        assert_eq!(
            first["campaign"],
            "customers/1234567890/campaigns/9000000002"
        );
        assert_eq!(first["negative"], true);
        assert_eq!(
            first["userList"]["userList"],
            "customers/1234567890/userLists/9000002001"
        );
    }

    #[test]
    fn exclude_audiences_ad_group_scope_writes_negative_ad_group_criteria() {
        let config = Config::default();
        let preview = exclude_audiences(
            &config,
            "1234567890",
            None,
            &ids(&["111", "222"]),
            &ids(&["901"]),
        )
        .expect("ok");

        assert_eq!(preview["entity_type"], "ad_group_criterion");
        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 2);
        for (op, ad_group) in ops.iter().zip(["111", "222"]) {
            let c = &op["adGroupCriterionOperation"]["create"];
            assert_eq!(c["negative"], true);
            assert_eq!(
                c["adGroup"],
                format!("customers/1234567890/adGroups/{}", ad_group)
            );
        }
    }

    #[test]
    fn exclude_audiences_needs_exactly_one_scope() {
        let config = Config::default();
        let both = exclude_audiences(
            &config,
            "1234567890",
            Some("1"),
            &ids(&["111"]),
            &ids(&["9"]),
        )
        .expect_err("both scopes");
        assert!(both.to_string().contains("not both"));

        let neither = exclude_audiences(&config, "1234567890", None, &[], &ids(&["9"]))
            .expect_err("no scope");
        assert!(neither.to_string().contains("campaign_id"));

        // A blank campaign_id counts as absent, not as a scope.
        let blank = exclude_audiences(&config, "1234567890", Some("  "), &[], &ids(&["9"]))
            .expect_err("blank campaign id");
        assert!(blank.to_string().contains("campaign_id"));
    }

    #[test]
    fn new_audience_tools_respect_blocked_operations() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec![
            "add_ad_group_audiences".to_string(),
            "exclude_audiences".to_string(),
        ];
        let add = add_ad_group_audiences(&config, "1234567890", &ids(&["1"]), &ids(&["2"]));
        assert!(add.unwrap_err().to_string().contains("blocked"));
        let exclude = exclude_audiences(&config, "1234567890", Some("1"), &[], &ids(&["2"]));
        assert!(exclude.unwrap_err().to_string().contains("blocked"));
    }
}
