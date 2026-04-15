use crate::client::GoogleAdsClient;
use crate::error::Result;

/// List campaign-level extensions (sitelinks, callouts, structured snippets).
pub async fn list_extensions(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            campaign_asset.campaign, \
            campaign_asset.asset, \
            campaign_asset.field_type, \
            asset.name, \
            asset.type, \
            asset.sitelink_asset.link_text, \
            asset.sitelink_asset.description1, \
            asset.sitelink_asset.description2, \
            asset.callout_asset.callout_text, \
            asset.structured_snippet_asset.header \
        FROM campaign_asset \
        WHERE campaign_asset.status != 'REMOVED' \
        LIMIT 500";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "extensions": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
