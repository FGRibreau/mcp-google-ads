//! Demographic targeting: income band, age bracket and gender exclusions.
//!
//! Google exposes these as `AdGroupCriterion` rows, one per (ad group, tier).
//! They are *ad group* scoped, not campaign scoped — a campaign-level demographic
//! exclusion does not exist, so excluding a band across a campaign means writing
//! one criterion per ad group in it.
//!
//! Before this module the only way to apply them was a hand-rolled REST call,
//! which meant every MCP-built campaign silently shipped without the income
//! exclusion the clinic playbook treats as mandatory.

use serde_json::json;
use std::collections::HashSet;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::demographic::{AgeRange, Gender, IncomeRange};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Upper bound on criteria written in a single plan.
///
/// Google's own mutate ceiling is far higher; this is a guard against a caller
/// fanning out over every ad group in a large account by accident. Exceeding it
/// is a hard error rather than a truncation — a silently partial exclusion is
/// worse than none, because it reads as done.
const MAX_CRITERIA_PER_PLAN: usize = 500;

/// Validate an ID is a bare positive integer, as Google Ads IDs always are.
fn validate_numeric_id(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must not be empty",
            label
        )));
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must be numeric, got '{}'",
            label, value
        )));
    }
    Ok(trimmed.to_string())
}

/// Deduplicate while preserving caller order, so the preview reads predictably.
fn dedupe_preserving_order<T: Clone + std::hash::Hash + Eq>(items: &[T]) -> Vec<T> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert((*item).clone()))
        .cloned()
        .collect()
}

