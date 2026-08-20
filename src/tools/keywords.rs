use crate::client::GoogleAdsClient;
use crate::error::{McpGoogleAdsError, Result};
use crate::gaql;

/// Get keyword-level performance metrics.
pub async fn get_keyword_performance(
    client: &GoogleAdsClient,
    customer_id: &str,
    date_start: Option<&str>,
    date_end: Option<&str>,
) -> Result<String> {
    let date_clause = match (date_start, date_end) {
        (Some(s), Some(e)) => format!(" AND {}", gaql::date_clause(s, e)),
        // Default to the last 30 days so we never silently return lifetime totals.
        _ => " AND segments.date DURING LAST_30_DAYS".to_string(),
    };

    let query = format!(
        "SELECT \
            campaign.name, \
            ad_group.name, \
            ad_group_criterion.keyword.text, \
            ad_group_criterion.keyword.match_type, \
            ad_group_criterion.quality_info.quality_score, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.ctr, \
            metrics.average_cpc, \
            metrics.cost_micros, \
            metrics.conversions \
        FROM keyword_view \
        WHERE ad_group_criterion.status != 'REMOVED'{} \
        ORDER BY metrics.cost_micros DESC",
        date_clause
    );

    let mut rows = client.search(customer_id, &query).await?;
    gaql::enrich_cost_fields(&mut rows);

    let result = serde_json::json!({
        "keywords": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Rows a single search-terms report returns when the caller does not say.
/// Matches the previously hardcoded value, so omitting `limit` is a no-op.
const DEFAULT_SEARCH_TERMS_LIMIT: u32 = 200;

/// Hard ceiling on the row count, so a caller cannot ask for an unbounded
/// response that neither side can handle.
const MAX_SEARCH_TERMS_LIMIT: u32 = 10_000;

/// Get search terms report showing actual user queries that triggered ads.
pub async fn get_search_terms(
    client: &GoogleAdsClient,
    customer_id: &str,
    date_start: Option<&str>,
    date_end: Option<&str>,
    limit: Option<u32>,
) -> Result<String> {
    // The report is unbounded by nature — a broad campaign produces thousands
    // of distinct queries — so the row count is always capped, and a caller who
    // asks for an impossible one is told rather than quietly served the default.
    let limit = match limit {
        None => DEFAULT_SEARCH_TERMS_LIMIT,
        Some(n) if (1..=MAX_SEARCH_TERMS_LIMIT).contains(&n) => n,
        Some(n) => {
            return Err(McpGoogleAdsError::Validation(format!(
                "limit must be between 1 and {MAX_SEARCH_TERMS_LIMIT}, got {n}"
            )));
        }
    };

    let date_clause = match (date_start, date_end) {
        (Some(s), Some(e)) => gaql::date_clause(s, e),
        _ => "segments.date DURING LAST_30_DAYS".to_string(),
    };

    let query = format!(
        "SELECT \
            search_term_view.search_term, \
            campaign.name, \
            ad_group.name, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.cost_micros, \
            metrics.conversions \
        FROM search_term_view \
        WHERE {} \
        ORDER BY metrics.clicks DESC \
        LIMIT {}",
        date_clause, limit
    );

    let mut rows = client.search(customer_id, &query).await?;
    gaql::enrich_cost_fields(&mut rows);

    let result = serde_json::json!({
        "search_terms": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get negative keywords at the campaign level.
pub async fn get_negative_keywords(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            campaign.id, \
            campaign.name, \
            campaign_criterion.keyword.text, \
            campaign_criterion.keyword.match_type, \
            campaign_criterion.negative, \
            campaign_criterion.criterion_id \
        FROM campaign_criterion \
        WHERE campaign_criterion.negative = TRUE \
            AND campaign_criterion.status != 'REMOVED'";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "negative_keywords": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
