//! A dry run of `confirm_and_apply` has Google validate the plan without
//! applying it (`validateOnly: true` on `googleAds:mutate`).
//!
//! A dry run used to re-display the draft with no HTTP call, so nothing Google
//! would refuse (a field, a mask, a value, a business or policy rule) surfaced
//! before the real apply — possibly after other plans were already applied.
//! Asserts the request sent, that a rejection comes back as Google's error, and
//! that the plan stays available for the real apply in both cases.

mod common;

use common::test_config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::set_campaign_network_settings;
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn draft(config: &mcp_google_ads::config::Config) -> String {
    let preview =
        set_campaign_network_settings(config, "1234567890", "777", None, Some(true), None).unwrap();
    preview["plan_id"].as_str().unwrap().to_string()
}

fn dry_run(plan_id: &str) -> ConfirmApplyInput {
    ConfirmApplyInput {
        plan_id: plan_id.to_string(),
        dry_run: true,
        ..Default::default()
    }
}

/// One test, so the base URL set for the mock server is never raced by another.
#[tokio::test]
async fn a_dry_run_is_validated_by_google_and_applies_nothing() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .and(body_partial_json(serde_json::json!({"validateOnly": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .up_to_n_times(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .and(body_partial_json(serde_json::json!({"validateOnly": true})))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "code": 400,
                "message": "Request contains an invalid argument.",
                "status": "INVALID_ARGUMENT",
                "details": [{
                    "@type": "type.googleapis.com/google.ads.googleads.v25.errors.GoogleAdsFailure",
                    "errors": [{
                        "errorCode": {"fieldError": "IMMUTABLE_FIELD"},
                        "message": "The field cannot be modified."
                    }]
                }]
            }
        })))
        .mount(&mock)
        .await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));
    let config = test_config();

    // Accepted: validated, nothing applied, the plan stays.
    let accepted = draft(&config);
    let preview = confirm_and_apply(&config, dry_run(&accepted))
        .await
        .unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["validation"], "VALIDATED_BY_GOOGLE");
    assert!(get_plan(&accepted).is_some());

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["validateOnly"], true);
    assert_eq!(body["partialFailure"], false);
    assert_eq!(
        body["mutateOperations"][0]["campaignOperation"]["updateMask"],
        "networkSettings.targetSearchNetwork"
    );

    // Rejected: Google's error comes back, the plan stays.
    let rejected = draft(&config);
    // What the MCP tool returns to the agent: the error envelope, with Google's details.
    let err = confirm_and_apply(&config, dry_run(&rejected))
        .await
        .unwrap_err()
        .to_json()
        .to_string();
    assert!(err.contains("IMMUTABLE_FIELD"), "{err}");
    assert!(get_plan(&rejected).is_some());

    // A real apply carries no validateOnly.
    let apply = Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"mutateOperationResponses": []})),
        );
    mock.reset().await;
    apply.mount(&mock).await;
    confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: accepted.clone(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply succeeds");
    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("validateOnly").is_none(), "{body}");
    assert!(get_plan(&accepted).is_none());
}
