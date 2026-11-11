//! Placement targeting and exclusions for Display.
//!
//! Where a Display ad may appear is set at three levels:
//!
//! - **Campaign** ([`exclude_placements`]): NEGATIVE campaign criteria for a
//!   website (`placement.url`), a mobile app category, or a single mobile app.
//!   One exclusion covers every ad group in the campaign, including later ones.
//! - **Ad group** ([`add_ad_group_placements`]): POSITIVE placements (the ad
//!   group only shows on those sites) or NEGATIVE ones for that ad group alone.
//! - **Account** ([`add_account_exclusions`]): `CustomerNegativeCriterion`
//!   entries — websites, app categories, apps and content labels — covering
//!   every campaign in the account.
//!
//! The common exclusion is app inventory: games and utility apps often bring
//! cheap accidental clicks that never convert. Mobile app category `69500` is
//! "All Apps" (look the categories up with `SELECT mobile_app_category_constant.id,
//! mobile_app_category_constant.name FROM mobile_app_category_constant`), so
//! excluding it keeps ads off every app placement in one criterion.
//!
//! Placement URL limits come from the criteria guide: at most 250 characters,
//! at most two levels deep, and `adsenseformobileapps.com` is not accepted.

use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::ContentLabelType;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::{
    check_fanout, dedupe_preserving_order, validate_id_list, validate_numeric_id,
    MAX_CRITERIA_PER_PLAN,
};

/// Mobile app category constant ID for "All Apps".
pub const ALL_APPS_CATEGORY_ID: &str = "69500";

/// Longest placement URL Google accepts, in characters.
const MAX_PLACEMENT_URL_CHARS: usize = 250;

/// The one domain the criteria guide says is never accepted as a placement.
const DISALLOWED_PLACEMENT_DOMAIN: &str = "adsenseformobileapps.com";

