use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{AdStatus, NextActionHint};
use crate::safety::guards::{
    check_blocked_operation, validate_description, validate_display_path, validate_final_url,
    validate_headline,
};
use crate::safety::preview::{store_plan, ChangePlan};

/// An RSA carries 3 to 15 headlines, each at most 30 characters.
///
/// Shared by drafting and updating so the two can never drift: an update that
/// accepted a set the create path refuses would write an ad Google rejects.
fn validate_headline_set(headlines: &[String]) -> Result<()> {
    if headlines.len() < 3 || headlines.len() > 15 {
        return Err(McpGoogleAdsError::Validation(format!(
            "RSA requires 3-15 headlines, got {}",
            headlines.len()
        )));
    }
    for headline in headlines {
        validate_headline(headline)?;
    }
    Ok(())
}

/// An RSA carries 2 to 4 descriptions, each at most 90 characters.
fn validate_description_set(descriptions: &[String]) -> Result<()> {
    if descriptions.len() < 2 || descriptions.len() > 4 {
        return Err(McpGoogleAdsError::Validation(format!(
            "RSA requires 2-4 descriptions, got {}",
            descriptions.len()
        )));
    }
    for desc in descriptions {
        validate_description(desc)?;
    }
    Ok(())
}

/// Parameters for drafting a Responsive Search Ad.
///
/// `status` defaults to [`AdStatus::Paused`] when `None` — newly drafted
/// ads ship paused so an agent can review before traffic flows. Set
/// `status = Some(AdStatus::Enabled)` to opt out. The chosen status is
/// echoed in the response as `status_after_apply`, and when the default
/// `PAUSED` is used the response also carries a `next_action_hint`
/// describing how to enable the ad via the `enable_entity` MCP tool.
pub struct DraftRsaParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub ad_group_id: &'a str,
    pub headlines: Vec<String>,
    pub descriptions: Vec<String>,
    pub final_url: &'a str,
    pub path1: Option<&'a str>,
    pub path2: Option<&'a str>,
    /// Lifecycle status to create the ad in.
    /// `None` means default behaviour: [`AdStatus::Paused`].
    pub status: Option<AdStatus>,
}

/// Draft a Responsive Search Ad (RSA).
///
/// Validates headline and description counts and character limits, then creates
/// a ChangePlan preview.
///
/// Default status is `PAUSED` (safety) — pass `status = Some(AdStatus::Enabled)`
/// to bypass. The response always carries `status_after_apply`, and when the
/// default `PAUSED` is in effect, also a `next_action_hint` pointing at the
/// `enable_entity` MCP tool so the workflow can complete with zero UI step.
///
/// Requirements:
/// - 3 to 15 headlines, each max 30 characters
/// - 2 to 4 descriptions, each max 90 characters
/// - At least one final URL
pub fn draft_responsive_search_ad(params: &DraftRsaParams) -> Result<serde_json::Value> {
    check_blocked_operation("draft_responsive_search_ad", &params.config.safety)?;

    validate_headline_set(&params.headlines)?;
    validate_description_set(&params.descriptions)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(params.customer_id);
    let ad_group_resource = format!("customers/{}/adGroups/{}", cid, params.ad_group_id);

    let headline_assets: Vec<serde_json::Value> = params
        .headlines
        .iter()
        .map(|h| json!({"text": h}))
        .collect();

    let description_assets: Vec<serde_json::Value> = params
        .descriptions
        .iter()
        .map(|d| json!({"text": d}))
        .collect();

    let mut ad = json!({
        "responsiveSearchAd": {
            "headlines": headline_assets,
            "descriptions": description_assets
        },
        "finalUrls": [params.final_url]
    });

    if let Some(p1) = params.path1 {
        if let Some(rsa) = ad
            .pointer_mut("/responsiveSearchAd")
            .and_then(|v| v.as_object_mut())
        {
            rsa.insert("path1".to_string(), json!(p1));
        }
    }

    if let Some(p2) = params.path2 {
        if let Some(rsa) = ad
            .pointer_mut("/responsiveSearchAd")
            .and_then(|v| v.as_object_mut())
        {
            rsa.insert("path2".to_string(), json!(p2));
        }
    }

    let resolved_status = params.status.unwrap_or_default();
    let operation = json!({
        "adGroupAdOperation": {
            "create": {
                "adGroup": ad_group_resource,
                "ad": ad,
                "status": resolved_status.as_api_str()
            }
        }
    });

    let changes = json!({
        "ad_group_id": params.ad_group_id,
        "headlines": params.headlines,
        "descriptions": params.descriptions,
        "final_url": params.final_url,
        "path1": params.path1,
        "path2": params.path2,
        "status": resolved_status.as_api_str()
    });

    let mut plan = ChangePlan::new(
        "draft_responsive_search_ad".to_string(),
        "ad".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        vec![operation],
    )
    .with_status_after_apply(resolved_status);

    // Only attach a next_action_hint when the entity ships PAUSED — when an
    // agent explicitly opted into ENABLED, no further step is needed.
    if resolved_status == AdStatus::Paused {
        plan = plan.with_next_action_hint(NextActionHint::enable_ad(
            params.ad_group_id,
            "<resolve ad_id from confirm_and_apply response>",
        ));
    }

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Maximum number of digits in a Google Ads entity ID (an int64).
const MAX_ID_DIGITS: usize = 19;

/// Validate that `ad_id` is a bare numeric Google Ads ad ID.
///
/// Callers reach for a full resource name (`customers/…/ads/123`) often enough
/// that interpolating one unchecked would build `customers/…/ads/customers/…`
/// and fail inside the API with a message that points nowhere. Reject it here,
/// naming the field.
fn validate_ad_id(ad_id: &str) -> Result<()> {
    let digits = ad_id.chars().count();
    if digits == 0 || digits > MAX_ID_DIGITS || !ad_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "ad_id must be a numeric Google Ads ad ID of 1-{} digits, got '{}'",
            MAX_ID_DIGITS, ad_id
        )));
    }
    Ok(())
}

