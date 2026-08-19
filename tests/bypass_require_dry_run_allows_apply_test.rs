//! With `bypass_require_dry_run = true`, the guard is overridden for THIS
//! single apply — the config setting itself is not mutated.

mod common;

use mcp_google_ads::safety::preview::{remove_plan, store_plan, ChangePlan};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn bypass_require_dry_run_lets_apply_proceed() {
    let mock = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": [
                {"adGroupOperation": {"resourceName": "customers/1234567890/adGroups/9"}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    // Point the client at the mock server via the env override.
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let plan = ChangePlan::new(
        "test_op".into(),
        "ad_group".into(),
        "9".into(),
        "1234567890".into(),
        json!({}),
        false,
        vec![json!({"adGroupOperation": {"create": {"name": "x"}}})],
    );
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.require_dry_run = true; // guard ON

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: true, // explicit opt-out
            confirmed_twice: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply should proceed when bypass=true");

    assert_eq!(result["status"], "APPLIED");

    // Cleanup
    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
    remove_plan(&plan_id);
}
