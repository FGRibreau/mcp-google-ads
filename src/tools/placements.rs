//! Campaign-level placement exclusions for Display campaigns.
//!
//! Where a Display ad may appear is narrowed by NEGATIVE campaign criteria:
//! a website (`placement.url`), a mobile app category, or a single mobile app.
//! They are campaign scoped, so one exclusion covers every ad group in the
//! campaign, including ones added later.
//!
//! The common case is app inventory: games and utility apps often bring cheap
//! accidental clicks that never convert. Mobile app category `69500` is
//! "All Apps" (look the categories up with `SELECT mobile_app_category_constant.id,
//! mobile_app_category_constant.name FROM mobile_app_category_constant`), so
//! excluding it keeps a campaign off every app placement in one criterion.

use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::{dedupe_preserving_order, validate_numeric_id, MAX_CRITERIA_PER_PLAN};

/// Mobile app category constant ID for "All Apps".
pub const ALL_APPS_CATEGORY_ID: &str = "69500";

/// Normalize a placement URL to the bare host-and-path form Google stores.
///
/// Strips an `http://` / `https://` scheme and a trailing slash, so
/// `https://www.example.com/` and `www.example.com` become the same criterion
/// instead of two that differ only in spelling.
fn normalize_placement_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    let without_scheme = if lower.starts_with("https://") {
        &trimmed["https://".len()..]
    } else if lower.starts_with("http://") {
        &trimmed["http://".len()..]
    } else {
        trimmed
    };
    let url = without_scheme.trim_end_matches('/');
    if url.is_empty() {
        return Err(McpGoogleAdsError::Validation(format!(
            "placement URL must not be empty, got '{}'",
            raw
        )));
    }
    if url.chars().any(char::is_whitespace) || !url.contains('.') {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' is not a website placement — pass a domain such as 'example.com'",
            raw
        )));
    }
    Ok(url.to_string())
}

/// Accept a bare category ID or a `mobileAppCategoryConstants/{id}` resource
/// name, and return the bare numeric ID.
fn normalize_category_id(raw: &str) -> Result<String> {
    let bare = raw
        .trim()
        .strip_prefix("mobileAppCategoryConstants/")
        .unwrap_or(raw.trim());
    validate_numeric_id("mobile_app_category_id", bare)
}

/// Validate a mobile app ID: `1-<App Store id>` or `2-<Android package name>`.
fn validate_app_id(raw: &str) -> Result<String> {
    let app_id = raw.trim();
    let valid = match app_id.split_once('-') {
        Some(("1", rest)) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
        Some(("2", rest)) => !rest.is_empty() && !rest.chars().any(char::is_whitespace),
        _ => false,
    };
    if !valid {
        return Err(McpGoogleAdsError::Validation(format!(
            "mobile_app_id '{}' must be '1-<App Store id>' (iOS) or '2-<package name>' (Android)",
            raw
        )));
    }
    Ok(app_id.to_string())
}