/// Normalize a placement URL to the bare host-and-path form Google stores.
///
/// Strips an `http://` / `https://` scheme and a trailing slash, so
/// `https://www.example.com/` and `www.example.com` become the same criterion
/// instead of two that differ only in spelling. Refuses a URL over 250
/// characters and `adsenseformobileapps.com`, which Google does not accept.
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
    let chars = url.chars().count();
    if chars > MAX_PLACEMENT_URL_CHARS {
        return Err(McpGoogleAdsError::Validation(format!(
            "placement URL is {} characters, over the {} limit",
            chars, MAX_PLACEMENT_URL_CHARS
        )));
    }
    let host = url
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if host == DISALLOWED_PLACEMENT_DOMAIN
        || host.ends_with(&format!(".{}", DISALLOWED_PLACEMENT_DOMAIN))
    {
        return Err(McpGoogleAdsError::Validation(format!(
            "Google does not accept {} as a placement — exclude app inventory with mobile \
             app category {} ('All Apps') instead",
            DISALLOWED_PLACEMENT_DOMAIN, ALL_APPS_CATEGORY_ID
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

/// Add website placements to ad groups — as targeting, or as exclusions for
/// those ad groups only.
///
/// `negative = false` (the default) writes POSITIVE placements: the ad group's
/// ads only show on these sites, together with its other targeting.
/// `negative = true` writes NEGATIVE placements that keep just these ad groups
/// off the sites; `mobile_app_ids` (`1-<App Store id>` / `2-<package name>`)
/// can then also be excluded. Positive mobile app targeting is not offered.
///
/// One `adGroupCriterionOperation.create` per (ad group × placement). A
/// positive and a negative row for the same placement on the same ad group
/// share a resource name, so the second create fails until the first is
/// removed.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
/// Reversible (remove the resulting criteria), so no double confirmation.
pub fn add_ad_group_placements(
    config: &Config,
    customer_id: &str,
    ad_group_ids: &[String],
    placement_urls: &[String],
    negative: bool,
    mobile_app_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("add_ad_group_placements", &config.safety)?;

    if !negative && !mobile_app_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "mobile_app_ids are only accepted with negative=true — this tool can exclude \
             single apps from ad groups, not target them"
                .to_string(),
        ));
    }
    if placement_urls.is_empty() && mobile_app_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "nothing to add — supply placement_urls (and, with negative=true, mobile_app_ids)"
                .to_string(),
        ));
    }

    let ad_groups = validate_id_list(
        "ad_group_ids",
        "ad_group_id",
        ad_group_ids,
        "name every ad group the placements apply to",
    )?;
    let urls = dedupe_preserving_order(
        &placement_urls
            .iter()
            .map(|u| normalize_placement_url(u))
            .collect::<Result<Vec<_>>>()?,
    );
    let apps = dedupe_preserving_order(
        &mobile_app_ids
            .iter()
            .map(|a| validate_app_id(a))
            .collect::<Result<Vec<_>>>()?,
    );
    let total = check_fanout(ad_groups.len(), urls.len() + apps.len(), "placements")?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let mut operations = Vec::with_capacity(total);
    for ad_group_id in &ad_groups {
        let ad_group = format!("customers/{}/adGroups/{}", cid, ad_group_id);
        for url in &urls {
            let mut create = json!({ "adGroup": ad_group, "placement": { "url": url } });
            if negative {
                create["negative"] = json!(true);
            }
            operations.push(json!({ "adGroupCriterionOperation": { "create": create } }));
        }
        for app in &apps {
            operations.push(json!({
                "adGroupCriterionOperation": {
                    "create": {
                        "adGroup": ad_group,
                        "negative": true,
                        "mobileApplication": { "appId": app }
                    }
                }
            }));
        }
    }

    let note = if negative {
        "Negative placements keep only these ad groups off the sites and apps. For a whole \
         campaign use exclude_placements; for the whole account, add_account_exclusions."
    } else {
        "Positive placements are targeting: these ad groups only show on the listed sites \
         (together with their other targeting). Placement URLs can be at most two levels deep."
    };
    let changes = json!({
        "ad_group_ids": ad_groups,
        "placement_urls": urls,
        "mobile_app_ids": apps,
        "negative": negative,
        "total_criteria": total,
        "note": note,
    });

    let plan = ChangePlan::new(
        "add_ad_group_placements".to_string(),
        "ad_group_criterion".to_string(),
        ad_groups.first().cloned().unwrap_or_default(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Account-level exclusions (`CustomerNegativeCriterion`), either as
/// requested by a caller or as found on the account.
///
/// Placement URLs are held normalized (see `normalize_placement_url`), app
/// categories as bare numeric IDs, apps as `1-…` / `2-…` app IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountExclusions {
    pub placement_urls: Vec<String>,
    pub mobile_app_category_ids: Vec<String>,
    pub mobile_app_ids: Vec<String>,
    pub content_labels: Vec<ContentLabelType>,
}

impl AccountExclusions {
    /// Number of exclusions held.
    pub fn len(&self) -> usize {
        self.placement_urls.len()
            + self.mobile_app_category_ids.len()
            + self.mobile_app_ids.len()
            + self.content_labels.len()
    }

    /// True when no exclusion is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn summary(&self) -> serde_json::Value {
        json!({
            "placement_urls": self.placement_urls,
            "mobile_app_category_ids": self.mobile_app_category_ids,
            "mobile_app_ids": self.mobile_app_ids,
            "content_label_types": self
                .content_labels
                .iter()
                .map(|l| l.as_api_str())
                .collect::<Vec<_>>(),
        })
    }
}

/// Validate and normalize a request for account-level exclusions, before any
/// HTTP traffic. At least one exclusion is required.
pub fn prepare_account_exclusions(
    config: &Config,
    placement_urls: &[String],
    mobile_app_category_ids: &[String],
    mobile_app_ids: &[String],
    content_labels: &[ContentLabelType],
) -> Result<AccountExclusions> {
    check_blocked_operation("add_account_exclusions", &config.safety)?;

    if placement_urls.is_empty()
        && mobile_app_category_ids.is_empty()
        && mobile_app_ids.is_empty()
        && content_labels.is_empty()
    {
        return Err(McpGoogleAdsError::Validation(
            "nothing to exclude — supply at least one of placement_urls, \
             mobile_app_category_ids, mobile_app_ids or content_label_types"
                .to_string(),
        ));
    }

    let requested = AccountExclusions {
        placement_urls: dedupe_preserving_order(
            &placement_urls
                .iter()
                .map(|u| normalize_placement_url(u))
                .collect::<Result<Vec<_>>>()?,
        ),
        mobile_app_category_ids: dedupe_preserving_order(
            &mobile_app_category_ids
                .iter()
                .map(|c| normalize_category_id(c))
                .collect::<Result<Vec<_>>>()?,
        ),
        mobile_app_ids: dedupe_preserving_order(
            &mobile_app_ids
                .iter()
                .map(|a| validate_app_id(a))
                .collect::<Result<Vec<_>>>()?,
        ),
        content_labels: dedupe_preserving_order(content_labels),
    };

    if requested.len() > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "plan would write {} exclusions, over the {} limit — split it into smaller batches",
            requested.len(),
            MAX_CRITERIA_PER_PLAN
        )));
    }
    Ok(requested)
}

