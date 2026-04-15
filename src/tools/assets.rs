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
}
