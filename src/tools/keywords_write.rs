use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Input for a keyword with its match type.
pub struct KeywordWithMatchType {
    pub text: String,
    pub match_type: String,
}

const VALID_MATCH_TYPES: &[&str] = &["EXACT", "PHRASE", "BROAD"];

/// Draft keywords to add to an ad group.
///
/// Validates match types and creates a ChangePlan preview.
/// Each keyword becomes an `adGroupCriterionOperation`.
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

    // Validate match types
    for kw in &keywords {
        if !VALID_MATCH_TYPES.contains(&kw.match_type.as_str()) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Invalid match type '{}'. Must be one of: {}",
                kw.match_type,
                VALID_MATCH_TYPES.join(", ")
            )));
        }
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let ad_group_resource = format!("customers/{}/adGroups/{}", cid, ad_group_id);

    let operations: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| {
            json!({
                "adGroupCriterionOperation": {
                    "create": {
                        "adGroup": ad_group_resource,
                        "keyword": {
                            "text": kw.text,
                            "matchType": kw.match_type
                        }
                    }
                }
            })
        })
        .collect();

    let keyword_summary: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| json!({"text": kw.text, "match_type": kw.match_type}))
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
                },
                KeywordWithMatchType {
                    text: "running shoes".to_string(),
                    match_type: "PHRASE".to_string(),
                },
            ],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_keywords");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
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
}