/// Read the account's existing exclusions of the kinds this server writes.
///
/// Runs one GAQL query on `customer_negative_criterion`. Rows of other kinds
/// (YouTube, IP blocks, lists) and content labels this API version cannot
/// name are ignored: they can never collide with a requested exclusion.
pub async fn fetch_account_exclusions(
    client: &GoogleAdsClient,
    customer_id: &str,
) -> Result<AccountExclusions> {
    let query = "SELECT customer_negative_criterion.resource_name, \
                 customer_negative_criterion.type, \
                 customer_negative_criterion.placement.url, \
                 customer_negative_criterion.mobile_app_category.mobile_app_category_constant, \
                 customer_negative_criterion.mobile_application.app_id, \
                 customer_negative_criterion.content_label.type \
                 FROM customer_negative_criterion";
    let rows = client.search(customer_id, query).await?;

    let mut existing = AccountExclusions::default();
    for row in &rows {
        let criterion = &row["customerNegativeCriterion"];
        if let Some(url) = criterion.pointer("/placement/url").and_then(|v| v.as_str()) {
            existing.placement_urls.push(url.to_string());
        }
        if let Some(category) = criterion
            .pointer("/mobileAppCategory/mobileAppCategoryConstant")
            .and_then(|v| v.as_str())
        {
            if let Some(id) = category.rsplit('/').next() {
                existing.mobile_app_category_ids.push(id.to_string());
            }
        }
        if let Some(app) = criterion
            .pointer("/mobileApplication/appId")
            .and_then(|v| v.as_str())
        {
            existing.mobile_app_ids.push(app.to_string());
        }
        if let Some(label) = criterion.pointer("/contentLabel/type") {
            if let Ok(label) = serde_json::from_value::<ContentLabelType>(label.clone()) {
                existing.content_labels.push(label);
            }
        }
    }
    Ok(existing)
}

