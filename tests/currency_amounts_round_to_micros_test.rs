//! Amounts given in account currency reach Google as exact micros.
//!
//! A truncating `as i64` cast turned 20.01 into 20_009_999 micros, which Google
//! rejects for not being a multiple of the currency's billable unit. These tests
//! pin the rounding on what actually reaches the wire, and the ad group tools'
//! currency `cpc_bid`, resolved to micros before the draft is built.

mod common;

use common::test_config;
use mcp_google_ads::models::resolve_micros;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ad_groups_write::{create_ad_group, update_ad_group};
use mcp_google_ads::tools::campaigns_write::{draft_campaign, DraftCampaignParams};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn budget_and_target_cpa_reach_google_as_exact_micros() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "mutateOperationResponses": [
                {"campaignBudgetResult": {"resourceName": "customers/1234567890/campaignBudgets/1"}},
                {"campaignResult": {"resourceName": "customers/1234567890/campaigns/2"}},
                {"adGroupResult": {"resourceName": "customers/1234567890/adGroups/3"}}
            ]
        })))
        .mount(&mock)
        .await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let config = test_config();
    let preview = draft_campaign(&DraftCampaignParams {
        config: &config,
        customer_id: "1234567890",
        campaign_name: "Rounding",
        daily_budget: 20.01,
        bidding_strategy: "MAXIMIZE_CONVERSIONS",
        target_cpa: Some(12.29),
        target_roas: None,
        channel_type: "SEARCH",
        ad_group_name: "AG1",
        keywords: vec![],
        geo_target_ids: vec![],
        language_ids: vec![],
        start_date_time: None,
        end_date_time: None,
        cpc_bid_ceiling: None,
        target_impression_share_location: None,
        target_impression_share_percent: None,
        status: None,
    })
    .expect("plan builds");

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
    let operations = body["mutateOperations"].as_array().unwrap();
    assert_eq!(
        operations[0]["campaignBudgetOperation"]["create"]["amountMicros"],
        "20010000"
    );
    let campaign = &operations[1]["campaignOperation"]["create"];
    assert_eq!(
        campaign["maximizeConversions"]["targetCpaMicros"],
        "12290000"
    );
}

#[test]
fn ad_group_cpc_bid_in_currency_becomes_exact_micros() {
    let config = test_config();
    let micros = resolve_micros(Some(2.01), None, "cpc_bid").unwrap();
    let preview = create_ad_group(&config, "1234567890", "999", "AG", micros, None).unwrap();

    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        plan.mutate_operations[0]["adGroupOperation"]["create"]["cpcBidMicros"],
        "2010000"
    );

    let micros = resolve_micros(Some(0.29), None, "cpc_bid").unwrap();
    let preview = update_ad_group(&config, "1234567890", "555", None, micros, None).unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        plan.mutate_operations[0]["adGroupOperation"]["update"]["cpcBidMicros"],
        "290000"
    );
}

#[test]
fn ad_group_cpc_bid_in_both_units_is_rejected() {
    let err = resolve_micros(Some(1.5), Some(1_500_000), "cpc_bid")
        .unwrap_err()
        .to_string();
    assert!(err.contains("not both"), "{err}");
}