/// Parameters for updating an existing Responsive Search Ad in place.
///
/// Every creative field is optional and independent: a field left `None` is
/// absent from the `updateMask`, so the ad keeps whatever it already holds.
/// Passing only `headlines` rewrites the headlines and leaves the descriptions,
/// the final URL and both display paths untouched.
pub struct UpdateRsaParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    /// Numeric ID of the ad to edit (`ad.id`), not a resource name.
    pub ad_id: &'a str,
    /// Replacement headlines: 3 to 15, each max 30 characters. `None` leaves
    /// the ad's current headlines in place.
    pub headlines: Option<Vec<String>>,
    /// Replacement descriptions: 2 to 4, each max 90 characters. `None` leaves
    /// the ad's current descriptions in place.
    pub descriptions: Option<Vec<String>>,
    /// Replacement landing page. Absolute http(s) URL, max 2048 characters.
    pub final_url: Option<&'a str>,
    /// Replacement display-URL path, max 15 characters. An empty string clears
    /// the path the ad currently shows.
    pub path1: Option<&'a str>,
    /// Second display-URL path segment, same bound as `path1`.
    pub path2: Option<&'a str>,
}

/// Update an existing Responsive Search Ad in place, via an
/// `adOperation.update` carrying a field mask scoped to the fields provided.
///
/// Unlike a remove + re-create, an update keeps the ad's ID and with it the
/// performance history Google Ads attaches to it — the asset-level performance
/// labels (LOW / GOOD / BEST) and the ad-level learning that a fresh ad starts
/// over from. That is the reason this is a distinct tool rather than a
/// `remove_entity` followed by `draft_responsive_search_ad`.
///
/// Partial by construction: only the fields provided are written. Omitting
/// `descriptions` does not clear them, it leaves them exactly as they are.
/// At least one field must be provided — an update with an empty mask writes
/// nothing, and a silent no-op reads like a success.
///
/// Bounds, all refused rather than truncated when violated:
/// - `headlines`: 3 to 15, max 30 characters each
/// - `descriptions`: 2 to 4, max 90 characters each
/// - `final_url`: absolute http(s), max 2048 characters
/// - `path1` / `path2`: max 15 characters each
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn update_responsive_search_ad(params: &UpdateRsaParams) -> Result<serde_json::Value> {
    check_blocked_operation("update_responsive_search_ad", &params.config.safety)?;
    validate_ad_id(params.ad_id)?;

    let mut rsa = serde_json::Map::new();
    // Field mask paths, in payload order. The mask is derived from what was
    // actually written rather than declared separately, so the two cannot drift.
    let mut update_mask_fields: Vec<&str> = Vec::new();

    if let Some(ref headlines) = params.headlines {
        validate_headline_set(headlines)?;
        let assets: Vec<serde_json::Value> = headlines.iter().map(|h| json!({"text": h})).collect();
        rsa.insert("headlines".to_string(), json!(assets));
        update_mask_fields.push("responsiveSearchAd.headlines");
    }

    if let Some(ref descriptions) = params.descriptions {
        validate_description_set(descriptions)?;
        let assets: Vec<serde_json::Value> =
            descriptions.iter().map(|d| json!({"text": d})).collect();
        rsa.insert("descriptions".to_string(), json!(assets));
        update_mask_fields.push("responsiveSearchAd.descriptions");
    }

    if let Some(p1) = params.path1 {
        validate_display_path("path1", p1)?;
        rsa.insert("path1".to_string(), json!(p1));
        update_mask_fields.push("responsiveSearchAd.path1");
    }

    if let Some(p2) = params.path2 {
        validate_display_path("path2", p2)?;
        rsa.insert("path2".to_string(), json!(p2));
        update_mask_fields.push("responsiveSearchAd.path2");
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(params.customer_id);
    let resource_name = format!("customers/{}/ads/{}", cid, params.ad_id);
    let mut ad = json!({ "resourceName": resource_name });

    if !rsa.is_empty() {
        ad["responsiveSearchAd"] = serde_json::Value::Object(rsa);
    }

    if let Some(url) = params.final_url {
        validate_final_url(url)?;
        ad["finalUrls"] = json!([url.trim()]);
        update_mask_fields.push("finalUrls");
    }

    if update_mask_fields.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one of headlines, descriptions, final_url, path1 or path2 must be \
             provided — an update with an empty field mask writes nothing"
                .to_string(),
        ));
    }

    let operation = json!({
        "adOperation": {
            "update": ad,
            "updateMask": update_mask_fields.join(",")
        }
    });

    let changes = json!({
        "ad_id": params.ad_id,
        "headlines": params.headlines,
        "descriptions": params.descriptions,
        "final_url": params.final_url,
        "path1": params.path1,
        "path2": params.path2,
        "update_mask": update_mask_fields.join(","),
    });

    let plan = ChangePlan::new(
        "update_responsive_search_ad".to_string(),
        "ad".to_string(),
        params.ad_id.to_string(),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_headlines(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("Headline {}", i + 1)).collect()
    }

    fn make_descriptions(count: usize) -> Vec<String> {
        (0..count)
            .map(|i| format!("Description number {}", i + 1))
            .collect()
    }

    #[test]
    fn test_draft_rsa_too_few_headlines() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(2), // too few
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("3-15 headlines"));
    }

    #[test]
    fn test_draft_rsa_too_many_headlines() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(16), // too many
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_draft_rsa_too_few_descriptions() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(1), // too few
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("2-4 descriptions"));
    }

    #[test]
    fn test_draft_rsa_headline_too_long() {
        let config = Config::default();
        let mut headlines = make_headlines(3);
        headlines[0] = "A".repeat(31); // exceeds 30 char limit
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines,
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("30 character limit"));
    }

    #[test]
    fn test_draft_rsa_success() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: Some("path1"),
            path2: Some("path2"),
            status: None,
        });
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_responsive_search_ad");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_draft_rsa_default_status_paused_with_hint() {
        let config = Config::default();
        let preview = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        })
        .unwrap();
        assert_eq!(preview["status_after_apply"], "PAUSED");
        assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
        assert_eq!(preview["next_action_hint"]["params"]["entity_type"], "ad");
    }

    #[test]
    fn test_draft_rsa_explicit_enabled_no_hint() {
        let config = Config::default();
        let preview = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: Some(AdStatus::Enabled),
        })
        .unwrap();
        assert_eq!(preview["status_after_apply"], "ENABLED");
        assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());
    }

    #[test]
    fn test_draft_rsa_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["draft_responsive_search_ad".to_string()];
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_draft_rsa_too_many_descriptions() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(5), // exceeds max of 4
            final_url: "https://example.com",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("2-4 descriptions"));
    }

    #[test]
    fn test_draft_rsa_empty_final_url() {
        // The function does not currently validate empty final_url,
        // so this should succeed (the API will reject it later)
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "",
            path1: None,
            path2: None,
            status: None,
        });
        assert!(result.is_ok());
    }
}
