use crate::client::GoogleAdsClient;
use crate::error::Result;
use crate::gaql;

/// Get campaign-level performance metrics.
///
/// Returns all non-removed campaigns ordered by cost descending, with enriched cost fields.
pub async fn get_campaign_performance(
    client: &GoogleAdsClient,
    customer_id: &str,
    date_start: Option<&str>,
    date_end: Option<&str>,
) -> Result<String> {
    let date_clause = build_date_clause(date_start, date_end);

    let query = format!(
        "SELECT \
            campaign.id, \
            campaign.name, \
            campaign.status, \
            campaign.advertising_channel_type, \
            campaign.bidding_strategy_type, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.cost_micros, \
            metrics.conversions, \
            metrics.conversions_value, \
            metrics.ctr, \
            metrics.average_cpc \
        FROM campaign \
        WHERE campaign.status != 'REMOVED'{} \
        ORDER BY metrics.cost_micros DESC",
        date_clause
    );

    let mut rows = client.search(customer_id, &query).await?;
    gaql::enrich_cost_fields(&mut rows);
    enrich_cpa(&mut rows);

    let result = serde_json::json!({
        "campaigns": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Build a WHERE-compatible date clause fragment.
/// Returns an empty string if no dates are provided, or " AND <clause>" otherwise.
fn build_date_clause(start: Option<&str>, end: Option<&str>) -> String {
    match (start, end) {
        (Some(s), Some(e)) => format!(" AND {}", gaql::date_clause(s, e)),
        _ => String::new(),
    }
}

/// Compute cost-per-acquisition for each row that has conversions and cost_micros.
fn enrich_cpa(rows: &mut [serde_json::Value]) {
    for row in rows.iter_mut() {
        let metrics = match row.get_mut("metrics").and_then(|m| m.as_object_mut()) {
            Some(m) => m,
            None => continue,
        };

        let cost_micros = metrics
            .get("costMicros")
            .or_else(|| metrics.get("cost_micros"))
            .and_then(|v| match v {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.parse::<f64>().ok(),
                _ => None,
            });

        let conversions = metrics.get("conversions").and_then(|v| match v {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        });

        if let (Some(cost), Some(conv)) = (cost_micros, conversions) {
            if conv > 0.0 {
                let cpa = (cost / 1_000_000.0) / conv;
                metrics.insert(
                    "cpa".to_string(),
                    serde_json::Value::String(format!("{:.2}", cpa)),
                );
            }
        }
    }
}
