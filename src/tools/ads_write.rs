use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::{check_blocked_operation, validate_description, validate_headline};
use crate::safety::preview::{store_plan, ChangePlan};

/// Parameters for drafting a Responsive Search Ad.
pub struct DraftRsaParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub ad_group_id: &'a str,
    pub headlines: Vec<String>,
    pub descriptions: Vec<String>,
    pub final_url: &'a str,
    pub path1: Option<&'a str>,
    pub path2: Option<&'a str>,
}

/// Draft a Responsive Search Ad (RSA).
///
/// Validates headline and description counts and character limits, then creates
/// a ChangePlan preview. The ad is created in PAUSED status.
///
/// Requirements:
/// - 3 to 15 headlines, each max 30 characters
/// - 2 to 4 descriptions, each max 90 characters
/// - At least one final URL
pub fn draft_responsive_search_ad(params: &DraftRsaParams) -> Result<serde_json::Value> {
    check_blocked_operation("draft_responsive_search_ad", &params.config.safety)?;

    // Validate headline count
    if params.headlines.len() < 3 || params.headlines.len() > 15 {
        return Err(McpGoogleAdsError::Validation(format!(
            "RSA requires 3-15 headlines, got {}",
            params.headlines.len()
        )));
    }

    // Validate description count
    if params.descriptions.len() < 2 || params.descriptions.len() > 4 {
        return Err(McpGoogleAdsError::Validation(format!(
            "RSA requires 2-4 descriptions, got {}",
            params.descriptions.len()
        )));
    }

    // Validate individual headline lengths
    for headline in &params.headlines {
        validate_headline(headline)?;
    }

    // Validate individual description lengths
    for desc in &params.descriptions {
        validate_description(desc)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(params.customer_id);
    let ad_group_resource = format!("customers/{}/adGroups/{}", cid, params.ad_group_id);

    let headline_assets: Vec<serde_json::Value> = params
        .headlines
        .iter()
        .map(|h| json!({"text": h}))
        .collect();

    let description_assets: Vec<serde_json::Value> = params
        .descriptions
        .iter()
        .map(|d| json!({"text": d}))
        .collect();

    let mut ad = json!({
        "responsiveSearchAd": {
            "headlines": headline_assets,
            "descriptions": description_assets
        },
        "finalUrls": [params.final_url]
    });

    if let Some(p1) = params.path1 {
        if let Some(rsa) = ad
            .pointer_mut("/responsiveSearchAd")
            .and_then(|v| v.as_object_mut())
        {
            rsa.insert("path1".to_string(), json!(p1));
        }
    }

    if let Some(p2) = params.path2 {
        if let Some(rsa) = ad
            .pointer_mut("/responsiveSearchAd")
            .and_then(|v| v.as_object_mut())
        {
            rsa.insert("path2".to_string(), json!(p2));
        }
    }

    let operation = json!({
        "adGroupAdOperation": {
            "create": {
                "adGroup": ad_group_resource,
                "ad": ad,
                "status": "PAUSED"
            }
        }
    });

    let changes = json!({
        "ad_group_id": params.ad_group_id,
        "headlines": params.headlines,
        "descriptions": params.descriptions,
        "final_url": params.final_url,
        "path1": params.path1,
        "path2": params.path2
    });

    let plan = ChangePlan::new(
        "draft_responsive_search_ad".to_string(),
        "ad".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_headlines(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("Headline {}", i + 1)).collect()
    }

    fn make_descriptions(count: usize) -> Vec<String> {
        (0..count)
            .map(|i| format!("Description number {}", i + 1))
            .collect()
    }

    #[test]
    fn test_draft_rsa_too_few_headlines() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(2), // too few
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("3-15 headlines"));
    }

    #[test]
    fn test_draft_rsa_too_many_headlines() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(16), // too many
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_draft_rsa_too_few_descriptions() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(1), // too few
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("2-4 descriptions"));
    }

    #[test]
    fn test_draft_rsa_headline_too_long() {
        let config = Config::default();
        let mut headlines = make_headlines(3);
        headlines[0] = "A".repeat(31); // exceeds 30 char limit
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines,
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("30 character limit"));
    }

    #[test]
    fn test_draft_rsa_success() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: Some("path1"),
            path2: Some("path2"),
        });
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_responsive_search_ad");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_draft_rsa_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["draft_responsive_search_ad".to_string()];
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_draft_rsa_too_many_descriptions() {
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(5), // exceeds max of 4
            final_url: "https://example.com",
            path1: None,
            path2: None,
        });
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("2-4 descriptions"));
    }

    #[test]
    fn test_draft_rsa_empty_final_url() {
        // The function does not currently validate empty final_url,
        // so this should succeed (the API will reject it later)
        let config = Config::default();
        let result = draft_responsive_search_ad(&DraftRsaParams {
            config: &config,
            customer_id: "123-456-7890",
            ad_group_id: "111",
            headlines: make_headlines(3),
            descriptions: make_descriptions(2),
            final_url: "",
            path1: None,
            path2: None,
        });
        assert!(result.is_ok());
    }
}
