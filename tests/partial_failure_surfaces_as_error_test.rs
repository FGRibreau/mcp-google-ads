//! Issue #5, bug 2: when Google returns HTTP 200 with a `partialFailureError`
//! in the body (we request `partialFailure: true`), the apply must be reported
//! as a FAILURE — not a false "APPLIED"/"SUCCESS" — and the plan kept for retry.

mod common;

use mcp_google_ads::safety::preview::{get_plan, remove_plan, store_plan, ChangePlan};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use serde_json::json;
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn partial_failure_error_is_reported_as_failure_and_plan_retained() {
    let mock = wiremock::MockServer::start().await;
    // HTTP 200, but the body carries a partialFailureError (gRPC code 3).
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": [],
            "partialFailureError": {
                "code": 3,
                "message": "The required field was not present.",
                "details": []
            }
        })))
        .expect(1)
        .mount(&mock)
        .await;

    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let log_file = PathBuf::from("/tmp/__test_audit_partial_failure__.log");
    let _ = std::fs::remove_file(&log_file);

    let plan = ChangePlan::new(
        "update_campaign".into(),
        "campaign".into(),
        "12345".into(),
        "1234567890".into(),
        json!({ "daily_budget": 50.0 }),
        false,
        vec![json!({
            "campaignBudgetOperation": {
                "update": {
                    "resourceName": "customers/1234567890/campaignBudgets/987654321",
                    "amountMicros": "50000000"
                },
                "updateMask": "amountMicros"
            }
        })],
    );
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.log_file = log_file.clone();

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: true,
            confirmed_twice: false,
        },
    )
    .await;

    // (a) The apply must surface as an error, never a false "APPLIED".
    assert!(
        result.is_err(),
        "a partialFailureError must be reported as a failure, got: {:?}",
        result
    );

    // (b) The audit log must record FAILED, not SUCCESS.
    let log_contents = std::fs::read_to_string(&log_file).expect("audit log written");
    assert!(
        log_contents.contains("\"result\":\"FAILED\""),
        "audit log must record FAILED, got: {log_contents}"
    );
    assert!(
        !log_contents.contains("\"result\":\"SUCCESS\""),
        "audit log must NOT record SUCCESS on partial failure"
    );

    // (c) The plan must be retained so the user can fix and retry.
    assert!(
        get_plan(&plan_id).is_some(),
        "the plan must be kept in the store after a failed apply"
    );

    // Cleanup
    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
    let _ = std::fs::remove_file(&log_file);
    remove_plan(&plan_id);
}
