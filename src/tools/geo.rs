use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::gaql;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Maximum number of digits accepted for a geo target constant ID.
/// Google geo target constant IDs are short numeric identifiers; anything
/// longer is a malformed input rather than a real location.
const MAX_GEO_TARGET_ID_LEN: usize = 12;

/// Search for geo target constants by name.
///
/// Useful for finding location IDs for geo-targeting campaigns.
pub async fn search_geo_targets(
    client: &GoogleAdsClient,
    customer_id: &str,
    query: &str,
) -> Result<String> {
    let gaql_query = format!(
        "SELECT \
            geo_target_constant.id, \
            geo_target_constant.name, \
            geo_target_constant.canonical_name, \
            geo_target_constant.country_code, \
            geo_target_constant.target_type \
        FROM geo_target_constant \
        WHERE geo_target_constant.name LIKE '%{}%'",
        query.replace('\'', "\\'")
    );

    let rows = client.search(customer_id, &gaql_query).await?;

    let result = serde_json::json!({
        "geo_targets": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get geographic performance data for campaigns.
pub async fn get_geo_performance(
    client: &GoogleAdsClient,
    customer_id: &str,
    date_start: Option<&str>,
    date_end: Option<&str>,
) -> Result<String> {
    let date_clause = match (date_start, date_end) {
        (Some(s), Some(e)) => gaql::date_clause(s, e),
        _ => "segments.date DURING LAST_30_DAYS".to_string(),
    };

    let query = format!(
        "SELECT \
            campaign.name, \
            geographic_view.country_criterion_id, \
            geographic_view.location_type, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.cost_micros, \
            metrics.conversions \
        FROM geographic_view \
        WHERE {} \
        ORDER BY metrics.cost_micros DESC",
        date_clause
    );

    let mut rows = client.search(customer_id, &query).await?;
    gaql::enrich_cost_fields(&mut rows);

    let result = serde_json::json!({
        "geo_performance": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Parse a geo target id into its bare numeric form.
///
/// Accepts the bare numeric id ("2276") or the full resource name
/// ("geoTargetConstants/2276"). Rejects empty, non-numeric, or over-long ids.
/// For LOCATION campaign criteria the criterion id equals this geo target
/// constant id, so the bare form is what builds both the `geoTargetConstant`
/// reference and the `campaignCriteria/{campaign}~{id}` resource name.
fn parse_geo_target_id(geo_target_id: &str) -> Result<String> {
    let trimmed = geo_target_id.trim();
    let id = trimmed
        .strip_prefix("geoTargetConstants/")
        .unwrap_or(trimmed);

    if id.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "geo_target_id must not be empty".to_string(),
        ));
    }
    if id.len() > MAX_GEO_TARGET_ID_LEN {
        return Err(McpGoogleAdsError::Validation(format!(
            "geo_target_id is too long (max {} digits), got '{}'",
            MAX_GEO_TARGET_ID_LEN, geo_target_id
        )));
    }
    if !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "geo_target_id must be numeric (e.g. '2276' or 'geoTargetConstants/2276'), got '{}'",
            geo_target_id
        )));
    }

    Ok(id.to_string())
}

/// Normalise a geo target id into a `geoTargetConstants/{id}` resource name.
fn normalize_geo_target_constant(geo_target_id: &str) -> Result<String> {
    Ok(format!(
        "geoTargetConstants/{}",
        parse_geo_target_id(geo_target_id)?
    ))
}

