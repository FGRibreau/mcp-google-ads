use crate::client::GoogleAdsClient;
use crate::error::Result;

/// List all conversion actions configured in the account.
pub async fn get_conversion_actions(
    client: &GoogleAdsClient,
    customer_id: &str,
) -> Result<String> {
    let query = "\
        SELECT \
            conversion_action.id, \
            conversion_action.name, \
            conversion_action.type, \
            conversion_action.status, \
            conversion_action.category, \
            conversion_action.value_settings.default_value, \
            conversion_action.counting_type \
        FROM conversion_action \
        WHERE conversion_action.status != 'REMOVED' \
        ORDER BY conversion_action.name \
        LIMIT 200";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "conversion_actions": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
