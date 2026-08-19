use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Upload an image asset.
///
/// `image_data_base64` is the base64-encoded image data.
/// The image is created as an asset that can be linked to campaigns or asset groups.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn upload_image_asset(
    config: &Config,
    customer_id: &str,
    asset_name: &str,
    image_data_base64: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("upload_image_asset", &config.safety)?;

    if asset_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Asset name cannot be empty".to_string(),
        ));
    }

    if image_data_base64.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Image data (base64) cannot be empty".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operation = json!({
        "assetOperation": {
            "create": {
                "name": asset_name,
                "type": "IMAGE",
                "imageAsset": {
                    "data": image_data_base64
                }
            }
        }
    });

    let changes = json!({
        "asset_name": asset_name,
        "asset_type": "IMAGE",
        "data_size_bytes": image_data_base64.len()
    });

    let plan = ChangePlan::new(
        "upload_image_asset".to_string(),
        "asset".to_string(),
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

/// Upload a text asset.
///
/// Creates a reusable text asset that can be linked to campaigns or asset groups.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn upload_text_asset(
    config: &Config,
    customer_id: &str,
    asset_name: &str,
    text_content: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("upload_text_asset", &config.safety)?;

    if asset_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Asset name cannot be empty".to_string(),
        ));
    }

    if text_content.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Text content cannot be empty".to_string(),
        ));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operation = json!({
        "assetOperation": {
            "create": {
                "name": asset_name,
                "textAsset": {
                    "text": text_content
                }
            }
        }
    });

    let changes = json!({
        "asset_name": asset_name,
        "asset_type": "TEXT",
        "text_content": text_content
    });

    let plan = ChangePlan::new(
        "upload_text_asset".to_string(),
        "asset".to_string(),
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

/// Asset group field types accepted by `link_asset_to_asset_group`.
///
/// Performance Max rejects any other field type on an `assetGroupAsset`, and the
/// API error for a bad one is opaque, so we reject client-side instead.
pub const VALID_ASSET_GROUP_FIELD_TYPES: &[&str] = &[
    "HEADLINE",
    "DESCRIPTION",
    "LONG_HEADLINE",
    "BUSINESS_NAME",
    "MARKETING_IMAGE",
    "SQUARE_MARKETING_IMAGE",
    "PORTRAIT_MARKETING_IMAGE",
    "LOGO",
    "LANDSCAPE_LOGO",
    "YOUTUBE_VIDEO",
    "CALL_TO_ACTION_SELECTION",
];

/// Link an existing asset to a Performance Max asset group.
///
/// `upload_image_asset` only creates the asset — it lands in the account's asset
/// library unattached. A PMax asset group with no MARKETING_IMAGE,
/// SQUARE_MARKETING_IMAGE and LOGO is "Not eligible" and never serves, so this
/// is the step that actually makes an asset group deliverable.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn link_asset_to_asset_group(
    config: &Config,
    customer_id: &str,
    asset_group_id: &str,
    asset_id: &str,
    field_type: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("link_asset_to_asset_group", &config.safety)?;

    if asset_group_id.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Asset group ID cannot be empty".to_string(),
        ));
    }

    if asset_id.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Asset ID cannot be empty".to_string(),
        ));
    }

    let field_type = field_type.to_uppercase();
    if !VALID_ASSET_GROUP_FIELD_TYPES.contains(&field_type.as_str()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid asset group field type '{}'. Must be one of: {}",
            field_type,
            VALID_ASSET_GROUP_FIELD_TYPES.join(", ")
        )));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operation = json!({
        "assetGroupAssetOperation": {
            "create": {
                "assetGroup": format!("customers/{}/assetGroups/{}", cid, asset_group_id),
                "asset": format!("customers/{}/assets/{}", cid, asset_id),
                "fieldType": field_type
            }
        }
    });

    let changes = json!({
        "asset_group_id": asset_group_id,
        "asset_id": asset_id,
        "field_type": field_type
    });

    let plan = ChangePlan::new(
        "link_asset_to_asset_group".to_string(),
        "asset_group_asset".to_string(),
        asset_group_id.to_string(),
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

    #[test]
    fn test_upload_image_asset_success() {
        let config = Config::default();
        let result = upload_image_asset(&config, "123-456-7890", "Logo", "iVBORw0KGgoAAAANS");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "upload_image_asset");
    }

    #[test]
    fn test_upload_image_asset_empty_name() {
        let config = Config::default();
        let result = upload_image_asset(&config, "123-456-7890", "", "data");
        assert!(result.is_err());
    }

    #[test]
    fn test_upload_image_asset_empty_data() {
        let config = Config::default();
        let result = upload_image_asset(&config, "123-456-7890", "Logo", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_upload_image_asset_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["upload_image_asset".to_string()];
        let result = upload_image_asset(&config, "123-456-7890", "Logo", "data");
        assert!(result.is_err());
    }

    #[test]
    fn test_upload_text_asset_success() {
        let config = Config::default();
        let result = upload_text_asset(&config, "123-456-7890", "Headline Asset", "Buy Now");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "upload_text_asset");
    }

    #[test]
    fn test_upload_text_asset_empty_name() {
        let config = Config::default();
        let result = upload_text_asset(&config, "123-456-7890", "", "text");
        assert!(result.is_err());
    }

    #[test]
    fn test_upload_text_asset_empty_content() {
        let config = Config::default();
        let result = upload_text_asset(&config, "123-456-7890", "Name", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_upload_text_asset_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["upload_text_asset".to_string()];
        let result = upload_text_asset(&config, "123-456-7890", "Asset", "Some text");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_link_asset_to_asset_group_success() {
        let config = Config::default();
        let result = link_asset_to_asset_group(
            &config,
            "123-456-7890",
            "6738426770",
            "404988595285",
            "MARKETING_IMAGE",
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "link_asset_to_asset_group");
        assert_eq!(preview["changes"]["field_type"], "MARKETING_IMAGE");
    }

    #[test]
    fn test_link_asset_to_asset_group_lowercase_field_type_is_normalized() {
        let config = Config::default();
        let result = link_asset_to_asset_group(
            &config,
            "123-456-7890",
            "123",
            "456",
            "square_marketing_image",
        );
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["changes"]["field_type"], "SQUARE_MARKETING_IMAGE");
    }

    #[test]
    fn test_link_asset_to_asset_group_invalid_field_type() {
        let config = Config::default();
        let result = link_asset_to_asset_group(&config, "123-456-7890", "123", "456", "SITELINK");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("Invalid asset group field type"));
    }

    #[test]
    fn test_link_asset_to_asset_group_empty_ids() {
        let config = Config::default();
        assert!(link_asset_to_asset_group(&config, "123-456-7890", "", "456", "LOGO").is_err());
        assert!(link_asset_to_asset_group(&config, "123-456-7890", "123", "", "LOGO").is_err());
    }

    #[test]
    fn test_link_asset_to_asset_group_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["link_asset_to_asset_group".to_string()];
        let result = link_asset_to_asset_group(&config, "123-456-7890", "123", "456", "LOGO");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }
}
