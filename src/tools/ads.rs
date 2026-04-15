use crate::client::GoogleAdsClient;
use crate::error::Result;
use crate::gaql;

/// Get ad-level performance metrics.
///
/// Returns all non-removed ads ordered by cost descending, with enriched cost fields.
pub async fn get_ad_performance(
    client: &GoogleAdsClient,
    customer_id: &str,
    date_start: Option<&str>,
    date_end: Option<&str>,
) -> Result<String> {
    let date_clause = match (date_start, date_end) {
        (Some(s), Some(e)) => format!(" AND {}", gaql::date_clause(s, e)),
        _ => String::new(),
    };

    let query = format!(
        "SELECT \
            campaign.name, \
            campaign.id, \
            ad_group.name, \
            ad_group.id, \
            ad_group_ad.ad.id, \
            ad_group_ad.ad.type, \
            ad_group_ad.ad.responsive_search_ad.headlines, \
            ad_group_ad.ad.responsive_search_ad.descriptions, \
            ad_group_ad.ad.final_urls, \
            ad_group_ad.status, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.ctr, \
            metrics.conversions, \
            metrics.cost_micros \
        FROM ad_group_ad \
        WHERE ad_group_ad.status != 'REMOVED'{} \
        ORDER BY metrics.cost_micros DESC",
        date_clause
    );

    let mut rows = client.search(customer_id, &query).await?;
    gaql::enrich_cost_fields(&mut rows);

    let result = serde_json::json!({
        "ads": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
