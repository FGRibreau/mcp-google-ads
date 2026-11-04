//! A campaign's start and end of delivery (`Campaign.start_date_time` /
//! `end_date_time`, Google Ads v23+) are written at creation and on update.
//!
//! Asserts what actually reaches the wire on creation, and that an update sends
//! the dates and the bidding strategy as ONE campaign update with ONE mask:
//! two updates of the same campaign in one mutate would be redundant at best.

mod common;

use common::test_config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::{
    draft_campaign, update_campaign, DraftCampaignParams, UpdateCampaignParams,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn draft<'a>(
    config: &'a mcp_google_ads::config::Config,
    start: Option<&'a str>,
    end: Option<&'a str>,
) -> DraftCampaignParams<'a> {
    DraftCampaignParams {
        config,
        customer_id: "1234567890",
        campaign_name: "Dated",
        daily_budget: 20.0,
        bidding_strategy: "MAXIMIZE_CONVERSIONS",
        target_cpa: None,
        target_roas: None,
        channel_type: "SEARCH",
        ad_group_name: "AG1",
        keywords: vec![],
        geo_target_ids: vec![],
        language_ids: vec![],
        status: None,
        start_date_time: start,
        end_date_time: end,
    }
}

fn update<'a>(
    config: &'a mcp_google_ads::config::Config,
    bidding_strategy: Option<&'a str>,
    start: Option<&'a str>,
    end: Option<&'a str>,
) -> UpdateCampaignParams<'a> {
    UpdateCampaignParams {
        config,
        customer_id: "1234567890",
        campaign_id: "777",
        bidding_strategy,
        target_cpa: None,
        target_roas: None,
        daily_budget: None,
        budget_resource_name: None,
        geo_target_ids: vec![],
        language_ids: vec![],
        start_date_time: start,
        end_date_time: end,
    }
}

#[tokio::test]
async fn creation_sends_the_start_and_end_date_times() {
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
    let preview = draft_campaign(&draft(
        &config,
        Some("2026-11-10 00:00:00"),
        Some("2026-12-31 23:59:59"),
    ))
    .expect("plan builds");
    assert_eq!(preview["changes"]["start_date_time"], "2026-11-10 00:00:00");

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
    let campaign = &body["mutateOperations"][1]["campaignOperation"]["create"];
    assert_eq!(campaign["startDateTime"], "2026-11-10 00:00:00");
    assert_eq!(campaign["endDateTime"], "2026-12-31 23:59:59");
}

#[test]
fn creation_without_dates_leaves_them_to_google() {
    let config = test_config();
    let preview = draft_campaign(&draft(&config, None, None)).unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();
    let campaign = &plan.mutate_operations[1]["campaignOperation"]["create"];
    assert!(campaign.get("startDateTime").is_none());
    assert!(campaign.get("endDateTime").is_none());
}

#[test]
fn an_update_of_the_dates_alone_masks_only_the_dates() {
    let config = test_config();
    let preview =
        update_campaign(&update(&config, None, None, Some("2027-01-31 23:59:59"))).unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    assert_eq!(plan.mutate_operations.len(), 1);
    let op = &plan.mutate_operations[0]["campaignOperation"];
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/campaigns/777"
    );
    assert_eq!(op["update"]["endDateTime"], "2027-01-31 23:59:59");
    assert_eq!(op["updateMask"], "endDateTime");
}

#[test]
fn dates_and_bidding_strategy_travel_as_one_campaign_update() {
    let config = test_config();
    let preview = update_campaign(&update(
        &config,
        Some("MAXIMIZE_CONVERSIONS"),
        Some("2026-11-10 00:00:00"),
        Some("2026-12-31 23:59:59"),
    ))
    .unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    assert_eq!(plan.mutate_operations.len(), 1);
    let op = &plan.mutate_operations[0]["campaignOperation"];
    assert_eq!(
        op["updateMask"],
        "maximizeConversions.targetCpaMicros,startDateTime,endDateTime"
    );
}

#[test]
fn a_date_time_in_another_format_is_rejected() {
    let config = test_config();
    for bad in [
        "2026-11-10",
        "2026-11-10T00:00:00+01:00",
        "10/11/2026 00:00:00",
    ] {
        let err = draft_campaign(&draft(&config, Some(bad), None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("yyyy-MM-dd HH:mm:ss"), "{bad}: {err}");
    }
}

#[test]
fn an_end_before_the_start_is_rejected() {
    let config = test_config();
    let err = update_campaign(&update(
        &config,
        None,
        Some("2026-12-31 00:00:00"),
        Some("2026-11-10 00:00:00"),
    ))
    .unwrap_err()
    .to_string();
    assert!(err.contains("after start_date_time"), "{err}");
}
