//! A Search campaign is created with the networks the caller chooses and, on
//! request, without the default ad group; its networks change afterwards with
//! `set_campaign_network_settings`.
//!
//! `draft_campaign` used to impose Google Search only and always create a
//! default ad group: a caller that builds its ad groups itself got a stray one.
//! Asserts what reaches the wire, the update mask, and each rejection.

mod common;

use common::test_config;
use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::{
    draft_campaign, set_campaign_network_settings, DraftCampaignParams, KeywordInput,
    NetworkSettingsInput,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn draft<'a>(
    config: &'a Config,
    channel_type: &'a str,
    network_settings: Option<NetworkSettingsInput>,
    with_ad_group: bool,
) -> DraftCampaignParams<'a> {
    DraftCampaignParams {
        config,
        customer_id: "1234567890",
        campaign_name: "Search",
        daily_budget: 20.0,
        bidding_strategy: "MAXIMIZE_CONVERSIONS",
        target_cpa: None,
        target_roas: None,
        channel_type,
        ad_group_name: "AG1",
        keywords: vec![],
        geo_target_ids: vec!["2250".to_string()],
        language_ids: vec!["1002".to_string()],
        status: None,
        start_date_time: None,
        end_date_time: None,
        cpc_bid_ceiling: None,
        target_impression_share_location: None,
        target_impression_share_percent: None,
        network_settings,
        with_ad_group,
    }
}

const PARTNERS: NetworkSettingsInput = NetworkSettingsInput {
    target_google_search: true,
    target_search_network: true,
    target_content_network: false,
};

#[tokio::test]
async fn a_search_campaign_is_created_alone_with_its_networks() {
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
    let preview = draft_campaign(&draft(&config, "SEARCH", Some(PARTNERS), false)).unwrap();
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
    let ops = body["mutateOperations"].as_array().unwrap();
    let networks = &ops[1]["campaignOperation"]["create"]["networkSettings"];
    assert_eq!(networks["targetGoogleSearch"], true);
    assert_eq!(networks["targetSearchNetwork"], true);
    assert_eq!(networks["targetContentNetwork"], false);
    assert!(
        ops.iter().all(|op| op.get("adGroupOperation").is_none()),
        "no default ad group: {ops:?}"
    );
    // Budget, campaign, location and language: the criteria stay.
    assert_eq!(ops.len(), 4);
}

#[test]
fn by_default_the_campaign_keeps_its_ad_group_and_google_search_only() {
    let config = test_config();
    let preview = draft_campaign(&draft(&config, "SEARCH", None, true)).unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    let networks = &plan.mutate_operations[1]["campaignOperation"]["create"]["networkSettings"];
    assert_eq!(networks["targetGoogleSearch"], true);
    assert_eq!(networks["targetSearchNetwork"], false);
    assert!(plan
        .mutate_operations
        .iter()
        .any(|op| op.get("adGroupOperation").is_some()));
}

#[test]
fn networks_change_one_by_one() {
    let config = test_config();
    let preview =
        set_campaign_network_settings(&config, "1234567890", "777", None, Some(false), Some(true))
            .unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    let op = &plan.mutate_operations[0]["campaignOperation"];
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/campaigns/777"
    );
    assert_eq!(
        op["update"]["networkSettings"]["targetSearchNetwork"],
        false
    );
    assert_eq!(
        op["update"]["networkSettings"]["targetContentNetwork"],
        true
    );
    assert!(op["update"]["networkSettings"]
        .get("targetGoogleSearch")
        .is_none());
    assert_eq!(
        op["updateMask"],
        "networkSettings.targetSearchNetwork,networkSettings.targetContentNetwork"
    );
}

#[test]
fn an_inconsistent_request_is_rejected() {
    let config = test_config();
    let err = draft_campaign(&draft(&config, "DISPLAY", Some(PARTNERS), true))
        .unwrap_err()
        .to_string();
    assert!(err.contains("only apply to a SEARCH campaign"), "{err}");

    let mut alone_with_keywords = draft(&config, "SEARCH", None, false);
    alone_with_keywords.keywords = vec![KeywordInput {
        text: "hiking shoes".to_string(),
        match_type: "PHRASE".to_string(),
        final_url: None,
    }];
    let err = draft_campaign(&alone_with_keywords)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("keywords belong to the default ad group"),
        "{err}"
    );

    let err = set_campaign_network_settings(&config, "1234567890", "777", None, None, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("No network specified"), "{err}");
}
