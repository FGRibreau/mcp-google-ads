//! A campaign's bidding strategy is written with all its values, at creation and
//! on update, and only with the values it takes.
//!
//! Target Impression Share used to be created as an empty scheme (Google rejects
//! it without a location and a fraction) and could not be set on update at all;
//! Maximize Clicks dropped its CPC ceiling; and any value a strategy ignores was
//! dropped silently. Asserts what reaches the wire, then each rejection.

mod common;

use common::test_config;
use mcp_google_ads::config::Config;
use mcp_google_ads::models::TargetImpressionShareLocation;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::{
    draft_campaign, update_campaign, DraftCampaignParams, UpdateCampaignParams,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Bidding values of a test call: target CPA, target ROAS, CPC ceiling, impression share location and percent.
type Values = (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<TargetImpressionShareLocation>,
    Option<f64>,
);

const NONE: Values = (None, None, None, None, None);

fn draft<'a>(config: &'a Config, strategy: &'a str, values: Values) -> DraftCampaignParams<'a> {
    DraftCampaignParams {
        config,
        customer_id: "1234567890",
        campaign_name: "Bidding",
        daily_budget: 20.0,
        bidding_strategy: strategy,
        target_cpa: values.0,
        target_roas: values.1,
        channel_type: "SEARCH",
        ad_group_name: "AG1",
        keywords: vec![],
        geo_target_ids: vec![],
        language_ids: vec![],
        status: None,
        start_date_time: None,
        end_date_time: None,
        cpc_bid_ceiling: values.2,
        target_impression_share_location: values.3,
        target_impression_share_percent: values.4,
    }
}

fn update<'a>(config: &'a Config, strategy: &'a str, values: Values) -> UpdateCampaignParams<'a> {
    UpdateCampaignParams {
        config,
        customer_id: "1234567890",
        campaign_id: "777",
        bidding_strategy: Some(strategy),
        target_cpa: values.0,
        target_roas: values.1,
        daily_budget: None,
        budget_resource_name: None,
        geo_target_ids: vec![],
        language_ids: vec![],
        start_date_time: None,
        end_date_time: None,
        cpc_bid_ceiling: values.2,
        target_impression_share_location: values.3,
        target_impression_share_percent: values.4,
    }
}

/// The campaign operation of a drafted plan.
fn campaign_operation(preview: &serde_json::Value) -> serde_json::Value {
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();
    plan.mutate_operations
        .iter()
        .find_map(|op| op.get("campaignOperation").cloned())
        .expect("a campaign operation")
}

#[tokio::test]
async fn target_impression_share_reaches_google_with_all_its_values() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"mutateOperationResponses": []})),
        )
        .mount(&mock)
        .await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let config = test_config();
    let values = (
        None,
        None,
        Some(2.0),
        Some(TargetImpressionShareLocation::AbsoluteTopOfPage),
        Some(80.0),
    );
    let preview = draft_campaign(&draft(&config, "TARGET_IMPRESSION_SHARE", values)).unwrap();
    confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: preview["plan_id"].as_str().unwrap().to_string(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply succeeds");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let scheme =
        &body["mutateOperations"][1]["campaignOperation"]["create"]["targetImpressionShare"];
    assert_eq!(scheme["location"], "ABSOLUTE_TOP_OF_PAGE");
    // 1 % is 10,000 micros.
    assert_eq!(scheme["locationFractionMicros"], "800000");
    assert_eq!(scheme["cpcBidCeilingMicros"], "2000000");
}

#[test]
fn maximize_clicks_is_created_with_its_cpc_ceiling() {
    let config = test_config();
    let preview = draft_campaign(&draft(
        &config,
        "TARGET_SPEND",
        (None, None, Some(1.5), None, None),
    ))
    .unwrap();
    let op = campaign_operation(&preview);
    assert_eq!(
        op["create"]["targetSpend"]["cpcBidCeilingMicros"],
        "1500000"
    );
}

