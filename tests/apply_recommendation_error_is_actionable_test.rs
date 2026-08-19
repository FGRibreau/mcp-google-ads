//! When `recommendations:apply` fails, the caller must be told enough to act.
//!
//! The body replayed here is the verbatim `partialFailureError` Google
//! returned for a real SITELINK_ASSET apply. Everything that makes it
//! diagnosable — the request id to quote to support, the operation index that
//! failed, and the fact that the error code itself is newer than the API
//! version we negotiated — lives in `details`, which the `Display` impl drops.
//! Left unextracted, the caller sees only "The error code is not in this
//! version." and has nowhere to go.

mod common;

use mcp_google_ads::safety::preview::{store_plan, ChangePlan, PlanDispatch};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use serde_json::json;
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn unknown_error_code_is_explained_not_parroted() {
    let mock = wiremock::MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "partialFailureError": {
                "code": 3,
                "message": "The error code is not in this version., at operations[0]",
                "details": [{
                    "@type": "type.googleapis.com/google.ads.googleads.v25.errors.GoogleAdsFailure",
                    "errors": [{
                        "details": {},
                        "errorCode": { "requestError": "UNKNOWN" },
                        "location": {
                            "fieldPathElements": [{ "fieldName": "operations", "index": 0 }]
                        },
                        "message": "The error code is not in this version."
                    }],
                    "requestId": "l0wUVTeh5ol-vXyp5n4iiA"
                }]
            }
        })))
        .expect(1)
        .mount(&mock)
        .await;

    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let log_file = PathBuf::from("/tmp/__test_audit_apply_rec_actionable__.log");
    let _ = std::fs::remove_file(&log_file);

    let plan = ChangePlan::new(
        "apply_recommendation".into(),
        "recommendation".into(),
        "REC1".into(),
        "1234567890".into(),
        json!({ "action": "APPLY" }),
        false,
        Vec::new(),
    )
    .with_dispatch(PlanDispatch::ApplyRecommendation {
        resource_names: vec!["customers/1234567890/recommendations/REC1".to_string()],
        apply_parameters: None,
    });
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.log_file = log_file.clone();

    let err = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id,
            dry_run: false,
            bypass_require_dry_run: true,
            confirmed_twice: false,
            ..Default::default()
        },
    )
    .await
    .expect_err("a partialFailureError must surface as an error");

    let payload = err.to_json();

    // Assert on the EXTRACTED field, not on the whole payload: the raw dump
    // already contains every string below, so asserting on `payload.to_string()`
    // would pass even with no extraction at all.
    let api_errors = payload["api_errors"]
        .as_array()
        .unwrap_or_else(|| panic!("partial failures must surface api_errors, got: {payload}"));
    assert_eq!(
        api_errors.len(),
        1,
        "one failing operation means one surfaced error, got: {api_errors:?}"
    );
    let surfaced = api_errors[0].as_str().unwrap_or_default().to_string();

    // The request id is the only handle Google support accepts. Losing it
    // means the failure cannot be escalated at all.
    assert!(
        surfaced.contains("l0wUVTeh5ol-vXyp5n4iiA"),
        "the request id must reach the caller, got: {surfaced}"
    );

    // Which operation failed. In a batch apply, "it failed" is useless
    // without the index.
    assert!(
        surfaced.contains("operations[0]"),
        "the failing field path must be surfaced, got: {surfaced}"
    );

    // The whole point: say WHY the code is unreadable, instead of repeating
    // Google's own dead-end sentence back at the caller.
    assert!(
        surfaced.to_lowercase().contains("api version"),
        "an unknown error code must be explained as a version mismatch, got: {surfaced}"
    );
}
