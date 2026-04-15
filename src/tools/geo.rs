use crate::client::GoogleAdsClient;
use crate::error::Result;
use crate::gaql;

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
