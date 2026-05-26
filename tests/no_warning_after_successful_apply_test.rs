//! A successful apply must NOT carry a `warnings` field. v0.2.x emitted a
//! cosmetic warning that lied about safety: it said "consider dry_run=true"
//! AFTER the mutation already happened.

mod common;

use mcp_google_ads::safety::preview::{remove_plan, store_plan, ChangePlan};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn successful_apply_has_no_warnings_field() {
    let mock = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v23/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v23", mock.uri()));

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
    // require_dry_run=true with bypass=true — this is the SCENARIO that v0.2.x
    // would have triggered the bogus warning on. Now it must be silent.
    config.safety.require_dry_run = true;

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: true,
            confirmed_twice: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(result["status"], "APPLIED");
    assert!(
        result.get("warnings").is_none(),
        "successful apply must not carry a warnings field — got: {}",
        result
    );

    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
    remove_plan(&plan_id);
}
