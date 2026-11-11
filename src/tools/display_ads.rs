//! Uploaded display ads: an HTML5 bundle or a single image, from a local file.
//!
//! One atomic `googleAds:mutate` creates the asset and the ad together, linked
//! by a temporary resource name (`customers/{cid}/assets/-1`):
//!
//! - `HTML5`: a `MEDIA_BUNDLE` asset holding the ZIP, and an ad group ad with
//!   `display_upload_ad { display_upload_product_type: HTML5_UPLOAD_AD,
//!   media_bundle.asset }` — the shape of Google's `add_display_upload_ad`
//!   example. HTML5 upload ads are only available to accounts Google has
//!   allowlisted (open > 90 days, > USD 9,000 lifetime spend, good policy
//!   history); other accounts get a rejection.
//! - `IMAGE`: an `IMAGE` asset and an ad group ad with
//!   `image_ad { image_asset.asset }`.
//!
//! The file is checked locally before a plan exists: an absolute path to a
//! regular file, an extension the ad type allows, magic bytes that agree with
//! the extension, and Google's size limit (150 KB image, 600 KB ZIP). The
//! preview shows the file's size and SHA-256, never its bytes; the base64 only
//! lives in the stored plan that `confirm_and_apply` sends.

use std::path::PathBuf;

use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::{AdStatus, DisplayAdType, NextActionHint};
use crate::safety::guards::{check_blocked_operation, validate_final_url};
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::validate_numeric_id;
use crate::tools::local_file::{
    base64_encode, image_dimensions, sha256_hex, sniff_format, FileFormat,
};

/// Image ad sizes (width, height) from Google's "Uploaded display ads
/// specifications". Regional sizes exist too, so an image outside this list
/// gets a warning, not a refusal.
pub const STANDARD_DISPLAY_AD_SIZES: &[(u32, u32)] = &[
    (200, 200),
    (240, 400),
    (250, 250),
    (250, 360),
    (300, 250),
    (336, 280),
    (580, 400),
    (120, 600),
    (160, 600),
    (300, 600),
    (300, 1050),
    (468, 60),
    (728, 90),
    (930, 180),
    (970, 90),
    (970, 250),
    (980, 120),
    (300, 50),
    (320, 50),
    (320, 100),
];

/// Parameters for [`upload_display_ad`].
pub struct UploadDisplayAdParams<'a> {
    pub config: &'a Config,
    pub customer_id: &'a str,
    pub ad_group_id: &'a str,
    /// Absolute path to the file (a leading `~/` is expanded).
    pub file_path: &'a str,
    pub ad_type: DisplayAdType,
    /// Name for the ad and its asset (shown in the Google Ads UI).
    pub ad_name: &'a str,
    pub final_url: &'a str,
    /// `None` -> `PAUSED`.
    pub status: Option<AdStatus>,
}

/// A local file that passed every check, ready to upload.
struct CheckedFile {
    path: PathBuf,
    bytes: Vec<u8>,
    format: FileFormat,
}

/// Expand a leading `~/` and insist on an absolute path: a relative one would
/// resolve against the server's working directory, which the caller cannot see.
fn resolve_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "file_path must not be empty".to_string(),
        ));
    }
    let path = match trimmed.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .ok_or_else(|| {
                McpGoogleAdsError::Validation("cannot expand ~: no home directory".to_string())
            })?
            .join(rest),
        None => PathBuf::from(trimmed),
    };
    if !path.is_absolute() {
        return Err(McpGoogleAdsError::Validation(format!(
            "file_path '{}' must be absolute (or start with ~/)",
            raw
        )));
    }
    Ok(path)
}

