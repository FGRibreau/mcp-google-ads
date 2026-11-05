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

/// List every Google Ads customer account directly accessible by the
/// configured credentials, via `customers:listAccessibleCustomers`.
///
/// Unlike [`list_accounts`] (which descends the `customer_client` hierarchy
/// under a single configured MCC), this is not scoped to any manager account
/// — it returns every account the credentials can reach directly, including
/// ones outside the configured MCC's hierarchy.
pub async fn list_accessible_customers(client: &GoogleAdsClient) -> Result<String> {
    let resource_names = client.list_accessible_customers().await?;

    let customer_ids: Vec<String> = resource_names
        .iter()
        .filter_map(|rn| rn.strip_prefix("customers/").map(|s| s.to_string()))
        .collect();

    let result = serde_json::json!({
        "resource_names": resource_names,
        "customer_ids": customer_ids,
        "total_count": customer_ids.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get detailed information about a specific Google Ads account.
///
/// Returns account metadata including currency, timezone, tagging settings,
/// manager status, and account status.
pub async fn get_account_info(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            customer.id, \
            customer.descriptive_name, \
            customer.currency_code, \
            customer.time_zone, \
            customer.auto_tagging_enabled, \
            customer.manager, \
            customer.status \
        FROM customer \
        LIMIT 1";

    let rows = client.search(customer_id, query).await?;

    let account = rows.first().cloned();

    let result = serde_json::json!({
        "account": account,
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
