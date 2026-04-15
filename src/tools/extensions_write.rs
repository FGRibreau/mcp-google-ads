use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::{
    check_blocked_operation, validate_sitelink_description, validate_sitelink_text,
};
use crate::safety::preview::{store_plan, ChangePlan};

/// Input for a sitelink extension.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SitelinkInput {
    pub link_text: String,
    pub final_url: String,
    pub description1: String,
    pub description2: String,
}

/// Draft sitelink extensions for a campaign.
///
/// Creates sitelink assets and links them to the campaign.
/// Warns if fewer than 2 sitelinks are provided (Google recommends at least 2).
pub fn draft_sitelinks(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    sitelinks: Vec<SitelinkInput>,
) -> Result<serde_json::Value> {
    check_blocked_operation("draft_sitelinks", &config.safety)?;

    if sitelinks.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one sitelink is required".to_string(),
        ));
    }

    // Validate all sitelinks
    for sl in &sitelinks {
        validate_sitelink_text(&sl.link_text)?;
        validate_sitelink_description(&sl.description1)?;
        validate_sitelink_description(&sl.description2)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();

    // Create sitelink assets with temporary resource IDs starting at -100
    for (i, sl) in sitelinks.iter().enumerate() {
        let temp_id = -(100 + i as i64);
        let asset_resource = format!("customers/{}/assets/{}", cid, temp_id);

        // Create the asset
        operations.push(json!({
            "assetOperation": {
                "create": {
                    "resourceName": asset_resource,
                    "sitelinkAsset": {
                        "linkText": sl.link_text,
                        "description1": sl.description1,
                        "description2": sl.description2
                    },
                    "finalUrls": [sl.final_url]
                }
            }
        }));

        // Link asset to campaign
        operations.push(json!({
            "campaignAssetOperation": {
                "create": {
                    "campaign": campaign_resource,
                    "asset": asset_resource,
                    "fieldType": "SITELINK"
                }
            }
        }));
    }

    let mut warnings: Vec<String> = Vec::new();
    if sitelinks.len() < 2 {
        warnings.push("Google recommends at least 2 sitelinks for best performance".to_string());
    }

    let sitelink_summary: Vec<serde_json::Value> = sitelinks
        .iter()
        .map(|sl| {
            json!({
                "link_text": sl.link_text,
                "final_url": sl.final_url,
                "description1": sl.description1,
                "description2": sl.description2
            })
        })
        .collect();

    let mut changes = json!({
        "campaign_id": campaign_id,
        "sitelinks": sitelink_summary
    });

    if !warnings.is_empty() {
        changes
            .as_object_mut()
            .map(|o| o.insert("warnings".to_string(), json!(warnings)));
    }

    let plan = ChangePlan::new(
        "draft_sitelinks".to_string(),
        "sitelink".to_string(),
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

/// Create callout extensions for a campaign.
///
/// Each callout text must be max 25 characters.
pub fn create_callouts(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    callouts: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_callouts", &config.safety)?;

    if callouts.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one callout is required".to_string(),
        ));
    }

    // Validate callout lengths (same limit as sitelink text: 25 chars)
    for callout in &callouts {
        validate_sitelink_text(callout)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();

    for (i, callout) in callouts.iter().enumerate() {
        let temp_id = -(200 + i as i64);
        let asset_resource = format!("customers/{}/assets/{}", cid, temp_id);

        // Create callout asset
        operations.push(json!({
            "assetOperation": {
                "create": {
                    "resourceName": asset_resource,
                    "calloutAsset": {
                        "calloutText": callout
                    }
                }
            }
        }));

        // Link to campaign
        operations.push(json!({
            "campaignAssetOperation": {
                "create": {
                    "campaign": campaign_resource,
                    "asset": asset_resource,
                    "fieldType": "CALLOUT"
                }
            }
        }));
    }

    let changes = json!({
        "campaign_id": campaign_id,
        "callouts": callouts
    });

    let plan = ChangePlan::new(
        "create_callouts".to_string(),
        "callout".to_string(),
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

/// Known structured snippet header types.
const VALID_SNIPPET_HEADERS: &[&str] = &[
    "Amenities",
    "Brands",
    "Courses",
    "Degree programs",
    "Destinations",
    "Featured hotels",
    "Insurance coverage",
    "Models",
    "Neighborhoods",
    "Service catalog",
    "Shows",
    "Styles",
    "Types",
];

/// Create structured snippet extensions for a campaign.
///
/// The header must be one of the predefined types recognized by Google Ads.
pub fn create_structured_snippets(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    header: &str,
    values: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_structured_snippets", &config.safety)?;

    if !VALID_SNIPPET_HEADERS.contains(&header) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid structured snippet header '{}'. Must be one of: {}",
            header,
            VALID_SNIPPET_HEADERS.join(", ")
        )));
    }

    if values.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one value is required for structured snippets".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);
    let mut operations: Vec<serde_json::Value> = Vec::new();

    let temp_id: i64 = -300;
    let asset_resource = format!("customers/{}/assets/{}", cid, temp_id);

    // Create structured snippet asset
    operations.push(json!({
        "assetOperation": {
            "create": {
                "resourceName": asset_resource,
                "structuredSnippetAsset": {
                    "header": header,
                    "values": values
                }
            }
        }
    }));

    // Link to campaign
    operations.push(json!({
        "campaignAssetOperation": {
            "create": {
                "campaign": campaign_resource,
                "asset": asset_resource,
                "fieldType": "STRUCTURED_SNIPPET"
            }
        }
    }));

    let changes = json!({
        "campaign_id": campaign_id,
        "header": header,
        "values": values
    });

    let plan = ChangePlan::new(
        "create_structured_snippets".to_string(),
        "structured_snippet".to_string(),
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

/// Remove a campaign asset (extension) by campaign ID, asset ID, and field type.
///
/// This is a destructive operation and requires double confirmation.
pub fn remove_extension(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    asset_id: &str,
    field_type: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_extension", &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let resource_name = format!(
        "customers/{}/campaignAssets/{}~{}~{}",
        cid, campaign_id, asset_id, field_type
    );

    let operations = vec![json!({
        "campaignAssetOperation": {
            "remove": resource_name
        }
    })];

    let changes = json!({
        "campaign_id": campaign_id,
        "asset_id": asset_id,
        "field_type": field_type,
        "warning": "This action is destructive and cannot be undone"
    });

    let plan = ChangePlan::new(
        "remove_extension".to_string(),
        "campaign_asset".to_string(),
        format!("{}~{}", campaign_id, asset_id),
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
    fn test_draft_sitelinks_empty() {
        let config = Config::default();
        let result = draft_sitelinks(&config, "123-456-7890", "555", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_draft_sitelinks_text_too_long() {
        let config = Config::default();
        let result = draft_sitelinks(
            &config,
            "123-456-7890",
            "555",
            vec![SitelinkInput {
                link_text: "A".repeat(26),
                final_url: "https://example.com".to_string(),
                description1: "Desc 1".to_string(),
                description2: "Desc 2".to_string(),
            }],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("25 character limit"));
    }

    #[test]
    fn test_draft_sitelinks_description_too_long() {
        let config = Config::default();
        let result = draft_sitelinks(
            &config,
            "123-456-7890",
            "555",
            vec![SitelinkInput {
                link_text: "Link".to_string(),
                final_url: "https://example.com".to_string(),
                description1: "A".repeat(36),
                description2: "Desc 2".to_string(),
            }],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("35 character limit"));
    }

    #[test]
    fn test_draft_sitelinks_warns_fewer_than_two() {
        let config = Config::default();
        let result = draft_sitelinks(
            &config,
            "123-456-7890",
            "555",
            vec![SitelinkInput {
                link_text: "Link 1".to_string(),
                final_url: "https://example.com".to_string(),
                description1: "Desc 1".to_string(),
                description2: "Desc 2".to_string(),
            }],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        let warnings = preview["changes"]["warnings"].as_array();
        assert!(warnings.is_some());
        assert!(!warnings.map(|w| w.is_empty()).unwrap_or(true));
    }

    #[test]
    fn test_draft_sitelinks_success() {
        let config = Config::default();
        let result = draft_sitelinks(
            &config,
            "123-456-7890",
            "555",
            vec![
                SitelinkInput {
                    link_text: "About Us".to_string(),
                    final_url: "https://example.com/about".to_string(),
                    description1: "Learn more".to_string(),
                    description2: "About our company".to_string(),
                },
                SitelinkInput {
                    link_text: "Contact".to_string(),
                    final_url: "https://example.com/contact".to_string(),
                    description1: "Get in touch".to_string(),
                    description2: "Contact us today".to_string(),
                },
            ],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "draft_sitelinks");
    }

    #[test]
    fn test_create_callouts_empty() {
        let config = Config::default();
        let result = create_callouts(&config, "123-456-7890", "555", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_callouts_too_long() {
        let config = Config::default();
        let result = create_callouts(&config, "123-456-7890", "555", vec!["A".repeat(26)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_callouts_success() {
        let config = Config::default();
        let result = create_callouts(
            &config,
            "123-456-7890",
            "555",
            vec!["Free Shipping".to_string(), "24/7 Support".to_string()],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_callouts");
    }

    #[test]
    fn test_create_structured_snippets_invalid_header() {
        let config = Config::default();
        let result = create_structured_snippets(
            &config,
            "123-456-7890",
            "555",
            "InvalidHeader",
            vec!["value1".to_string()],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("Invalid structured snippet header"));
    }

    #[test]
    fn test_create_structured_snippets_empty_values() {
        let config = Config::default();
        let result = create_structured_snippets(&config, "123-456-7890", "555", "Brands", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_structured_snippets_success() {
        let config = Config::default();
        let result = create_structured_snippets(
            &config,
            "123-456-7890",
            "555",
            "Brands",
            vec!["Nike".to_string(), "Adidas".to_string(), "Puma".to_string()],
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "create_structured_snippets");
    }

    #[test]
    fn test_draft_sitelinks_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["draft_sitelinks".to_string()];
        let result = draft_sitelinks(
            &config,
            "123-456-7890",
            "555",
            vec![SitelinkInput {
                link_text: "Link".to_string(),
                final_url: "https://example.com".to_string(),
                description1: "Desc 1".to_string(),
                description2: "Desc 2".to_string(),
            }],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_create_callouts_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_callouts".to_string()];
        let result = create_callouts(
            &config,
            "123-456-7890",
            "555",
            vec!["Free Shipping".to_string()],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_create_structured_snippets_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_structured_snippets".to_string()];
        let result = create_structured_snippets(
            &config,
            "123-456-7890",
            "555",
            "Brands",
            vec!["Nike".to_string()],
        );
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_remove_extension_success() {
        let config = Config::default();
        let result = remove_extension(&config, "123-456-7890", "555", "999", "SITELINK");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "remove_extension");
        assert_eq!(preview["requires_double_confirm"], true);
        assert_eq!(preview["changes"]["campaign_id"], "555");
        assert_eq!(preview["changes"]["asset_id"], "999");
        assert_eq!(preview["changes"]["field_type"], "SITELINK");
    }

    #[test]
    fn test_remove_extension_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["remove_extension".to_string()];
        let result = remove_extension(&config, "123-456-7890", "555", "999", "SITELINK");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
