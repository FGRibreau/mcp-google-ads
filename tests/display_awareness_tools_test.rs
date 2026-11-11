//! A Display awareness campaign needs more than remarketing: custom segments,
//! interests, placements, account-wide exclusions, device adjustments, a
//! campaign that bids on one conversion action, and uploaded creatives. These
//! tests drive the public crate API (no HTTP) and inspect the stored plans:
//! the JSON each tool emits, its input validation and defaults, that every
//! emitted operation passes the client's whitelist, and the MCP parameter
//! structs the tools are called through.

use std::path::PathBuf;

use mcp_google_ads::client::GoogleAdsClient;
use mcp_google_ads::client::MutateOperation;
use mcp_google_ads::config::Config;
use mcp_google_ads::models::{
    AdStatus, CampaignConversionGoalInput, ContentLabelType, ConversionActionCategory,
    ConversionOrigin, CustomSegmentType, DisplayAdType,
};
use mcp_google_ads::safety::preview::{get_plan, remove_plan, ChangePlan, PlanDispatch};
use mcp_google_ads::tools::{
    audiences, conversion_goals, custom_segments, devices, display_ads, keywords_write, placements,
};
use mcp_google_ads::{
    AddAccountExclusionsToolParams, AddAdGroupCustomSegmentsToolParams,
    AddAdGroupPlacementsToolParams, AddAdGroupUserInterestsToolParams, ConfirmApplyParams,
    CreateCustomConversionGoalToolParams, CreateCustomSegmentToolParams,
    SetCampaignConversionGoalsToolParams, SetCampaignCustomConversionGoalToolParams,
    SetCampaignDeviceModifiersToolParams, UpdateAdGroupToolParams, UploadDisplayAdToolParams,
};
use serde_json::json;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

/// Take a stored plan out of the store.
fn take_plan(preview: &serde_json::Value) -> ChangePlan {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    remove_plan(plan_id);
    plan
}

/// Every googleAds:mutate plan must pass the client's whitelist, or it fails
/// on every apply before reaching Google.
fn assert_whitelisted(plan: &ChangePlan) {
    let ops: Vec<MutateOperation> = plan
        .mutate_operations
        .iter()
        .map(|op| MutateOperation {
            operation: op.clone(),
        })
        .collect();
    GoogleAdsClient::validate_mutate_operations(&ops)
        .unwrap_or_else(|e| panic!("{} emits a rejected operation: {e}", plan.operation));
}

// ── Custom segments ────────────────────────────────────────────────────────

#[test]
fn custom_segment_goes_to_its_own_service_with_bare_operations() {
    let config = Config::default();
    let preview = custom_segments::create_custom_segment(
        &config,
        "123-456-7890",
        "Runners without a watch",
        None,
        None,
        &strings(&["buy running shoes", "marathon 2027"]),
        &strings(&["https://www.example.org/running/shoes/"]),
        &strings(&["com.example.runner"]),
    )
    .expect("plan builds");

    assert_eq!(preview["operation"], "create_custom_segment");
    assert_eq!(
        preview["changes"]["segment_type"], "AUTO",
        "AUTO by default"
    );
    assert_eq!(preview["changes"]["endpoint"], "customAudiences:mutate");

    let plan = take_plan(&preview);
    assert_eq!(plan.dispatch, PlanDispatch::CustomAudiences);
    GoogleAdsClient::validate_custom_audience_operations(&plan.mutate_operations)
        .expect("a bare CustomAudienceOperation");
    let create = &plan.mutate_operations[0]["create"];
    assert_eq!(create["type"], "AUTO");
    assert_eq!(create["members"].as_array().map(Vec::len), Some(4));
    assert_eq!(create["members"][3]["memberType"], "APP");
    assert_eq!(create["members"][3]["app"], "com.example.runner");
}