/// Run every local check on the file and read it.
fn check_file(raw_path: &str, ad_type: DisplayAdType) -> Result<CheckedFile> {
    let path = resolve_path(raw_path)?;

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !ad_type.allowed_extensions().contains(&extension.as_str()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} ads take a .{} file, got '{}'",
            ad_type.as_api_str(),
            ad_type.allowed_extensions().join(" / ."),
            path.display()
        )));
    }

    let metadata = std::fs::metadata(&path).map_err(|e| {
        McpGoogleAdsError::Validation(format!("cannot read '{}': {}", path.display(), e))
    })?;
    if !metadata.is_file() {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' is not a regular file",
            path.display()
        )));
    }
    let size = metadata.len();
    if size == 0 {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' is empty",
            path.display()
        )));
    }
    let max = ad_type.max_file_bytes();
    if size > max {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' is {} bytes; {} ads may be at most {} bytes ({} KB)",
            path.display(),
            size,
            ad_type.as_api_str(),
            max,
            max / 1024
        )));
    }

    let bytes = std::fs::read(&path).map_err(|e| {
        McpGoogleAdsError::Validation(format!("cannot read '{}': {}", path.display(), e))
    })?;
    if bytes.len() as u64 > max {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' grew to {} bytes while being read; the limit is {}",
            path.display(),
            bytes.len(),
            max
        )));
    }

    let format = sniff_format(&bytes);
    let expected = match extension.as_str() {
        "zip" => FileFormat::Zip,
        "png" => FileFormat::Png,
        "gif" => FileFormat::Gif,
        _ => FileFormat::Jpeg,
    };
    if format != expected {
        return Err(McpGoogleAdsError::Validation(format!(
            "'{}' has a .{} extension but its content is {} — export it again in the \
             right format",
            path.display(),
            extension,
            format.name()
        )));
    }

    Ok(CheckedFile {
        path,
        bytes,
        format,
    })
}

