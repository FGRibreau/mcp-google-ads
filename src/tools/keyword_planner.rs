use crate::client::GoogleAdsClient;
use crate::error::Result;

/// Discover keyword ideas from seed keywords using the Keyword Planner API.
pub async fn discover_keywords(
    client: &GoogleAdsClient,
    customer_id: &str,
    seed_keywords: Vec<String>,
) -> Result<String> {
    let results = client
        .generate_keyword_ideas(customer_id, seed_keywords, Some(50))
        .await?;

    let result = serde_json::json!({
        "keyword_ideas": results,
        "total_count": results.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get historical keyword performance metrics for forecasting.
pub async fn get_keyword_forecasts(
    client: &GoogleAdsClient,
    customer_id: &str,
    keyword_texts: Vec<String>,
) -> Result<String> {
    if keyword_texts.is_empty() {
        let result = serde_json::json!({
            "keyword_forecasts": [],
            "total_count": 0,
            "message": "No keywords provided."
        });
        return serde_json::to_string_pretty(&result).map_err(Into::into);
    }

    let escaped: Vec<String> = keyword_texts
        .iter()
        .map(|kw| format!("'{}'", kw.replace('\'', "\\'")))
        .collect();
    let in_clause = escaped.join(", ");

    let query = format!(
        "SELECT \
            ad_group_criterion.keyword.text, \
            metrics.average_cpc, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.cost_micros, \
            metrics.average_cpm \
        FROM keyword_view \
        WHERE ad_group_criterion.keyword.text IN ({}) \
        AND segments.date DURING LAST_30_DAYS",
        in_clause
    );

    let rows = client.search(customer_id, &query).await?;

    let result = if rows.is_empty() {
        serde_json::json!({
            "keyword_forecasts": [],
            "total_count": 0,
            "message": "No matching keywords found in the account. These keywords may not exist in any active ad group."
        })
    } else {
        serde_json::json!({
            "keyword_forecasts": rows,
            "total_count": rows.len(),
        })
    };

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
