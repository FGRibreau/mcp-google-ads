//! Read tools for negative keyword lists (Google Ads "shared sets").
//!
//! A negative keyword list is a `SharedSet` of type `NEGATIVE_KEYWORDS`. Its
//! members are `SharedCriterion` rows, and it is attached to campaigns through
//! `CampaignSharedSet` link rows. Campaign-level negatives
//! (`campaign_criterion`) are a *separate* mechanism — see
//! [`crate::tools::keywords::get_negative_keywords`] for those. A campaign's
//! effective exclusions are the union of both.

use crate::client::GoogleAdsClient;
use crate::error::{McpGoogleAdsError, Result};

/// Reject anything that is not a bare positive integer.
///
/// Shared set / campaign IDs are interpolated into GAQL and into resource
/// names, so they must never carry quotes, spaces or operators.
pub fn validate_numeric_id(label: &str, id: &str) -> Result<()> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must be a numeric ID, got '{}'",
            label, id
        )));
    }
    Ok(())
}

/// List the account's negative keyword lists, each with the campaigns it is
/// attached to.
///
/// Two queries: one for the sets, one for the campaign links. They are joined
/// in-process because GAQL cannot return a set with zero links and its member
/// data in a single row set — a set attached to no campaign would simply
/// vanish from a `campaign_shared_set` query, which is exactly the case worth
/// surfacing (a list that exists but protects nothing).
pub async fn list_negative_keyword_lists(
    client: &GoogleAdsClient,
    customer_id: &str,
) -> Result<String> {
    let sets_query = "\
        SELECT \
            shared_set.id, \
            shared_set.name, \
            shared_set.type, \
            shared_set.member_count, \
            shared_set.reference_count, \
            shared_set.status \
        FROM shared_set \
        WHERE shared_set.type = 'NEGATIVE_KEYWORDS' \
            AND shared_set.status != 'REMOVED'";

    let links_query = "\
        SELECT \
            shared_set.id, \
            campaign.id, \
            campaign.name, \
            campaign_shared_set.status \
        FROM campaign_shared_set \
        WHERE campaign_shared_set.status != 'REMOVED'";

    let sets = client.search(customer_id, sets_query).await?;
    let links = client.search(customer_id, links_query).await?;

    let lists: Vec<serde_json::Value> = sets
        .iter()
        .map(|row| {
            let set_id = row
                .pointer("/sharedSet/id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let attached: Vec<serde_json::Value> = links
                .iter()
                .filter(|l| {
                    l.pointer("/sharedSet/id").and_then(|v| v.as_str()) == Some(set_id)
                })
                .map(|l| {
                    serde_json::json!({
                        "id": l.pointer("/campaign/id").and_then(|v| v.as_str()),
                        "name": l.pointer("/campaign/name").and_then(|v| v.as_str()),
                    })
                })
                .collect();

            serde_json::json!({
                "id": set_id,
                "name": row.pointer("/sharedSet/name").and_then(|v| v.as_str()),
                "status": row.pointer("/sharedSet/status").and_then(|v| v.as_str()),
                "member_count": row.pointer("/sharedSet/memberCount"),
                "attached_campaign_count": attached.len(),
                "attached_campaigns": attached,
            })
        })
        .collect();

    let result = serde_json::json!({
        "negative_keyword_lists": lists,
        "total_count": lists.len(),
        "note": "These are shared-list exclusions. Campaign-level negatives are separate — call get_negative_keywords for those.",
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get the keywords inside one negative keyword list.
///
/// `criterion_id` is returned because it is the handle
/// `remove_from_negative_keyword_list` needs.
pub async fn get_negative_keyword_list(
    client: &GoogleAdsClient,
    customer_id: &str,
    shared_set_id: &str,
) -> Result<String> {
    validate_numeric_id("shared_set_id", shared_set_id)?;

    let query = format!(
        "SELECT \
            shared_set.id, \
            shared_set.name, \
            shared_criterion.criterion_id, \
            shared_criterion.type, \
            shared_criterion.keyword.text, \
            shared_criterion.keyword.match_type \
        FROM shared_criterion \
        WHERE shared_set.id = {}",
        shared_set_id
    );

    let rows = client.search(customer_id, &query).await?;

    let list_name = rows
        .first()
        .and_then(|r| r.pointer("/sharedSet/name"))
        .and_then(|v| v.as_str());

    let keywords: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "criterion_id": row.pointer("/sharedCriterion/criterionId").and_then(|v| v.as_str()),
                "text": row.pointer("/sharedCriterion/keyword/text").and_then(|v| v.as_str()),
                "match_type": row.pointer("/sharedCriterion/keyword/matchType").and_then(|v| v.as_str()),
            })
        })
        .collect();

    let result = serde_json::json!({
        "shared_set_id": shared_set_id,
        "name": list_name,
        "keywords": keywords,
        "total_count": keywords.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_numeric_id_accepts_digits() {
        assert!(validate_numeric_id("shared_set_id", "1234567890").is_ok());
    }

    #[test]
    fn test_validate_numeric_id_rejects_empty() {
        assert!(validate_numeric_id("shared_set_id", "").is_err());
    }

    #[test]
    fn test_validate_numeric_id_rejects_gaql_injection() {
        let err = validate_numeric_id("shared_set_id", "1 OR 1=1")
            .expect_err("injection attempt rejected");
        assert!(err.to_string().contains("numeric ID"));
    }

    #[test]
    fn test_validate_numeric_id_rejects_resource_name() {
        assert!(validate_numeric_id("shared_set_id", "customers/1/sharedSets/2").is_err());
    }
}