/// Draft an uploaded display ad (HTML5 or image) from a local file.
///
/// Checks the file locally, then plans one atomic mutate with two operations:
/// the asset (temporary resource name `-1`) and the ad group ad that uses it.
/// The ad is created `PAUSED` unless `status` says `ENABLED`; `REMOVED` is
/// refused.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
pub fn upload_display_ad(params: &UploadDisplayAdParams) -> Result<serde_json::Value> {
    check_blocked_operation("upload_display_ad", &params.config.safety)?;

    let ad_group_id = validate_numeric_id("ad_group_id", params.ad_group_id)?;
    let ad_name = params.ad_name.trim();
    if ad_name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "ad_name must not be empty — it names the ad and its asset in Google Ads".to_string(),
        ));
    }
    validate_final_url(params.final_url)?;
    let final_url = params.final_url.trim();
    let status = params.status.unwrap_or_default();
    if status == AdStatus::Removed {
        return Err(McpGoogleAdsError::Validation(
            "status REMOVED makes no sense for a new ad — use PAUSED (default) or ENABLED"
                .to_string(),
        ));
    }

    let file = check_file(params.file_path, params.ad_type)?;
    let size = file.bytes.len();
    let sha256 = sha256_hex(&file.bytes);

    let mut warnings: Vec<String> = Vec::new();
    let dimensions = match params.ad_type {
        DisplayAdType::Image => {
            let dims = image_dimensions(&file.bytes);
            match dims {
                Some((w, h)) if !STANDARD_DISPLAY_AD_SIZES.contains(&(w, h)) => {
                    warnings.push(format!(
                        "{}x{} is not one of Google's standard image ad sizes (e.g. 300x250, \
                         728x90, 160x600, 300x600, 320x50); Google may reject it as \
                         UNEXPECTED_SIZE unless it is a regional size",
                        w, h
                    ))
                }
                None => warnings
                    .push("could not read the image dimensions from the file header".to_string()),
                _ => {}
            }
            dims
        }
        DisplayAdType::Html5 => None,
    };

    let cid = GoogleAdsClient::normalize_customer_id(params.customer_id);
    let asset_resource = format!("customers/{}/assets/-1", cid);
    let data = base64_encode(&file.bytes);

    let (asset_create, ad_body, asset_type, served_as) = match params.ad_type {
        DisplayAdType::Html5 => (
            json!({
                "resourceName": asset_resource,
                "name": ad_name,
                "type": "MEDIA_BUNDLE",
                "mediaBundleAsset": { "data": data }
            }),
            json!({
                "displayUploadProductType": "HTML5_UPLOAD_AD",
                "mediaBundle": { "asset": asset_resource }
            }),
            "MEDIA_BUNDLE",
            "displayUploadAd (HTML5_UPLOAD_AD)",
        ),
        DisplayAdType::Image => (
            json!({
                "resourceName": asset_resource,
                "name": ad_name,
                "type": "IMAGE",
                "imageAsset": { "data": data }
            }),
            json!({
                "imageAsset": { "asset": asset_resource }
            }),
            "IMAGE",
            "imageAd",
        ),
    };
    let ad_field = match params.ad_type {
        DisplayAdType::Html5 => "displayUploadAd",
        DisplayAdType::Image => "imageAd",
    };

    let mut ad = json!({
        "name": ad_name,
        "finalUrls": [final_url],
    });
    ad[ad_field] = ad_body;

    let operations = vec![
        json!({ "assetOperation": { "create": asset_create } }),
        json!({
            "adGroupAdOperation": {
                "create": {
                    "adGroup": format!("customers/{}/adGroups/{}", cid, ad_group_id),
                    "status": status.as_api_str(),
                    "ad": ad
                }
            }
        }),
    ];

    let mut file_summary = json!({
        "path": file.path.display().to_string(),
        "file_name": file.path.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
        "size_bytes": size,
        "max_bytes": params.ad_type.max_file_bytes(),
        "sha256": sha256,
        "detected_format": file.format.name(),
    });
    if let Some((w, h)) = dimensions {
        file_summary["dimensions"] = json!(format!("{}x{}", w, h));
    }

    let mut changes = json!({
        "ad_group_id": ad_group_id,
        "ad_type": params.ad_type.as_api_str(),
        "ad_name": ad_name,
        "final_url": final_url,
        "status": status.as_api_str(),
        "file": file_summary,
        "creates": [
            format!("asset ({})", asset_type),
            format!("ad_group_ad ({})", served_as),
        ],
        "note": "The asset and the ad are created in one atomic mutate. After apply, the ad ID \
                 is the part after '~' in the adGroupAdResult resourceName \
                 (customers/{cid}/adGroupAds/{ad_group_id}~{ad_id}).",
    });
    if params.ad_type == DisplayAdType::Html5 {
        changes["eligibility"] = json!(
            "HTML5 upload ads are only available to accounts Google has allowlisted (open \
             more than 90 days, more than USD 9,000 lifetime spend, good policy history). \
             Run confirm_and_apply with validate_only=true first to see whether this account \
             qualifies."
        );
    }
    if !warnings.is_empty() {
        changes["warnings"] = json!(warnings);
    }

    let mut plan = ChangePlan::new(
        "upload_display_ad".to_string(),
        "ad_group_ad".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        operations,
    )
    .with_status_after_apply(status);

    if status == AdStatus::Paused {
        plan = plan.with_next_action_hint(NextActionHint {
            tool: "enable_entity".to_string(),
            params: json!({
                "entity_type": "ad",
                "entity_id": format!(
                    "{}~<ad_id from the adGroupAdResult resourceName>",
                    ad_group_id
                ),
            }),
            description: format!(
                "The ad was created in PAUSED status. After confirm_and_apply, read the ad ID \
                 from responses[1].adGroupAdResult.resourceName and call enable_entity with \
                 entity_type='ad' and entity_id='{}~<ad_id>' — no UI action required.",
                ad_group_id
            ),
        });
    }

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_refused() {
        let err = resolve_path("banners/ad.zip").unwrap_err();
        assert!(err.to_string().contains("absolute"));
        assert!(resolve_path("  ").is_err());
        assert!(resolve_path("/tmp/x.zip").is_ok());
    }

    #[test]
    fn standard_sizes_include_the_common_formats() {
        for size in [(300, 250), (728, 90), (160, 600), (300, 600), (320, 50)] {
            assert!(STANDARD_DISPLAY_AD_SIZES.contains(&size));
        }
    }
}
