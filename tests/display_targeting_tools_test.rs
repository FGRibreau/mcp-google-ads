//! Display remarketing needs four things the server could not do: target lists
//! per ad group, exclude lists, cap frequency, and keep ads off app inventory.
//! These tests drive the public crate API (no mocks) and inspect the stored
//! plans, plus the MCP parameter structs the tools are called through.

use mcp_google_ads::config::Config;
use mcp_google_ads::models::{FrequencyCap, FrequencyCapLevel, FrequencyCapTimeUnit};
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::set_campaign_frequency_caps;
use mcp_google_ads::{
    AddAdGroupAudiencesToolParams, ExcludeAudiencesToolParams, ExcludePlacementsToolParams,
    SetCampaignFrequencyCapsToolParams, UpdateAdGroupToolParams,
};
use serde_json::json;

fn cap(max_impressions: i32, time_unit: FrequencyCapTimeUnit) -> FrequencyCap {
    FrequencyCap {
        max_impressions,
        time_unit,
        level: None,
    }
}

fn campaign_update(preview: &serde_json::Value) -> serde_json::Value {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    plan.mutate_operations[0]["campaignOperation"].clone()
}

#[test]
fn frequency_caps_replace_the_list_under_one_mask() {
    let config = Config::default();
    let preview = set_campaign_frequency_caps(
        &config,
        "123-456-7890",
        "9000000002",
        &[
            cap(3, FrequencyCapTimeUnit::Day),
            cap(10, FrequencyCapTimeUnit::Week),
        ],
    )
    .expect("plan builds");

    assert_eq!(preview["operation"], "set_campaign_frequency_caps");
    assert_eq!(preview["entity_type"], "campaign");
    assert_eq!(preview["requires_double_confirm"], false);
    assert_eq!(
        preview["changes"]["frequency_caps"][1]["max_impressions"],
        10
    );

    let op = campaign_update(&preview);
    assert_eq!(op["updateMask"], "frequencyCaps");
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/campaigns/9000000002"
    );
    let caps = op["update"]["frequencyCaps"].as_array().expect("array");
    assert_eq!(caps.len(), 2);
    assert_eq!(caps[0]["cap"], 3);
    assert_eq!(caps[0]["key"]["timeUnit"], "DAY");
    assert_eq!(caps[0]["key"]["level"], "CAMPAIGN");
    assert_eq!(caps[0]["key"]["eventType"], "IMPRESSION");
    assert_eq!(caps[0]["key"]["timeLength"], 1);
    assert_eq!(caps[1]["key"]["timeUnit"], "WEEK");
}

#[test]
fn frequency_caps_refuse_empty_zero_duplicate_and_bad_ids() {
    let config = Config::default();

    let empty = set_campaign_frequency_caps(&config, "1234567890", "1", &[])
        .expect_err("an empty list must not silently clear every cap");
    assert!(empty.to_string().contains("must not be empty"));

    let zero = set_campaign_frequency_caps(
        &config,
        "1234567890",
        "1",
        &[cap(0, FrequencyCapTimeUnit::Day)],
    )
    .expect_err("zero impressions");
    assert!(zero.to_string().contains("at least 1"));

    let duplicate = set_campaign_frequency_caps(
        &config,
        "1234567890",
        "1",
        &[
            cap(3, FrequencyCapTimeUnit::Day),
            cap(5, FrequencyCapTimeUnit::Day),
        ],
    )
    .expect_err("two daily caps at the same level contradict each other");
    assert!(duplicate.to_string().contains("CAMPAIGN per DAY"));

    // Same window at two different levels is fine.
    set_campaign_frequency_caps(
        &config,
        "1234567890",
        "1",
        &[
            cap(3, FrequencyCapTimeUnit::Day),
            FrequencyCap {
                max_impressions: 2,
                time_unit: FrequencyCapTimeUnit::Day,
                level: Some(FrequencyCapLevel::AdGroupAd),
            },
        ],
    )
    .expect("one cap per level and window");

    let bad_id = set_campaign_frequency_caps(
        &config,
        "1234567890",
        "Display | Retargeting",
        &[cap(3, FrequencyCapTimeUnit::Day)],
    )
    .expect_err("campaign name instead of id");
    assert!(bad_id.to_string().contains("numeric"));
}

#[test]
fn frequency_caps_respect_blocked_operations() {
    let mut config = Config::default();
    config.safety.blocked_operations = vec!["set_campaign_frequency_caps".to_string()];
    let err = set_campaign_frequency_caps(
        &config,
        "1234567890",
        "1",
        &[cap(3, FrequencyCapTimeUnit::Day)],
    )
    .expect_err("blocked");
    assert!(err.to_string().contains("blocked"));
}

#[test]
fn tool_parameters_parse_as_an_mcp_host_sends_them() {
    let caps: SetCampaignFrequencyCapsToolParams = serde_json::from_value(json!({
        "campaign_id": "9000000002",
        "frequency_caps": [
            {"max_impressions": 3, "time_unit": "DAY"},
            {"max_impressions": 10, "time_unit": "WEEK", "level": "CAMPAIGN"}
        ]
    }))
    .expect("frequency caps payload");
    assert_eq!(caps.frequency_caps.len(), 2);
    assert_eq!(
        caps.frequency_caps[0].resolved_level(),
        FrequencyCapLevel::Campaign
    );

    let placements: ExcludePlacementsToolParams = serde_json::from_value(json!({
        "campaign_id": "9000000002",
        "mobile_app_category_ids": ["69500"]
    }))
    .expect("placements payload with only one list");
    assert!(placements.placement_urls.is_none());

    let exclude: ExcludeAudiencesToolParams = serde_json::from_value(json!({
        "campaign_id": "9000000002",
        "audience_ids": ["9000002001", "9000002002"]
    }))
    .expect("campaign-scoped exclusion payload");
    assert!(exclude.ad_group_ids.is_none());

    let add: AddAdGroupAudiencesToolParams = serde_json::from_value(json!({
        "ad_group_ids": ["9000000105"],
        "audience_ids": ["9000002003"]
    }))
    .expect("ad group audiences payload");
    assert_eq!(add.audience_ids, vec!["9000002003"]);

    let update: UpdateAdGroupToolParams = serde_json::from_value(json!({
        "ad_group_id": "9000000105",
        "optimized_targeting_enabled": false
    }))
    .expect("optimized targeting payload");
    assert_eq!(update.optimized_targeting_enabled, Some(false));
}

#[test]
fn tool_parameters_reject_unknown_keys() {
    let err = serde_json::from_value::<ExcludeAudiencesToolParams>(json!({
        "campaign_id": "1",
        "audience_ids": ["2"],
        "negative": true
    }))
    .expect_err("an undefined key must not be swallowed");
    assert!(err.to_string().contains("negative"));

    let err = serde_json::from_value::<SetCampaignFrequencyCapsToolParams>(json!({
        "campaign_id": "1",
        "frequency_caps": [{"max_impressions": 3, "time_unit": "DAY", "event_type": "VIDEO_VIEW"}]
    }))
    .expect_err("unknown key inside a cap");
    assert!(err.to_string().contains("event_type"));
}
