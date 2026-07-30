//! Issue #5, bug 1: a `daily_budget` update must target the campaign's real
//! budget resource (resolved via the API), NOT a resource name built by reusing
//! the campaign ID — campaign budgets have their own distinct IDs.

mod common;

use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::{
    resolve_campaign_budget_resource, update_campaign, UpdateCampaignParams,
};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn budget_update_targets_resolved_budget_resource_not_campaign_id() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    // The campaign's budget has its own ID (987654321), different from the
    // campaign ID (12345).
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "campaign": { "campaignBudget": "customers/1234567890/campaignBudgets/987654321" } }
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let resolved = resolve_campaign_budget_resource(&client, "1234567890", "12345")
        .await
        .expect("budget resource resolves from the API");
    assert_eq!(resolved, "customers/1234567890/campaignBudgets/987654321");

    let config = common::test_config();
    let preview = update_campaign(&UpdateCampaignParams {
        config: &config,
        customer_id: "1234567890",
        campaign_id: "12345",
        bidding_strategy: None,
        target_cpa: None,
        target_roas: None,
        daily_budget: Some(50.0),
        budget_resource_name: Some(&resolved),
        geo_target_ids: vec![],
        language_ids: vec![],
    })
    .expect("update_campaign builds a plan");

    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");

    let budget_op = plan
        .mutate_operations
        .iter()
        .find(|op| op.get("campaignBudgetOperation").is_some())
        .expect("a campaignBudgetOperation is present");
    let resource_name = budget_op["campaignBudgetOperation"]["update"]["resourceName"]
        .as_str()
        .unwrap_or_default();

    assert_eq!(
        resource_name, "customers/1234567890/campaignBudgets/987654321",
        "budget update must point at the resolved budget resource"
    );
    assert_ne!(
        resource_name, "customers/1234567890/campaignBudgets/12345",
        "budget update must NOT reuse the campaign ID as the budget ID (issue #5)"
    );
}

#[tokio::test]
async fn budget_resolution_errors_when_campaign_has_no_budget() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    let result = resolve_campaign_budget_resource(&client, "1234567890", "99999").await;
    assert!(
        result.is_err(),
        "resolution must fail loudly when no budget is found"
    );
}
