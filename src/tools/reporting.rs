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
    let mut rows = client.search(customer_id, query).await?;
    gaql::enrich_cost_fields(&mut rows);

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