/// Exclude a geographic location from a campaign.
///
/// Creates a negative `campaignCriterionOperation` carrying a `location`
/// criterion — the inverse of the positive geo targeting that `update_campaign`
/// adds. Ads in `campaign_id` stop serving to users in `geo_target_id`.
///
/// `geo_target_id` accepts either the bare numeric ID ("2276") or the full
/// resource name ("geoTargetConstants/2276"); both normalise to the same
/// criterion. Discover IDs with `search_geo_targets`.
///
/// Returns a [`ChangePlan`] preview that must be confirmed via
/// `confirm_and_apply`. The exclusion is reversible (remove the resulting
/// campaign criterion) and therefore does not require double confirmation.
pub fn exclude_geo_target(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    geo_target_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("exclude_geo_target", &config.safety)?;

    let geo_target_constant = normalize_geo_target_constant(geo_target_id)?;
    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let operations = vec![json!({
        "campaignCriterionOperation": {
            "create": {
                "campaign": campaign_resource,
                "negative": true,
                "location": { "geoTargetConstant": geo_target_constant }
            }
        }
    })];

    let changes = json!({
        "campaign_id": campaign_id,
        "geo_target_constant": geo_target_constant,
        "negative": true,
    });

    let plan = ChangePlan::new(
        "exclude_geo_target".to_string(),
        "campaign_criterion".to_string(),
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

/// Remove a positively-targeted geographic location from a campaign.
///
/// Use this when a location is currently an *included* target (a positive
/// `campaign_criterion`) and you want to stop serving there — the common case
/// for trimming a country out of a multi-country campaign. It removes the
/// existing criterion rather than layering a negative one.
///
/// This is the necessary counterpart to [`exclude_geo_target`]: for LOCATION
/// criteria the criterion id equals the geo target constant id, so a campaign
/// that already targets `geo_target_id` positively cannot also receive a
/// negative criterion for the same id (it would collide on
/// `campaignCriteria/{campaign}~{id}`). Removing the positive criterion is the
/// correct operation in that case.
///
/// `geo_target_id` accepts the bare numeric ID ("2276") or the full resource
/// name ("geoTargetConstants/2276"). Returns a [`ChangePlan`] preview that must
/// be confirmed via `confirm_and_apply`; like the other removals it requires
/// double confirmation.
pub fn remove_geo_target(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    geo_target_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_geo_target", &config.safety)?;

    let geo_id = parse_geo_target_id(geo_target_id)?;
    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let criterion_resource = format!(
        "customers/{}/campaignCriteria/{}~{}",
        cid, campaign_id, geo_id
    );

    let operations = vec![json!({
        "campaignCriterionOperation": {
            "remove": criterion_resource
        }
    })];

    let changes = json!({
        "campaign_id": campaign_id,
        "geo_target_id": geo_id,
        "removed_criterion": format!("{}~{}", campaign_id, geo_id),
        "warning": "Removes the campaign's positive location target. Reversible by re-adding it.",
    });

    let plan = ChangePlan::new(
        "remove_geo_target".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::safety::preview::{get_plan, remove_plan};

    /// Extract the single `campaignCriterionOperation.create` object from a stored plan.
    fn created_criterion(preview: &serde_json::Value) -> serde_json::Value {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let op = plan
            .mutate_operations
            .first()
            .expect("one mutate operation")
            .clone();
        remove_plan(plan_id);
        op["campaignCriterionOperation"]["create"].clone()
    }

    #[test]
    fn test_exclude_geo_target_bare_id() {
        let config = Config::default();
        let preview =
            exclude_geo_target(&config, "123-456-7890", "23812937408", "2276").expect("ok");

        assert_eq!(preview["operation"], "exclude_geo_target");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        assert_eq!(preview["requires_double_confirm"], false);

        let create = created_criterion(&preview);
        // The whole point: it is a NEGATIVE location criterion on the campaign.
        assert_eq!(create["negative"], true);
        assert_eq!(
            create["location"]["geoTargetConstant"],
            "geoTargetConstants/2276"
        );
        assert_eq!(
            create["campaign"],
            "customers/1234567890/campaigns/23812937408"
        );
    }

    #[test]
    fn test_exclude_geo_target_full_resource_name_normalised() {
        let config = Config::default();
        let preview = exclude_geo_target(&config, "1234567890", "111", "geoTargetConstants/2276")
            .expect("ok");
        let create = created_criterion(&preview);
        // A full resource name is accepted and not double-prefixed.
        assert_eq!(
            create["location"]["geoTargetConstant"],
            "geoTargetConstants/2276"
        );
    }

    #[test]
    fn test_exclude_geo_target_empty_id_rejected() {
        let config = Config::default();
        let err =
            exclude_geo_target(&config, "1234567890", "111", "   ").expect_err("empty id rejected");
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_exclude_geo_target_non_numeric_rejected() {
        let config = Config::default();
        let err = exclude_geo_target(&config, "1234567890", "111", "Germany")
            .expect_err("non-numeric id rejected");
        assert!(err.to_string().contains("numeric"));
    }

    #[test]
    fn test_exclude_geo_target_too_long_rejected() {
        let config = Config::default();
        let long_id = "1".repeat(MAX_GEO_TARGET_ID_LEN + 1);
        let err = exclude_geo_target(&config, "1234567890", "111", &long_id)
            .expect_err("over-long id rejected");
        assert!(err.to_string().contains("too long"));
    }

    #[test]
    fn test_exclude_geo_target_blocked_operation_rejected() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["exclude_geo_target".to_string()];
        let err = exclude_geo_target(&config, "1234567890", "111", "2276")
            .expect_err("blocked operation rejected");
        assert!(err.to_string().contains("blocked"));
    }

    /// Extract the `campaignCriterionOperation.remove` resource string from a stored plan.
    fn removed_criterion(preview: &serde_json::Value) -> serde_json::Value {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let op = plan
            .mutate_operations
            .first()
            .expect("one mutate operation")
            .clone();
        remove_plan(plan_id);
        op["campaignCriterionOperation"]["remove"].clone()
    }

    #[test]
    fn test_remove_geo_target_builds_criterion_resource() {
        let config = Config::default();
        let preview =
            remove_geo_target(&config, "123-456-7890", "23812937408", "2276").expect("ok");

        assert_eq!(preview["operation"], "remove_geo_target");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        // Removals require double confirmation, like the other remove_* tools.
        assert_eq!(preview["requires_double_confirm"], true);

        // For LOCATION criteria the criterion id equals the geo target id.
        assert_eq!(
            removed_criterion(&preview),
            "customers/1234567890/campaignCriteria/23812937408~2276"
        );
    }

    #[test]
    fn test_remove_geo_target_accepts_full_resource_name() {
        let config = Config::default();
        let preview =
            remove_geo_target(&config, "1234567890", "111", "geoTargetConstants/2276").expect("ok");
        assert_eq!(
            removed_criterion(&preview),
            "customers/1234567890/campaignCriteria/111~2276"
        );
    }

    #[test]
    fn test_remove_geo_target_non_numeric_rejected() {
        let config = Config::default();
        let err = remove_geo_target(&config, "1234567890", "111", "DE")
            .expect_err("non-numeric id rejected");
        assert!(err.to_string().contains("numeric"));
    }

    #[test]
    fn test_remove_geo_target_blocked_operation_rejected() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["remove_geo_target".to_string()];
        let err = remove_geo_target(&config, "1234567890", "111", "2276")
            .expect_err("blocked operation rejected");
        assert!(err.to_string().contains("blocked"));
    }
}