/// Exclude demographic tiers from one or more ad groups.
///
/// Writes a negative `adGroupCriterionOperation` per (ad group × tier). Users in
/// an excluded tier stop seeing the ads, and — unlike a −100% bid modifier —
/// the exclusion is absolute.
///
/// The criterion is created **by type**: Google resolves the fixed criterion ID
/// itself, so the caller never supplies one. The IDs in
/// [`crate::models::demographic`] are only needed to undo an exclusion.
///
/// # Collisions
///
/// A tier that already exists on the ad group as an explicit *positive* row
/// cannot also receive a negative row — both share the resource name
/// `adGroupCriteria/{ad_group}~{criterion_id}`, so Google rejects the create
/// with a duplicate-resource error. Remove the positive row first via
/// [`remove_demographic_criterion`]. A fresh ad group has no demographic rows at
/// all, so the common case needs no such dance.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
/// Reversible (remove the resulting criteria), so no double confirmation.
#[allow(clippy::too_many_arguments)]
pub fn exclude_demographics(
    config: &Config,
    customer_id: &str,
    ad_group_ids: &[String],
    income_ranges: &[IncomeRange],
    age_ranges: &[AgeRange],
    genders: &[Gender],
) -> Result<serde_json::Value> {
    check_blocked_operation("exclude_demographics", &config.safety)?;

    if ad_group_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "ad_group_ids must not be empty — demographic criteria are ad-group scoped, \
             so name every ad group the exclusion should cover"
                .to_string(),
        ));
    }

    if income_ranges.is_empty() && age_ranges.is_empty() && genders.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "nothing to exclude — supply at least one of income_ranges, age_ranges or genders"
                .to_string(),
        ));
    }

    let ad_groups = dedupe_preserving_order(ad_group_ids);
    let incomes = dedupe_preserving_order(income_ranges);
    let ages = dedupe_preserving_order(age_ranges);
    let genders = dedupe_preserving_order(genders);

    // Excluding every tier of a dimension would stop the ad group serving
    // entirely. That is never the intent, and Google would accept it silently.
    if genders.len() == 3 {
        return Err(McpGoogleAdsError::Validation(
            "excluding MALE, FEMALE and UNDETERMINED together would stop the ad group serving \
             to anyone — exclude at most two"
                .to_string(),
        ));
    }
    if ages.len() == 7 {
        return Err(McpGoogleAdsError::Validation(
            "excluding all seven age brackets would stop the ad group serving to anyone"
                .to_string(),
        ));
    }
    if incomes.len() == 7 {
        return Err(McpGoogleAdsError::Validation(
            "excluding all seven income bands would stop the ad group serving to anyone"
                .to_string(),
        ));
    }

    let per_ad_group = incomes.len() + ages.len() + genders.len();
    let total = per_ad_group * ad_groups.len();
    if total > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "plan would write {} criteria ({} ad groups × {} tiers), over the {} limit — \
             split it into smaller batches",
            total,
            ad_groups.len(),
            per_ad_group,
            MAX_CRITERIA_PER_PLAN
        )));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);

    let mut operations = Vec::with_capacity(total);
    for ad_group_id in &ad_groups {
        let id = validate_numeric_id("ad_group_id", ad_group_id)?;
        let ad_group_resource = format!("customers/{}/adGroups/{}", cid, id);

        for band in &incomes {
            operations.push(negative_criterion(
                &ad_group_resource,
                "incomeRange",
                band.as_api_str(),
            ));
        }
        for age in &ages {
            operations.push(negative_criterion(
                &ad_group_resource,
                "ageRange",
                age.as_api_str(),
            ));
        }
        for gender in &genders {
            operations.push(negative_criterion(
                &ad_group_resource,
                "gender",
                gender.as_api_str(),
            ));
        }
    }

    let changes = json!({
        "ad_group_ids": ad_groups,
        "ad_group_count": ad_groups.len(),
        "income_ranges": incomes.iter().map(|b| b.as_api_str()).collect::<Vec<_>>(),
        "age_ranges": ages.iter().map(|a| a.as_api_str()).collect::<Vec<_>>(),
        "genders": genders.iter().map(|g| g.as_api_str()).collect::<Vec<_>>(),
        "criteria_per_ad_group": per_ad_group,
        "total_criteria": total,
        "negative": true,
        "note": "Demographic criteria are ad-group scoped. Excluded users never see the ad — \
                 this is absolute, not a bid adjustment.",
    });

    let plan = ChangePlan::new(
        "exclude_demographics".to_string(),
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

/// Build one negative `adGroupCriterionOperation.create`.
///
/// `field` is the criterion field name (`incomeRange` / `ageRange` / `gender`)
/// and `type_value` its API enum string. No criterion ID is sent: Google derives
/// it from the type.
fn negative_criterion(ad_group_resource: &str, field: &str, type_value: &str) -> serde_json::Value {
    json!({
        "adGroupCriterionOperation": {
            "create": {
                "adGroup": ad_group_resource,
                "negative": true,
                field: { "type": type_value }
            }
        }
    })
}

/// Remove a demographic criterion from an ad group.
///
/// Two uses: undoing an exclusion, and clearing a positive row that is blocking
/// a negative one (see [`exclude_demographics`]).
///
/// `criterion_id` is the fixed ID for the tier — read it from
/// `get_demographics`, or from the `criterion_id()` methods on the enums in
/// [`crate::models::demographic`] (e.g. income "Unknown" is always 510000).
///
/// Requires double confirmation: dropping a *negative* row silently re-opens the
/// ad group to a tier the advertiser deliberately excluded.
pub fn remove_demographic_criterion(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    criterion_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_demographic_criterion", &config.safety)?;

    let ad_group = validate_numeric_id("ad_group_id", ad_group_id)?;
    let criterion = validate_numeric_id("criterion_id", criterion_id)?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let resource = format!(
        "customers/{}/adGroupCriteria/{}~{}",
        cid, ad_group, criterion
    );

    let operations = vec![json!({
        "adGroupCriterionOperation": { "remove": resource }
    })];

    let changes = json!({
        "ad_group_id": ad_group,
        "criterion_id": criterion,
        "removed_criterion": resource,
        "warning": "If this was a negative criterion, removing it re-opens the ad group to that \
                    demographic tier.",
    });

    let plan = ChangePlan::new(
        "remove_demographic_criterion".to_string(),
        "ad_group_criterion".to_string(),
        ad_group,
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Read the demographic criteria currently on an account's ad groups.
///
/// Returns both positive and negative rows — the `negative` flag distinguishes
/// them — so this doubles as the verification step after `exclude_demographics`
/// and as the way to discover a blocking positive row. An ad group with no rows
/// simply does not appear: absence means "serves to everyone", which is the
/// default state, not an error.
pub async fn get_demographics(
    client: &GoogleAdsClient,
    customer_id: &str,
    campaign_id: Option<&str>,
) -> Result<String> {
    let mut query = String::from(
        "SELECT \
            campaign.id, \
            campaign.name, \
            ad_group.id, \
            ad_group.name, \
            ad_group_criterion.criterion_id, \
            ad_group_criterion.type, \
            ad_group_criterion.negative, \
            ad_group_criterion.status, \
            ad_group_criterion.bid_modifier, \
            ad_group_criterion.income_range.type, \
            ad_group_criterion.age_range.type, \
            ad_group_criterion.gender.type \
        FROM ad_group_criterion \
        WHERE ad_group_criterion.type IN ('INCOME_RANGE', 'AGE_RANGE', 'GENDER')",
    );

    if let Some(id) = campaign_id {
        let cid = validate_numeric_id("campaign_id", id)?;
        query.push_str(&format!(" AND campaign.id = {}", cid));
    }

    query.push_str(" ORDER BY campaign.name, ad_group.name");

    let rows = client.search(customer_id, &query).await?;

    let result = json!({
        "demographic_criteria": rows,
        "total_count": rows.len(),
        "note": "Ad groups absent from this list have no demographic criteria and serve to \
                 everyone — that is the API default, not a misconfiguration.",
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::preview::{get_plan, remove_plan};

    /// Pull the stored mutate operations out of a preview.
    fn operations_of(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let ops = plan.mutate_operations.clone();
        remove_plan(plan_id);
        ops
    }

    #[test]
    fn writes_one_negative_criterion_per_ad_group_and_band() {
        let config = Config::default();
        let preview = exclude_demographics(
            &config,
            "123-456-7890",
            &["111".to_string(), "222".to_string()],
            &IncomeRange::bottom_50_and_unknown(),
            &[],
            &[],
        )
        .expect("ok");

        assert_eq!(preview["operation"], "exclude_demographics");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        assert_eq!(preview["requires_double_confirm"], false);
        assert_eq!(preview["changes"]["total_criteria"], 4);

        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 4, "2 ad groups × 2 bands");

        let first = &ops[0]["adGroupCriterionOperation"]["create"];
        assert_eq!(first["adGroup"], "customers/1234567890/adGroups/111");
        assert_eq!(first["negative"], true);
        assert_eq!(first["incomeRange"]["type"], "INCOME_RANGE_0_50");
        // The criterion ID is Google's to assign — we must not send one.
        assert!(first["criterionId"].is_null());
        assert!(first["resourceName"].is_null());

        assert_eq!(
            ops[1]["adGroupCriterionOperation"]["create"]["incomeRange"]["type"],
            "INCOME_RANGE_UNDETERMINED"
        );
        assert_eq!(
            ops[2]["adGroupCriterionOperation"]["create"]["adGroup"],
            "customers/1234567890/adGroups/222"
        );
    }

    #[test]
    fn mixes_all_three_dimensions() {
        let config = Config::default();
        let preview = exclude_demographics(
            &config,
            "1234567890",
            &["111".to_string()],
            &[IncomeRange::IncomeRange0_50],
            &[AgeRange::AgeRange18_24],
            &[Gender::GenderMale],
        )
        .expect("ok");

        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 3);

        let fields: Vec<String> = ops
            .iter()
            .map(|o| {
                let c = &o["adGroupCriterionOperation"]["create"];
                for f in ["incomeRange", "ageRange", "gender"] {
                    if !c[f].is_null() {
                        return f.to_string();
                    }
                }
                panic!("no criterion field on operation");
            })
            .collect();
        assert_eq!(fields, vec!["incomeRange", "ageRange", "gender"]);

        // Gender goes on the wire unprefixed.
        assert_eq!(
            ops[2]["adGroupCriterionOperation"]["create"]["gender"]["type"],
            "MALE"
        );
    }

    #[test]
    fn deduplicates_ad_groups_and_tiers() {
        let config = Config::default();
        let preview = exclude_demographics(
            &config,
            "1234567890",
            &["111".to_string(), "111".to_string()],
            &[IncomeRange::IncomeRange0_50, IncomeRange::IncomeRange0_50],
            &[],
            &[],
        )
        .expect("ok");

        // Without dedupe this would emit 4 ops and Google would reject the
        // batch on duplicate resource names.
        assert_eq!(operations_of(&preview).len(), 1);
    }

    #[test]
    fn empty_ad_groups_rejected() {
        let config = Config::default();
        let err = exclude_demographics(
            &config,
            "1234567890",
            &[],
            &[IncomeRange::IncomeRange0_50],
            &[],
            &[],
        )
        .expect_err("rejected");
        assert!(err.to_string().contains("ad_group_ids must not be empty"));
    }

    #[test]
    fn no_tiers_selected_rejected() {
        let config = Config::default();
        let err = exclude_demographics(&config, "1234567890", &["111".to_string()], &[], &[], &[])
            .expect_err("rejected");
        assert!(err.to_string().contains("nothing to exclude"));
    }

    #[test]
    fn non_numeric_ad_group_rejected() {
        let config = Config::default();
        let err = exclude_demographics(
            &config,
            "1234567890",
            &["GA Marca".to_string()],
            &[IncomeRange::IncomeRange0_50],
            &[],
            &[],
        )
        .expect_err("rejected");
        assert!(err.to_string().contains("numeric"));
    }

    #[test]
    fn excluding_every_gender_rejected() {
        let config = Config::default();
        let err = exclude_demographics(
            &config,
            "1234567890",
            &["111".to_string()],
            &[],
            &[],
            &[
                Gender::GenderMale,
                Gender::GenderFemale,
                Gender::GenderUndetermined,
            ],
        )
        .expect_err("rejected");
        assert!(err.to_string().contains("stop the ad group serving"));
    }

    #[test]
    fn over_limit_batch_rejected_not_truncated() {
        let config = Config::default();
        let many: Vec<String> = (1..=300).map(|i| i.to_string()).collect();
        let err = exclude_demographics(
            &config,
            "1234567890",
            &many,
            &IncomeRange::bottom_50_and_unknown(),
            &[],
            &[],
        )
        .expect_err("rejected");
        let msg = err.to_string();
        assert!(msg.contains("600"), "reports the real count: {}", msg);
        assert!(msg.contains("500"));
    }

    #[test]
    fn at_limit_batch_allowed() {
        let config = Config::default();
        let many: Vec<String> = (1..=250).map(|i| i.to_string()).collect();
        let preview = exclude_demographics(
            &config,
            "1234567890",
            &many,
            &IncomeRange::bottom_50_and_unknown(),
            &[],
            &[],
        )
        .expect("exactly at the limit is fine");
        assert_eq!(operations_of(&preview).len(), MAX_CRITERIA_PER_PLAN);
    }

    #[test]
    fn blocked_operation_rejected() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["exclude_demographics".to_string()];
        let err = exclude_demographics(
            &config,
            "1234567890",
            &["111".to_string()],
            &[IncomeRange::IncomeRange0_50],
            &[],
            &[],
        )
        .expect_err("blocked");
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn remove_builds_criterion_resource_and_double_confirms() {
        let config = Config::default();
        let preview =
            remove_demographic_criterion(&config, "123-456-7890", "111", "510000").expect("ok");

        assert_eq!(preview["operation"], "remove_demographic_criterion");
        assert_eq!(preview["requires_double_confirm"], true);

        let ops = operations_of(&preview);
        assert_eq!(
            ops[0]["adGroupCriterionOperation"]["remove"],
            "customers/1234567890/adGroupCriteria/111~510000"
        );
    }

    #[test]
    fn remove_rejects_non_numeric_criterion() {
        let config = Config::default();
        let err =
            remove_demographic_criterion(&config, "1234567890", "111", "INCOME_RANGE_UNDETERMINED")
                .expect_err("rejected");
        assert!(err.to_string().contains("numeric"));
    }
}
