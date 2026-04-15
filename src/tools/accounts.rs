use crate::client::GoogleAdsClient;
use crate::error::Result;

/// List all accessible Google Ads accounts.
///
/// If `login_customer_id` is set in config (MCC account), queries against it.
/// Otherwise queries the single configured `customer_id`.
pub async fn list_accounts(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            customer_client.id, \
            customer_client.descriptive_name, \
            customer_client.status, \
            customer_client.manager \
        FROM customer_client";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "accounts": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
