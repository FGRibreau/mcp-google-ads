//! `requires_double_confirm` is wired as a hard guard:
//! - `confirmed_twice = false` -> Err(DoubleConfirmRequired)
//! - `confirmed_twice = true` -> apply proceeds

mod common;

use mcp_google_ads::error::McpGoogleAdsError;
use mcp_google_ads::safety::preview::{remove_plan, store_plan, ChangePlan};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, ResponseTemplate};

fn make_destructive_plan() -> ChangePlan {
    ChangePlan::new(
        "remove_entity".into(),
        "campaign".into(),
        "999".into(),
        "1234567890".into(),
        json!({}),
        true, // requires_double_confirm
        vec![json!({"campaignOperation": {"remove": "customers/1234567890/campaigns/999"}})],
    )
}

#[tokio::test]
async fn confirmed_twice_false_blocks_apply() {
    // Mock server with ZERO expected requests — the guard must short-circuit.
    let mock = wiremock::MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let plan = make_destructive_plan();
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.require_dry_run = false; // isolate the double-confirm guard

    let err = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: false,
            confirmed_twice: false,
            ..Default::default()
        },
    )
    .await
    .expect_err("must require double-confirm");

    assert!(matches!(err, McpGoogleAdsError::DoubleConfirmRequired));
    remove_plan(&plan_id);
}

#[tokio::test]
async fn confirmed_twice_true_lets_apply_proceed() {
    let mock = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v23/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": [{"campaignOperation": {"resourceName": "customers/1234567890/campaigns/999"}}]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v23", mock.uri()));

    let plan = make_destructive_plan();
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.require_dry_run = false;

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: false,
            confirmed_twice: true,
            ..Default::default()
        },
    )
    .await
    .expect("apply should proceed with confirmed_twice=true");

    assert_eq!(result["status"], "APPLIED");

    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
    remove_plan(&plan_id);
}