#[test]
fn custom_segment_validation() {
    let config = Config::default();
    let run = |keywords: &[&str], urls: &[&str], apps: &[&str]| {
        custom_segments::create_custom_segment(
            &config,
            "1234567890",
            "S",
            None,
            Some(CustomSegmentType::Search),
            &strings(keywords),
            &strings(urls),
            &strings(apps),
        )
    };
    assert!(run(&[], &[], &[]).is_err(), "no members");
    assert!(
        run(&[&"x".repeat(81)], &[], &[]).is_err(),
        "keyword over 80 chars"
    );
    assert!(
        run(&[], &["example.org"], &[]).is_err(),
        "URL without protocol"
    );
    assert!(
        run(&[], &[], &["2-com.example.runner"]).is_err(),
        "app ID prefix"
    );
    let ok = run(&["a", "A", "a"], &[], &[]).expect("dupes collapse");
    assert_eq!(
        take_plan(&ok).mutate_operations[0]["create"]["members"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn ad_group_custom_segments_and_user_interests_fan_out() {
    let config = Config::default();
    let preview = audiences::add_ad_group_custom_segments(
        &config,
        "123-456-7890",
        &strings(&["111", "222"]),
        &strings(&["9001", "9001"]),
    )
    .expect("ok");
    assert_eq!(preview["changes"]["total_criteria"], 2);
    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    let create = &plan.mutate_operations[1]["adGroupCriterionOperation"]["create"];
    assert_eq!(create["adGroup"], "customers/1234567890/adGroups/222");
    assert_eq!(
        create["customAudience"]["customAudience"],
        "customers/1234567890/customAudiences/9001"
    );
    assert!(create.get("negative").is_none(), "positive only");

    let preview = audiences::add_ad_group_user_interests(
        &config,
        "1234567890",
        &strings(&["111"]),
        &strings(&["92926", "80432"]),
    )
    .expect("ok");
    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    assert_eq!(plan.mutate_operations.len(), 2);
    assert_eq!(
        plan.mutate_operations[0]["adGroupCriterionOperation"]["create"]["userInterest"]
            ["userInterestCategory"],
        "customers/1234567890/userInterests/92926"
    );

    for err in [
        audiences::add_ad_group_custom_segments(&config, "1", &[], &strings(&["1"])),
        audiences::add_ad_group_custom_segments(&config, "1", &strings(&["1"]), &[]),
        audiences::add_ad_group_user_interests(
            &config,
            "1",
            &strings(&["1"]),
            &strings(&["uservertical::92926"]),
        ),
    ] {
        assert!(err.is_err());
    }
}

// ── Placements and account exclusions ──────────────────────────────────────

#[test]
fn ad_group_placements_positive_by_default_and_negative_on_request() {
    let config = Config::default();
    let positive = placements::add_ad_group_placements(
        &config,
        "123-456-7890",
        &strings(&["111"]),
        &strings(&["https://www.news.example/", "news.example"]),
        false,
        &[],
    )
    .expect("ok");
    assert_eq!(positive["changes"]["negative"], false);
    let plan = take_plan(&positive);
    assert_whitelisted(&plan);
    assert_eq!(plan.mutate_operations.len(), 2, "www. and bare host differ");
    let create = &plan.mutate_operations[0]["adGroupCriterionOperation"]["create"];
    assert_eq!(create["placement"]["url"], "www.news.example");
    assert!(create.get("negative").is_none());

    let negative = placements::add_ad_group_placements(
        &config,
        "1234567890",
        &strings(&["111", "222"]),
        &strings(&["example.com"]),
        true,
        &strings(&["2-com.example.game"]),
    )
    .expect("ok");
    let plan = take_plan(&negative);
    assert_eq!(
        plan.mutate_operations.len(),
        4,
        "2 ad groups × (1 site + 1 app)"
    );
    assert!(plan
        .mutate_operations
        .iter()
        .all(|op| op["adGroupCriterionOperation"]["create"]["negative"] == true));
    assert_eq!(
        plan.mutate_operations[1]["adGroupCriterionOperation"]["create"]["mobileApplication"]
            ["appId"],
        "2-com.example.game"
    );
}

#[test]
fn ad_group_placement_validation() {
    let config = Config::default();
    let apps_without_negative = placements::add_ad_group_placements(
        &config,
        "1",
        &strings(&["111"]),
        &[],
        false,
        &strings(&["2-com.example.game"]),
    );
    assert!(apps_without_negative
        .unwrap_err()
        .to_string()
        .contains("negative=true"));
    assert!(
        placements::add_ad_group_placements(&config, "1", &strings(&["111"]), &[], true, &[])
            .is_err()
    );
    let too_long = format!("example.com/{}", "a".repeat(250));
    assert!(placements::add_ad_group_placements(
        &config,
        "1",
        &strings(&["111"]),
        &[too_long],
        false,
        &[]
    )
    .is_err());
    assert!(placements::add_ad_group_placements(
        &config,
        "1",
        &strings(&["111"]),
        &strings(&["adsenseformobileapps.com"]),
        true,
        &[]
    )
    .is_err());
}

#[test]
fn account_exclusions_skip_what_exists_and_flag_caveats() {
    let config = Config::default();
    let existing = placements::AccountExclusions {
        placement_urls: strings(&["Example.com"]),
        mobile_app_category_ids: strings(&["60008"]),
        mobile_app_ids: vec![],
        content_labels: vec![ContentLabelType::Tragedy],
    };
    let preview = placements::add_account_exclusions(
        &config,
        "123-456-7890",
        &strings(&["https://example.com/", "news.example.org"]),
        &strings(&["60008"]),
        &strings(&["1-123456789"]),
        &[
            ContentLabelType::Tragedy,
            ContentLabelType::BelowTheFold,
            ContentLabelType::ParkedDomain,
        ],
        &existing,
    )
    .expect("ok");

    assert_eq!(preview["operation"], "add_account_exclusions");
    assert_eq!(preview["changes"]["total_new_exclusions"], 4);
    assert_eq!(
        preview["changes"]["already_excluded"]["placement_urls"],
        json!(["example.com"])
    );
    assert_eq!(
        preview["changes"]["already_excluded"]["content_label_types"],
        json!(["TRAGEDY"])
    );
    assert_eq!(
        preview["changes"]["content_label_caveats"][0]["content_label_type"],
        "PARKED_DOMAIN"
    );

    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    let creates: Vec<&serde_json::Value> = plan
        .mutate_operations
        .iter()
        .map(|op| &op["customerNegativeCriterionOperation"]["create"])
        .collect();
    assert_eq!(
        creates[0],
        &json!({"placement": {"url": "news.example.org"}})
    );
    assert_eq!(
        creates[1],
        &json!({"mobileApplication": {"appId": "1-123456789"}})
    );
    assert_eq!(
        creates[2],
        &json!({"contentLabel": {"type": "BELOW_THE_FOLD"}})
    );
    assert_eq!(
        creates[3],
        &json!({"contentLabel": {"type": "PARKED_DOMAIN"}})
    );
}

#[test]
fn account_exclusions_with_nothing_new_create_no_plan() {
    let config = Config::default();
    let existing = placements::AccountExclusions {
        mobile_app_category_ids: strings(&["60008"]),
        ..Default::default()
    };
    let answer = placements::add_account_exclusions(
        &config,
        "1234567890",
        &[],
        &strings(&["mobileAppCategoryConstants/60008"]),
        &[],
        &[],
        &existing,
    )
    .expect("ok");
    assert_eq!(answer["status"], "NO_CHANGES");
    assert!(answer.get("plan_id").is_none());
}

#[test]
fn account_exclusions_refuse_all_apps() {
    // Google answers INVALID_MOBILE_APP_CATEGORY for 69500 at account level
    // (validateOnly against a live account, 2026-11-11).
    let config = Config::default();
    let err = placements::add_account_exclusions(
        &config,
        "1234567890",
        &[],
        &strings(&["69500"]),
        &[],
        &[],
        &placements::AccountExclusions::default(),
    )
    .expect_err("refused");
    let message = err.to_string();
    assert!(message.contains("69500"), "{message}");
    assert!(message.contains("exclude_placements"), "{message}");
}

#[test]
fn account_exclusions_refuse_every_digital_content_label_and_empty_requests() {
    let config = Config::default();
    let existing = placements::AccountExclusions {
        content_labels: vec![
            ContentLabelType::VideoRatingDvG,
            ContentLabelType::VideoRatingDvPg,
        ],
        ..Default::default()
    };
    let err = placements::add_account_exclusions(
        &config,
        "1",
        &[],
        &[],
        &[],
        &[
            ContentLabelType::VideoRatingDvT,
            ContentLabelType::VideoRatingDvMa,
            ContentLabelType::VideoNotYetRated,
        ],
        &existing,
    )
    .expect_err("all five DL labels");
    assert!(err.to_string().contains("digital content labels"));

    let empty = placements::add_account_exclusions(
        &config,
        "1",
        &[],
        &[],
        &[],
        &[],
        &placements::AccountExclusions::default(),
    );
    assert!(empty
        .unwrap_err()
        .to_string()
        .contains("nothing to exclude"));
}

// ── Devices ────────────────────────────────────────────────────────────────

#[test]
fn device_modifiers_create_or_update() {
    let config = Config::default();
    let existing = [devices::ExistingDeviceCriterion {
        device: "TABLET".to_string(),
        resource_name: "customers/1234567890/campaignCriteria/555~30002".to_string(),
        bid_modifier: Some(1.0),
    }];
    let preview = devices::set_campaign_device_modifiers(
        &config,
        "123-456-7890",
        "555",
        &devices::DeviceModifiers {
            tablet: Some(0.0),
            connected_tv: Some(0.0),
            ..Default::default()
        },
        &existing,
    )
    .expect("ok");
    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    assert_eq!(
        plan.mutate_operations[0]["campaignCriterionOperation"]["update"]["resourceName"],
        "customers/1234567890/campaignCriteria/555~30002"
    );
    assert_eq!(
        plan.mutate_operations[1]["campaignCriterionOperation"]["create"]["device"]["type"],
        "CONNECTED_TV"
    );
}

// ── Conversion goals ───────────────────────────────────────────────────────

#[test]
fn one_campaign_bids_on_one_action_path() {
    let config = Config::default();

    let create = conversion_goals::create_custom_conversion_goal(
        &config,
        "123-456-7890",
        "Display | Quote",
        &strings(&["7001"]),
    )
    .expect("ok");
    let plan = take_plan(&create);
    assert_whitelisted(&plan);
    assert_eq!(
        plan.mutate_operations[0]["customConversionGoalOperation"]["create"]["conversionActions"],
        json!(["customers/1234567890/conversionActions/7001"])
    );

    let existing = [conversion_goals::ExistingCampaignGoal {
        resource_name: "customers/1234567890/campaignConversionGoals/555~SUBMIT_LEAD_FORM~WEBSITE"
            .to_string(),
        category: "SUBMIT_LEAD_FORM".to_string(),
        origin: "WEBSITE".to_string(),
        biddable: true,
    }];
    let set = conversion_goals::set_campaign_custom_conversion_goal(
        &config,
        "123-456-7890",
        "555",
        "8001",
        Some(&existing),
    )
    .expect("ok");
    assert_eq!(set["changes"]["exclusive"], true);
    let plan = take_plan(&set);
    assert_whitelisted(&plan);
    assert_eq!(plan.mutate_operations.len(), 2);
    assert_eq!(
        plan.mutate_operations[0]["conversionGoalCampaignConfigOperation"]["updateMask"],
        "customConversionGoal"
    );
    assert_eq!(
        plan.mutate_operations[1]["campaignConversionGoalOperation"]["update"]["biddable"],
        false
    );
}

#[test]
fn campaign_conversion_goals_by_category_and_origin() {
    let config = Config::default();
    let preview = conversion_goals::set_campaign_conversion_goals(
        &config,
        "1234567890",
        "555",
        &[CampaignConversionGoalInput {
            category: ConversionActionCategory::RequestQuote,
            origin: ConversionOrigin::Website,
            biddable: true,
        }],
    )
    .expect("ok");
    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    assert_eq!(
        plan.mutate_operations[0]["campaignConversionGoalOperation"]["update"]["resourceName"],
        "customers/1234567890/campaignConversionGoals/555~REQUEST_QUOTE~WEBSITE"
    );
}

// ── Display keywords ───────────────────────────────────────────────────────

#[test]
fn draft_keywords_emit_plain_keyword_criteria_usable_as_display_keywords() {
    let config = Config::default();
    let preview = keywords_write::draft_keywords(
        &config,
        "123-456-7890",
        "111",
        vec![keywords_write::KeywordWithMatchType {
            text: "running shoes".to_string(),
            match_type: "BROAD".to_string(),
            final_url: None,
        }],
    )
    .expect("ok");
    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    // The same KeywordInfo criterion a DISPLAY_STANDARD ad group uses for
    // contextual targeting: nothing Search-specific is emitted.
    assert_eq!(
        plan.mutate_operations[0],
        json!({
            "adGroupCriterionOperation": {
                "create": {
                    "adGroup": "customers/1234567890/adGroups/111",
                    "keyword": {"text": "running shoes", "matchType": "BROAD"}
                }
            }
        })
    );
}

// ── Uploaded display ads ───────────────────────────────────────────────────

/// Write `bytes` to a fresh temp file with the given extension.
fn temp_file(extension: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-google-ads-display-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ));
    std::fs::write(&path, bytes).expect("write temp file");
    path
}

/// A PNG header for a 300x250 image, plus a few bytes of payload.
fn png_300x250() -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    png.extend_from_slice(&300u32.to_be_bytes());
    png.extend_from_slice(&250u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(b"display-awareness-test-image");
    png
}

fn upload(
    path: &str,
    ad_type: DisplayAdType,
    status: Option<AdStatus>,
) -> mcp_google_ads::error::Result<serde_json::Value> {
    let config = Config::default();
    display_ads::upload_display_ad(&display_ads::UploadDisplayAdParams {
        config: &config,
        customer_id: "123-456-7890",
        ad_group_id: "111",
        file_path: path,
        ad_type,
        ad_name: "Banner | 300x250",
        final_url: "https://example.com/",
        status,
    })
}

#[test]
fn image_ad_is_one_atomic_mutate_with_a_temporary_asset() {
    let bytes = png_300x250();
    let path = temp_file("PNG", &bytes);
    let preview = upload(path.to_str().unwrap(), DisplayAdType::Image, None).expect("ok");

    assert_eq!(preview["operation"], "upload_display_ad");
    assert_eq!(preview["status_after_apply"], "PAUSED");
    assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
    let file = &preview["changes"]["file"];
    assert_eq!(file["size_bytes"], 57);
    assert_eq!(file["max_bytes"], 153_600);
    assert_eq!(
        file["sha256"],
        "d3763b4f4549cfe499a416aef497ba9b4dd5a903d9fa11faa1eee9fd93be188c"
    );
    assert_eq!(file["dimensions"], "300x250");
    assert!(
        preview["changes"].get("warnings").is_none(),
        "300x250 is standard"
    );

    let expected_base64 =
        "iVBORw0KGgoAAAANSUhEUgAAASwAAAD6CAYAAABkaXNwbGF5LWF3YXJlbmVzcy10ZXN0LWltYWdl";
    assert!(
        !preview.to_string().contains(expected_base64),
        "the preview must not carry the file content"
    );

    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    assert_eq!(plan.mutate_operations.len(), 2);
    let asset = &plan.mutate_operations[0]["assetOperation"]["create"];
    assert_eq!(asset["resourceName"], "customers/1234567890/assets/-1");
    assert_eq!(asset["type"], "IMAGE");
    assert_eq!(asset["name"], "Banner | 300x250");
    assert_eq!(asset["imageAsset"]["data"], expected_base64);

    let ad_group_ad = &plan.mutate_operations[1]["adGroupAdOperation"]["create"];
    assert_eq!(ad_group_ad["adGroup"], "customers/1234567890/adGroups/111");
    assert_eq!(ad_group_ad["status"], "PAUSED");
    assert_eq!(ad_group_ad["ad"]["name"], "Banner | 300x250");
    assert_eq!(
        ad_group_ad["ad"]["finalUrls"],
        json!(["https://example.com/"])
    );
    assert_eq!(
        ad_group_ad["ad"]["imageAd"]["imageAsset"]["asset"],
        "customers/1234567890/assets/-1"
    );
    // Google requires a display URL on image ads: the final URL's host.
    assert_eq!(ad_group_ad["ad"]["displayUrl"], "example.com");
    assert_eq!(preview["changes"]["display_url"], "example.com");
    std::fs::remove_file(path).ok();
}

#[test]
fn html5_ad_uses_a_media_bundle_and_display_upload_ad() {
    let bytes: Vec<u8> = [
        b"PK\x03\x04".as_slice(),
        &b"html5-bundle-for-tests".repeat(10),
    ]
    .concat();
    let path = temp_file("zip", &bytes);
    let preview = upload(
        path.to_str().unwrap(),
        DisplayAdType::Html5,
        Some(AdStatus::Enabled),
    )
    .expect("ok");

    assert_eq!(preview["status_after_apply"], "ENABLED");
    assert!(preview.get("next_action_hint").is_none());
    assert_eq!(
        preview["changes"]["file"]["sha256"],
        "54b450a3a712655ce7926adca46fbda516d90b646e5584305d49481faed312df"
    );
    assert!(preview["changes"]["eligibility"]
        .as_str()
        .unwrap_or_default()
        .contains("allowlisted"));

    let plan = take_plan(&preview);
    assert_whitelisted(&plan);
    let asset = &plan.mutate_operations[0]["assetOperation"]["create"];
    assert_eq!(asset["type"], "MEDIA_BUNDLE");
    assert!(asset["mediaBundleAsset"]["data"].is_string());
    let ad = &plan.mutate_operations[1]["adGroupAdOperation"]["create"]["ad"];
    assert_eq!(
        ad["displayUploadAd"],
        json!({
            "displayUploadProductType": "HTML5_UPLOAD_AD",
            "mediaBundle": {"asset": "customers/1234567890/assets/-1"}
        })
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn upload_checks_the_file_before_any_plan() {
    // Too big for an image ad.
    let mut big = png_300x250();
    big.resize(150 * 1024 + 1, 0);
    let path = temp_file("png", &big);
    let err = upload(path.to_str().unwrap(), DisplayAdType::Image, None).unwrap_err();
    assert!(err.to_string().contains("at most 153600 bytes"), "{err}");
    std::fs::remove_file(path).ok();

    // Extension the ad type does not take.
    let path = temp_file("png", &png_300x250());
    let err = upload(path.to_str().unwrap(), DisplayAdType::Html5, None).unwrap_err();
    assert!(err.to_string().contains(".zip"));
    std::fs::remove_file(path).ok();

    // Content that does not match the extension.
    let path = temp_file("jpg", &png_300x250());
    let err = upload(path.to_str().unwrap(), DisplayAdType::Image, None).unwrap_err();
    assert!(err.to_string().contains("content is PNG"));
    std::fs::remove_file(path).ok();

    // Missing file, relative path, empty file, REMOVED status.
    let missing = std::env::temp_dir().join("mcp-google-ads-does-not-exist.zip");
    assert!(upload(missing.to_str().unwrap(), DisplayAdType::Html5, None).is_err());
    assert!(upload("banners/banner.zip", DisplayAdType::Html5, None)
        .unwrap_err()
        .to_string()
        .contains("absolute"));
    let empty = temp_file("gif", b"");
    assert!(upload(empty.to_str().unwrap(), DisplayAdType::Image, None)
        .unwrap_err()
        .to_string()
        .contains("empty"));
    std::fs::remove_file(empty).ok();
    let path = temp_file("png", &png_300x250());
    assert!(upload(
        path.to_str().unwrap(),
        DisplayAdType::Image,
        Some(AdStatus::Removed)
    )
    .is_err());
    std::fs::remove_file(path).ok();
}

#[test]
fn non_standard_image_size_is_a_warning_not_a_refusal() {
    let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    png.extend_from_slice(&600u32.to_be_bytes());
    png.extend_from_slice(&500u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    let path = temp_file("png", &png);
    let preview = upload(path.to_str().unwrap(), DisplayAdType::Image, None).expect("ok");
    assert!(preview["changes"]["warnings"][0]
        .as_str()
        .unwrap_or_default()
        .contains("600x500"));
    take_plan(&preview);
    std::fs::remove_file(path).ok();
}

// ── MCP parameter structs ──────────────────────────────────────────────────

#[test]
fn tool_parameters_parse_as_an_mcp_host_sends_them() {
    let segment: CreateCustomSegmentToolParams = serde_json::from_value(json!({
        "name": "Running shoe searchers",
        "segment_type": "SEARCH",
        "keywords": ["running shoes"]
    }))
    .expect("segment payload");
    assert_eq!(segment.segment_type, Some(CustomSegmentType::Search));
    assert!(
        serde_json::from_value::<CreateCustomSegmentToolParams>(json!({
            "name": "x", "segment_type": "PURCHASE_INTENT", "keywords": ["y"]
        }))
        .is_err()
    );

    let placements: AddAdGroupPlacementsToolParams = serde_json::from_value(json!({
        "ad_group_ids": ["1"],
        "placement_urls": ["news.example"]
    }))
    .expect("placements payload");
    assert!(placements.negative.is_none(), "defaults to targeting");

    let exclusions: AddAccountExclusionsToolParams = serde_json::from_value(json!({
        "mobile_app_category_ids": ["69500"],
        "content_label_types": ["BELOW_THE_FOLD", "PARKED_DOMAIN"]
    }))
    .expect("exclusions payload");
    assert_eq!(exclusions.content_label_types.map(|l| l.len()), Some(2));
    assert!(
        serde_json::from_value::<AddAccountExclusionsToolParams>(json!({
            "content_label_types": ["UNKNOWN"]
        }))
        .is_err()
    );

    let goals: SetCampaignConversionGoalsToolParams = serde_json::from_value(json!({
        "campaign_id": "555",
        "goals": [{"category": "SUBMIT_LEAD_FORM", "origin": "WEBSITE", "biddable": true}]
    }))
    .expect("goals payload");
    assert_eq!(goals.goals[0].origin, ConversionOrigin::Website);

    let custom: SetCampaignCustomConversionGoalToolParams = serde_json::from_value(json!({
        "campaign_id": "555", "custom_conversion_goal_id": "8001"
    }))
    .expect("custom goal payload");
    assert!(
        custom.exclusive.is_none(),
        "defaults to exclusive in the tool"
    );

    let upload: UploadDisplayAdToolParams = serde_json::from_value(json!({
        "ad_group_id": "111",
        "file_path": "/tmp/banner.zip",
        "ad_type": "HTML5",
        "ad_name": "Banner",
        "final_url": "https://example.com/"
    }))
    .expect("upload payload");
    assert_eq!(upload.ad_type, DisplayAdType::Html5);

    let confirm: ConfirmApplyParams = serde_json::from_value(json!({
        "plan_id": "abc", "validate_only": true
    }))
    .expect("confirm payload");
    assert_eq!(confirm.validate_only, Some(true));

    let update: UpdateAdGroupToolParams = serde_json::from_value(json!({
        "ad_group_id": "111", "audience_targeting_mode": "OBSERVATION"
    }))
    .expect("update payload");
    assert!(update.audience_targeting_mode.is_some());

    serde_json::from_value::<SetCampaignDeviceModifiersToolParams>(json!({
        "campaign_id": "555", "mobile": 0, "connected_tv": 0
    }))
    .expect("device payload");
    serde_json::from_value::<AddAdGroupCustomSegmentsToolParams>(json!({
        "ad_group_ids": ["1"], "custom_segment_ids": ["2"]
    }))
    .expect("custom segments payload");
    serde_json::from_value::<AddAdGroupUserInterestsToolParams>(json!({
        "ad_group_ids": ["1"], "user_interest_ids": ["92926"]
    }))
    .expect("user interests payload");
    serde_json::from_value::<CreateCustomConversionGoalToolParams>(json!({
        "name": "x", "conversion_action_ids": ["1"]
    }))
    .expect("custom goal payload");
}

#[test]
fn tool_parameters_reject_unknown_keys() {
    let cases = [
        (
            "create_custom_segment",
            serde_json::from_value::<CreateCustomSegmentToolParams>(
                json!({"name": "x", "members": []}),
            )
            .err(),
        ),
        (
            "add_ad_group_custom_segments",
            serde_json::from_value::<AddAdGroupCustomSegmentsToolParams>(
                json!({"ad_group_ids": [], "custom_segment_ids": [], "negative": true}),
            )
            .err(),
        ),
        (
            "add_ad_group_user_interests",
            serde_json::from_value::<AddAdGroupUserInterestsToolParams>(
                json!({"ad_group_ids": [], "user_interest_ids": [], "mode": "TARGETING"}),
            )
            .err(),
        ),
        (
            "add_ad_group_placements",
            serde_json::from_value::<AddAdGroupPlacementsToolParams>(
                json!({"ad_group_ids": [], "urls": []}),
            )
            .err(),
        ),
        (
            "add_account_exclusions",
            serde_json::from_value::<AddAccountExclusionsToolParams>(json!({"campaign_id": "1"}))
                .err(),
        ),
        (
            "set_campaign_device_modifiers",
            serde_json::from_value::<SetCampaignDeviceModifiersToolParams>(
                json!({"campaign_id": "1", "smartphone": 0}),
            )
            .err(),
        ),
        (
            "create_custom_conversion_goal",
            serde_json::from_value::<CreateCustomConversionGoalToolParams>(
                json!({"name": "x", "conversion_action_ids": [], "primary": true}),
            )
            .err(),
        ),
        (
            "set_campaign_custom_conversion_goal",
            serde_json::from_value::<SetCampaignCustomConversionGoalToolParams>(
                json!({"campaign_id": "1", "custom_conversion_goal_id": "2", "only": true}),
            )
            .err(),
        ),
        (
            "set_campaign_conversion_goals",
            serde_json::from_value::<SetCampaignConversionGoalsToolParams>(
                json!({"campaign_id": "1", "goals": [], "level": "CAMPAIGN"}),
            )
            .err(),
        ),
        (
            "upload_display_ad",
            serde_json::from_value::<UploadDisplayAdToolParams>(json!({
                "ad_group_id": "1", "file_path": "/x.zip", "ad_type": "HTML5",
                "ad_name": "x", "final_url": "https://x.example", "image_base64": "..."
            }))
            .err(),
        ),
    ];
    for (tool, err) in cases {
        assert!(err.is_some(), "{tool} must reject an undefined key");
    }
}