/// Build the plan for the requested exclusions that the account does not
/// already have.
///
/// Creating an exclusion that already exists fails the whole (atomic) mutate,
/// so existing ones are skipped and listed under `already_excluded`. Placement
/// URLs match case-insensitively. When nothing new remains, no plan is stored
/// and the answer says so (`status: NO_CHANGES`).
///
/// Refuses a request that would leave all five digital content labels
/// excluded: Google then has no Display inventory left to serve on.
pub fn build_account_exclusions_plan(
    config: &Config,
    customer_id: &str,
    requested: &AccountExclusions,
    existing: &AccountExclusions,
) -> Result<serde_json::Value> {
    check_blocked_operation("add_account_exclusions", &config.safety)?;

    // Google's stored spelling may differ from ours in case, scheme or a
    // trailing slash; compare on a form that ignores all three.
    let comparable = |url: &str| {
        let lower = url.trim().to_ascii_lowercase();
        let bare = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"))
            .unwrap_or(&lower);
        bare.trim_end_matches('/').to_string()
    };
    let existing_urls: std::collections::HashSet<String> = existing
        .placement_urls
        .iter()
        .map(|u| comparable(u))
        .collect();

    let mut to_add = AccountExclusions::default();
    let mut skipped = AccountExclusions::default();
    for url in &requested.placement_urls {
        if existing_urls.contains(&comparable(url)) {
            skipped.placement_urls.push(url.clone());
        } else {
            to_add.placement_urls.push(url.clone());
        }
    }
    for category in &requested.mobile_app_category_ids {
        if existing.mobile_app_category_ids.contains(category) {
            skipped.mobile_app_category_ids.push(category.clone());
        } else {
            to_add.mobile_app_category_ids.push(category.clone());
        }
    }
    for app in &requested.mobile_app_ids {
        if existing.mobile_app_ids.contains(app) {
            skipped.mobile_app_ids.push(app.clone());
        } else {
            to_add.mobile_app_ids.push(app.clone());
        }
    }
    for label in &requested.content_labels {
        if existing.content_labels.contains(label) {
            skipped.content_labels.push(*label);
        } else {
            to_add.content_labels.push(*label);
        }
    }

    let all_digital_labels_excluded = ContentLabelType::DIGITAL_CONTENT_LABELS
        .iter()
        .all(|dl| existing.content_labels.contains(dl) || to_add.content_labels.contains(dl));
    if all_digital_labels_excluded
        && to_add
            .content_labels
            .iter()
            .any(|l| l.is_digital_content_label())
    {
        return Err(McpGoogleAdsError::Validation(
            "this would exclude all five digital content labels (VIDEO_RATING_DV_G, _PG, _T, \
             _MA and VIDEO_NOT_YET_RATED) — Google then stops serving Display ads. Keep at \
             least one label open."
                .to_string(),
        ));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);

    if to_add.is_empty() {
        return Ok(json!({
            "status": "NO_CHANGES",
            "operation": "add_account_exclusions",
            "customer_id": cid,
            "already_excluded": skipped.summary(),
            "message": "Every requested exclusion already exists on the account. No plan was \
                        created; there is nothing to confirm.",
        }));
    }

    let negative = |criterion: (&str, serde_json::Value)| {
        let mut create = json!({});
        create[criterion.0] = criterion.1;
        json!({ "customerNegativeCriterionOperation": { "create": create } })
    };
    let mut operations = Vec::with_capacity(to_add.len());
    for url in &to_add.placement_urls {
        operations.push(negative(("placement", json!({ "url": url }))));
    }
    for category in &to_add.mobile_app_category_ids {
        operations.push(negative((
            "mobileAppCategory",
            json!({
                "mobileAppCategoryConstant": format!("mobileAppCategoryConstants/{}", category)
            }),
        )));
    }
    for app in &to_add.mobile_app_ids {
        operations.push(negative(("mobileApplication", json!({ "appId": app }))));
    }
    for label in &to_add.content_labels {
        operations.push(negative((
            "contentLabel",
            json!({ "type": label.as_api_str() }),
        )));
    }

    let caveats: Vec<serde_json::Value> = to_add
        .content_labels
        .iter()
        .filter_map(|label| {
            label
                .display_caveat()
                .map(|caveat| json!({ "content_label_type": label.as_api_str(), "caveat": caveat }))
        })
        .collect();

    let mut changes = json!({
        "to_add": to_add.summary(),
        "already_excluded": skipped.summary(),
        "total_new_exclusions": to_add.len(),
        "negative": true,
        "note": "Account-level exclusions cover every campaign in the account, including \
                 campaigns created later.",
    });
    if !caveats.is_empty() {
        changes["content_label_caveats"] = json!(caveats);
    }
    if to_add
        .mobile_app_category_ids
        .iter()
        .any(|c| c == ALL_APPS_CATEGORY_ID)
    {
        changes["app_note"] = json!(
            "Category 69500 is 'All Apps': no campaign in the account serves on mobile app \
             placements any more."
        );
    }

    let plan = ChangePlan::new(
        "add_account_exclusions".to_string(),
        "customer_negative_criterion".to_string(),
        cid.clone(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Validate a request and build its plan against the given existing
/// exclusions — [`prepare_account_exclusions`] followed by
/// [`build_account_exclusions_plan`]. The MCP tool fetches `existing` with
/// [`fetch_account_exclusions`] in between.
pub fn add_account_exclusions(
    config: &Config,
    customer_id: &str,
    placement_urls: &[String],
    mobile_app_category_ids: &[String],
    mobile_app_ids: &[String],
    content_labels: &[ContentLabelType],
    existing: &AccountExclusions,
) -> Result<serde_json::Value> {
    let requested = prepare_account_exclusions(
        config,
        placement_urls,
        mobile_app_category_ids,
        mobile_app_ids,
        content_labels,
    )?;
    build_account_exclusions_plan(config, customer_id, &requested, existing)
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