/// Exclude websites, mobile app categories and/or mobile apps from a campaign.
///
/// Writes one NEGATIVE `campaignCriterionOperation` per placement. Supply at
/// least one of `placement_urls`, `mobile_app_category_ids` or
/// `mobile_app_ids`. Pass category `69500` ("All Apps") to keep a Display
/// campaign off app inventory entirely.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
/// Reversible (remove the resulting criteria), so no double confirmation.
pub fn exclude_placements(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    placement_urls: &[String],
    mobile_app_category_ids: &[String],
    mobile_app_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("exclude_placements", &config.safety)?;

    if placement_urls.is_empty() && mobile_app_category_ids.is_empty() && mobile_app_ids.is_empty()
    {
        return Err(McpGoogleAdsError::Validation(
            "nothing to exclude — supply at least one of placement_urls, \
             mobile_app_category_ids or mobile_app_ids"
                .to_string(),
        ));
    }

    let campaign = validate_numeric_id("campaign_id", campaign_id)?;

    let urls = dedupe_preserving_order(
        &placement_urls
            .iter()
            .map(|u| normalize_placement_url(u))
            .collect::<Result<Vec<_>>>()?,
    );
    let categories = dedupe_preserving_order(
        &mobile_app_category_ids
            .iter()
            .map(|c| normalize_category_id(c))
            .collect::<Result<Vec<_>>>()?,
    );
    let apps = dedupe_preserving_order(
        &mobile_app_ids
            .iter()
            .map(|a| validate_app_id(a))
            .collect::<Result<Vec<_>>>()?,
    );

    let total = urls.len() + categories.len() + apps.len();
    if total > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "plan would write {} exclusions, over the {} limit — split it into smaller batches",
            total, MAX_CRITERIA_PER_PLAN
        )));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign);
    let negative = |criterion: (&str, serde_json::Value)| {
        let mut create = json!({ "campaign": campaign_resource, "negative": true });
        create[criterion.0] = criterion.1;
        json!({ "campaignCriterionOperation": { "create": create } })
    };

    let mut operations = Vec::with_capacity(total);
    for url in &urls {
        operations.push(negative(("placement", json!({ "url": url }))));
    }
    for category in &categories {
        operations.push(negative((
            "mobileAppCategory",
            json!({
                "mobileAppCategoryConstant": format!("mobileAppCategoryConstants/{}", category)
            }),
        )));
    }
    for app in &apps {
        operations.push(negative(("mobileApplication", json!({ "appId": app }))));
    }

    let mut changes = json!({
        "campaign_id": campaign,
        "placement_urls": urls,
        "mobile_app_category_ids": categories,
        "mobile_app_ids": apps,
        "total_exclusions": total,
        "negative": true,
    });
    if categories.iter().any(|c| c == ALL_APPS_CATEGORY_ID) {
        changes["note"] = json!(
            "Category 69500 is 'All Apps': the campaign stops serving on every mobile app \
             placement."
        );
    }

    let plan = ChangePlan::new(
        "exclude_placements".to_string(),
        "campaign_criterion".to_string(),
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
    use crate::safety::preview::{get_plan, remove_plan};

    fn operations_of(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let ops = plan.mutate_operations.clone();
        remove_plan(plan_id);
        ops
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn excludes_all_apps_with_one_negative_campaign_criterion() {
        let config = Config::default();
        let preview = exclude_placements(
            &config,
            "123-456-7890",
            "9000000002",
            &[],
            &strings(&["69500"]),
            &[],
        )
        .expect("ok");

        assert_eq!(preview["operation"], "exclude_placements");
        assert_eq!(preview["entity_type"], "campaign_criterion");
        assert_eq!(preview["requires_double_confirm"], false);
        assert!(preview["changes"]["note"]
            .as_str()
            .unwrap_or_default()
            .contains("All Apps"));

        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 1);
        let create = &ops[0]["campaignCriterionOperation"]["create"];
        assert_eq!(
            create["campaign"],
            "customers/1234567890/campaigns/9000000002"
        );
        assert_eq!(create["negative"], true);
        assert_eq!(
            create["mobileAppCategory"]["mobileAppCategoryConstant"],
            "mobileAppCategoryConstants/69500"
        );
    }

    #[test]
    fn mixes_websites_categories_and_apps() {
        let config = Config::default();
        let preview = exclude_placements(
            &config,
            "1234567890",
            "555",
            &strings(&["https://www.example.com/", "www.example.com"]),
            &strings(&["mobileAppCategoryConstants/60008"]),
            &strings(&["2-com.example.game", "1-123456789"]),
        )
        .expect("ok");

        // The two spellings of example.com collapse into one criterion.
        assert_eq!(preview["changes"]["total_exclusions"], 4);
        let ops = operations_of(&preview);
        assert_eq!(ops.len(), 4);
        let c = |i: usize| ops[i]["campaignCriterionOperation"]["create"].clone();
        assert_eq!(c(0)["placement"]["url"], "www.example.com");
        assert_eq!(
            c(1)["mobileAppCategory"]["mobileAppCategoryConstant"],
            "mobileAppCategoryConstants/60008"
        );
        assert_eq!(c(2)["mobileApplication"]["appId"], "2-com.example.game");
        assert_eq!(c(3)["mobileApplication"]["appId"], "1-123456789");
        assert!(ops
            .iter()
            .all(|op| op["campaignCriterionOperation"]["create"]["negative"] == true));
    }

    #[test]
    fn nothing_to_exclude_rejected() {
        let config = Config::default();
        let err =
            exclude_placements(&config, "1234567890", "555", &[], &[], &[]).expect_err("rejected");
        assert!(err.to_string().contains("nothing to exclude"));
    }

    #[test]
    fn invalid_inputs_rejected() {
        let config = Config::default();
        let bad_campaign = exclude_placements(
            &config,
            "1234567890",
            "Display",
            &[],
            &strings(&["69500"]),
            &[],
        );
        assert!(bad_campaign.unwrap_err().to_string().contains("numeric"));

        let bad_url = exclude_placements(
            &config,
            "1234567890",
            "555",
            &strings(&["not a site"]),
            &[],
            &[],
        );
        assert!(bad_url.unwrap_err().to_string().contains("not a website"));

        let bad_category =
            exclude_placements(&config, "1234567890", "555", &[], &strings(&["Games"]), &[]);
        assert!(bad_category.unwrap_err().to_string().contains("numeric"));

        let bad_app = exclude_placements(
            &config,
            "1234567890",
            "555",
            &[],
            &[],
            &strings(&["com.example.game"]),
        );
        assert!(bad_app
            .unwrap_err()
            .to_string()
            .contains("'2-<package name>'"));

        let bad_ios =
            exclude_placements(&config, "1234567890", "555", &[], &[], &strings(&["1-abc"]));
        assert!(bad_ios.is_err());
    }

    #[test]
    fn blocked_operation_rejected() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["exclude_placements".to_string()];
        let err = exclude_placements(&config, "1234567890", "555", &[], &strings(&["69500"]), &[])
            .expect_err("blocked");
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn url_normalization() {
        assert_eq!(
            normalize_placement_url("HTTPS://Example.com/news/").unwrap(),
            "Example.com/news"
        );
        assert_eq!(
            normalize_placement_url("http://example.com").unwrap(),
            "example.com"
        );
        assert!(normalize_placement_url("https://").is_err());
    }
}
