use crate::client::GoogleAdsClient;
use crate::error::Result;
use crate::gaql;

/// Execute an arbitrary GAQL query and return results in the specified format.
///
/// Supported formats: "json" (default), "table", "csv".
pub async fn run_gaql(
    client: &GoogleAdsClient,
    customer_id: &str,
    query: &str,
    format: &str,
) -> Result<String> {
    // `campaign.start_date`/`end_date` were removed from the API in v23; rewrite
    // them to `start_date_time`/`end_date_time` and alias the result back under
    // the field name the caller wrote, rather than surfacing a bare
    // UNRECOGNIZED_FIELD — see gaql::rewrite_deprecated_date_fields.
    let (effective_query, date_field_rewrites) = gaql::rewrite_deprecated_date_fields(query);

    let mut rows = client.search(customer_id, &effective_query).await?;
    gaql::enrich_cost_fields(&mut rows);
    for (deprecated, replacement) in &date_field_rewrites {
        gaql::alias_date_time_as_date(&mut rows, deprecated, replacement);
    }

    let fields = gaql::parse_select_fields(query);

    match format {
        "table" => Ok(gaql::format_table(&rows, &fields)),
        "csv" => Ok(gaql::format_csv(&rows, &fields)),
        _ => {
            let result = serde_json::json!({
                "results": rows,
                "total_count": rows.len(),
                "fields": fields,
            });
            serde_json::to_string_pretty(&result).map_err(Into::into)
        }
    }
}
