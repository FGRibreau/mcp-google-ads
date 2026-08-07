use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::{check_blocked_operation, validate_final_url};
use crate::safety::preview::{store_plan, ChangePlan};

/// Input for a keyword with its match type.
pub struct KeywordWithMatchType {
    pub text: String,
    pub match_type: String,
    /// Optional keyword-level final URL override. When set, clicks on this
    /// keyword route to this landing page (`ad_group_criterion.final_urls`)
    /// instead of inheriting the ad's final URL. Must be an absolute http(s)
    /// URL. `None` inherits the ad's final URL.
    pub final_url: Option<String>,
}

const VALID_MATCH_TYPES: &[&str] = &["EXACT", "PHRASE", "BROAD"];

/// Draft keywords to add to an ad group.
///
/// Validates match types and creates a ChangePlan preview.
/// Each keyword becomes an `adGroupCriterionOperation`.
///
/// Note on policy: health, medical and other sensitive-category keywords are
/// routinely rejected at apply time under exemptible policies such as
/// `HEALTH_IN_PERSONALIZED_ADS`. That is not a malformed operation — the Google
/// Ads UI simply auto-requests an exemption. Apply with
/// `confirm_and_apply(exempt_policy_violations=true)` to do the same here; see
/// [`crate::safety::policy_exemption`].
///
/// TODO: Check broad+manual CPC blocker (requires querying the campaign's
/// bidding strategy, which needs an async client call — deferred to a future iteration).
pub fn draft_keywords(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    keywords: Vec<KeywordWithMatchType>,
) -> Result<serde_json::Value> {
    check_blocked_operation("draft_keywords", &config.safety)?;

    if keywords.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one keyword is required".to_string(),
        ));
    }

    // Validate match types and any keyword-level final URL overrides.
    for kw in &keywords {
        if !VALID_MATCH_TYPES.contains(&kw.match_type.as_str()) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Invalid match type '{}'. Must be one of: {}",
                kw.match_type,
                VALID_MATCH_TYPES.join(", ")
            )));
        }
        if let Some(ref url) = kw.final_url {
            validate_final_url(url)?;
        }
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let ad_group_resource = format!("customers/{}/adGroups/{}", cid, ad_group_id);

    let operations: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| {
            let mut criterion = json!({
                "adGroup": ad_group_resource,
                "keyword": {
                    "text": kw.text,
                    "matchType": kw.match_type
                }
            });
            if let Some(ref url) = kw.final_url {
                if let Some(obj) = criterion.as_object_mut() {
                    obj.insert("finalUrls".to_string(), json!([url]));
                }
            }
            json!({
                "adGroupCriterionOperation": {
                    "create": criterion
                }
            })
        })
        .collect();

    let keyword_summary: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| json!({"text": kw.text, "match_type": kw.match_type, "final_url": kw.final_url}))
        .collect();

    let changes = json!({
        "ad_group_id": ad_group_id,
        "keywords": keyword_summary
    });

    let plan = ChangePlan::new(
        "draft_keywords".to_string(),
        "keyword".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Add negative keywords to a campaign.
///
/// Creates `campaignCriterionOperation` entries with `negative: true` for each keyword.
pub fn add_negative_keywords(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    keywords: Vec<String>,
    match_type: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("add_negative_keywords", &config.safety)?;

    if keywords.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one keyword is required".to_string(),
        ));
    }

    if !VALID_MATCH_TYPES.contains(&match_type) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid match type '{}'. Must be one of: {}",
            match_type,
            VALID_MATCH_TYPES.join(", ")
        )));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let operations: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| {
            json!({
                "campaignCriterionOperation": {
                    "create": {
                        "campaign": campaign_resource,
                        "negative": true,
                        "keyword": {
                            "text": kw,
                            "matchType": match_type
                        }
                    }
                }
            })
        })
        .collect();

    let changes = json!({
        "campaign_id": campaign_id,
        "keywords": keywords,
        "match_type": match_type,
        "negative": true
    });

    let plan = ChangePlan::new(
        "add_negative_keywords".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Remove keywords from an ad group by criterion IDs.
///
/// This is a destructive operation and requires double confirmation.
pub fn remove_keywords(
    config: &Config,
    customer_id: &str,
    ad_group_id: &str,
    criterion_ids: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_keywords", &config.safety)?;

    if criterion_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one criterion ID is required".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operations: Vec<serde_json::Value> = criterion_ids
        .iter()
        .map(|criterion_id| {
            json!({
                "adGroupCriterionOperation": {
                    "remove": format!("customers/{}/adGroupCriteria/{}~{}", cid, ad_group_id, criterion_id)
                }
            })
        })
        .collect();

    let changes = json!({
        "ad_group_id": ad_group_id,
        "criterion_ids": criterion_ids,
        "warning": "This action is destructive and cannot be undone"
    });

    let plan = ChangePlan::new(
        "remove_keywords".to_string(),
        "ad_group_criterion".to_string(),
        ad_group_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Remove negative keywords from a campaign by criterion IDs.
///
/// This is a destructive operation and requires double confirmation.
pub fn remove_negative_keywords(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    criterion_ids: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_negative_keywords", &config.safety)?;

    if criterion_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one criterion ID is required".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operations: Vec<serde_json::Value> = criterion_ids
        .iter()
        .map(|criterion_id| {
            json!({
                "campaignCriterionOperation": {
                    "remove": format!("customers/{}/campaignCriteria/{}~{}", cid, campaign_id, criterion_id)
                }
            })
        })
        .collect();

    let changes = json!({
        "campaign_id": campaign_id,
        "criterion_ids": criterion_ids,
        "warning": "This action is destructive and cannot be undone"
    });

    let plan = ChangePlan::new(
        "remove_negative_keywords".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_draft_keywords_empty() {
        let config = Config::default();
        let result = draft_keywords(&config, "123-456-7890", "111", vec![]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("At least one keyword"));
    }

    #[test]
    fn test_draft_keywords_invalid_match_type() {
        let config = Config::default();
        let result = draft_keywords(
            &config,
            "123-456-7890",
            "111",
            vec![KeywordWithMatchType {
                text: "test".to_string(),
                match_type: "INVALID".to_string(),
                final_url: None,
            }],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("Invalid match type"));
    }

    #[test]
    fn test_draft_keywords_success() {
        let config = Config::default();
        let result = draft_keywords(
            &config,
            "123-456-7890",
            "111",
            vec![
                KeywordWithMatchType {
                    text: "buy shoes".to_string(),
                    match_type: "EXACT".to_string(),
                    final_url: None,
                },
                KeywordWithMatchType {
                    text: "running shoes".to_string(),
                    match_type: "PHRASE".to_string(),
                    final_url: None,
                },
            ],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_keywords");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_draft_keywords_final_url_sets_final_urls() {
        let config = Config::default();
        let preview = draft_keywords(
            &config,
            "1234567890",
            "111",
            vec![
                KeywordWithMatchType {
                    text: "routed".to_string(),
                    match_type: "EXACT".to_string(),
                    final_url: Some("https://example.com/routed".to_string()),
                },
                KeywordWithMatchType {
                    text: "inherit".to_string(),
                    match_type: "PHRASE".to_string(),
                    final_url: None,
                },
            ],
        )
        .expect("ok");

        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = crate::safety::preview::get_plan(plan_id).expect("plan stored");

        let criteria: Vec<&serde_json::Value> = plan
            .mutate_operations
            .iter()
            .filter_map(|op| op.pointer("/adGroupCriterionOperation/create"))
            .collect();
        assert_eq!(criteria.len(), 2);

        let routed = criteria
            .iter()
            .find(|c| c["keyword"]["text"] == "routed")
            .expect("routed keyword present");
        assert_eq!(routed["finalUrls"], json!(["https://example.com/routed"]));

        // A keyword without a final_url must not carry a finalUrls field — it
        // inherits the ad's final URL.
        let inherit = criteria
            .iter()
            .find(|c| c["keyword"]["text"] == "inherit")
            .expect("inherit keyword present");
        assert!(inherit.get("finalUrls").is_none());
    }

    #[test]
    fn test_draft_keywords_invalid_final_url_rejected() {
        let config = Config::default();
        let err = draft_keywords(
            &config,
            "1234567890",
            "111",
            vec![KeywordWithMatchType {
                text: "bad".to_string(),
                match_type: "EXACT".to_string(),
                final_url: Some("not-a-url".to_string()),
            }],
        )
        .expect_err("invalid final_url rejected");
        assert!(err.to_string().contains("http(s) URL"));
    }

    #[test]
    fn test_draft_keywords_empty_final_url_rejected() {
        let config = Config::default();
        let err = draft_keywords(
            &config,
            "1234567890",
            "111",
            vec![KeywordWithMatchType {
                text: "bad".to_string(),
                match_type: "EXACT".to_string(),
                final_url: Some("".to_string()),
            }],
        )
        .expect_err("empty final_url rejected");
        assert!(err.to_string().contains("final_url must not be empty"));
    }

    #[test]
    fn test_add_negative_keywords_empty() {
        let config = Config::default();
        let result = add_negative_keywords(&config, "123-456-7890", "222", vec![], "EXACT");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_negative_keywords_invalid_match_type() {
        let config = Config::default();
        let result = add_negative_keywords(
            &config,
            "123-456-7890",
            "222",
            vec!["bad keyword".to_string()],
            "FUZZY",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_add_negative_keywords_success() {
        let config = Config::default();
        let result = add_negative_keywords(
            &config,
            "123-456-7890",
            "222",
            vec!["free".to_string(), "cheap".to_string()],
            "BROAD",
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "add_negative_keywords");
        assert_eq!(preview["changes"]["negative"], true);
    }

    #[test]
    fn test_draft_keywords_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["draft_keywords".to_string()];
        let result = draft_keywords(
            &config,
            "123-456-7890",
            "111",
            vec![KeywordWithMatchType {
                text: "test".to_string(),
                match_type: "EXACT".to_string(),
                final_url: None,
            }],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_add_negative_keywords_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["add_negative_keywords".to_string()];
        let result = add_negative_keywords(
            &config,
            "123-456-7890",
            "222",
            vec!["free".to_string()],
            "EXACT",
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_remove_keywords_empty() {
        let config = Config::default();
        let result = remove_keywords(&config, "123-456-7890", "111", vec![]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("At least one criterion ID"));
    }

    #[test]
    fn test_remove_keywords_success() {
        let config = Config::default();
        let result = remove_keywords(
            &config,
            "123-456-7890",
            "111",
            vec!["555".to_string(), "666".to_string()],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "remove_keywords");
        assert_eq!(preview["requires_double_confirm"], true);
        assert_eq!(preview["changes"]["ad_group_id"], "111");
    }

    #[test]
    fn test_remove_keywords_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["remove_keywords".to_string()];
        let result = remove_keywords(&config, "123-456-7890", "111", vec!["555".to_string()]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_remove_negative_keywords_empty() {
        let config = Config::default();
        let result = remove_negative_keywords(&config, "123-456-7890", "222", vec![]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("At least one criterion ID"));
    }

    #[test]
    fn test_remove_negative_keywords_success() {
        let config = Config::default();
        let result = remove_negative_keywords(
            &config,
            "123-456-7890",
            "222",
            vec!["777".to_string(), "888".to_string()],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "remove_negative_keywords");
        assert_eq!(preview["requires_double_confirm"], true);
        assert_eq!(preview["changes"]["campaign_id"], "222");
    }

    #[test]
    fn test_remove_negative_keywords_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["remove_negative_keywords".to_string()];
        let result =
            remove_negative_keywords(&config, "123-456-7890", "222", vec!["777".to_string()]);
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
