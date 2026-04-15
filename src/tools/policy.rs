use crate::client::GoogleAdsClient;
use crate::error::Result;

/// Get policy issues for ads (disapproved, limited, under review).
pub async fn get_policy_issues(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            ad_group_ad.ad.id, \
            ad_group_ad.ad.name, \
            ad_group_ad.policy_summary.approval_status, \
            ad_group_ad.policy_summary.review_status, \
            ad_group_ad.policy_summary.policy_topic_entries, \
            campaign.name, \
            ad_group.name \
        FROM ad_group_ad \
        WHERE ad_group_ad.policy_summary.approval_status != 'APPROVED' \
        LIMIT 200";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "policy_issues": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