#[test]
fn target_impression_share_is_updated_leaf_by_leaf() {
    let config = test_config();
    let values = (
        None,
        None,
        None,
        Some(TargetImpressionShareLocation::TopOfPage),
        Some(50.0),
    );
    let preview = update_campaign(&update(&config, "TARGET_IMPRESSION_SHARE", values)).unwrap();
    let op = campaign_operation(&preview);

    let scheme = &op["update"]["targetImpressionShare"];
    assert_eq!(scheme["location"], "TOP_OF_PAGE");
    assert_eq!(scheme["locationFractionMicros"], "500000");
    // No ceiling given: sent as 0, which clears a previous one.
    assert_eq!(scheme["cpcBidCeilingMicros"], "0");
    let mut mask: Vec<&str> = op["updateMask"].as_str().unwrap().split(',').collect();
    mask.sort();
    assert_eq!(
        mask,
        vec![
            "targetImpressionShare.cpcBidCeilingMicros",
            "targetImpressionShare.location",
            "targetImpressionShare.locationFractionMicros",
        ]
    );
    assert!(op["update"].get("biddingStrategyType").is_none());
}

#[test]
fn maximize_clicks_is_updated_with_its_cpc_ceiling() {
    let config = test_config();
    let preview = update_campaign(&update(
        &config,
        "TARGET_SPEND",
        (None, None, Some(1.25), None, None),
    ))
    .unwrap();
    let op = campaign_operation(&preview);
    assert_eq!(
        op["update"]["targetSpend"]["cpcBidCeilingMicros"],
        "1250000"
    );
    assert_eq!(op["updateMask"], "targetSpend.cpcBidCeilingMicros");
}

#[test]
fn a_strategy_without_its_required_value_is_rejected() {
    let config = test_config();
    let location = Some(TargetImpressionShareLocation::TopOfPage);
    for (strategy, values, expected) in [
        ("TARGET_CPA", NONE, "TARGET_CPA requires target_cpa"),
        ("TARGET_ROAS", NONE, "TARGET_ROAS requires target_roas"),
        (
            "TARGET_IMPRESSION_SHARE",
            (None, None, None, None, Some(80.0)),
            "requires target_impression_share_location",
        ),
        (
            "TARGET_IMPRESSION_SHARE",
            (None, None, None, location, None),
            "requires target_impression_share_percent",
        ),
    ] {
        let err = draft_campaign(&draft(&config, strategy, values))
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{strategy}: {err}");
        let err = update_campaign(&update(&config, strategy, values))
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{strategy} update: {err}");
    }
}

#[test]
fn a_value_the_strategy_ignores_is_rejected_not_dropped() {
    let config = test_config();
    for (strategy, values, expected) in [
        (
            "MANUAL_CPC",
            (Some(10.0), None, None, None, None),
            "MANUAL_CPC takes no target_cpa",
        ),
        (
            "TARGET_SPEND",
            (Some(10.0), None, None, None, None),
            "TARGET_SPEND takes no target_cpa",
        ),
        (
            "MAXIMIZE_CONVERSIONS",
            (None, Some(3.0), None, None, None),
            "takes no target_roas",
        ),
        (
            "TARGET_CPA",
            (Some(10.0), None, Some(2.0), None, None),
            "TARGET_CPA takes no cpc_bid_ceiling",
        ),
    ] {
        let err = draft_campaign(&draft(&config, strategy, values))
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{strategy}: {err}");
    }
}

#[test]
fn an_unknown_strategy_or_an_out_of_range_value_is_rejected() {
    let config = test_config();
    let err = update_campaign(&update(&config, "MAXIMISE_CLICKS", NONE))
        .unwrap_err()
        .to_string();
    assert!(err.contains("Unknown bidding strategy"), "{err}");

    let values = (
        None,
        None,
        None,
        Some(TargetImpressionShareLocation::TopOfPage),
        Some(120.0),
    );
    let err = draft_campaign(&draft(&config, "TARGET_IMPRESSION_SHARE", values))
        .unwrap_err()
        .to_string();
    assert!(err.contains("(0, 100]"), "{err}");

    let err = draft_campaign(&draft(
        &config,
        "TARGET_CPA",
        (Some(0.0), None, None, None, None),
    ))
    .unwrap_err()
    .to_string();
    assert!(err.contains("greater than 0"), "{err}");
}
