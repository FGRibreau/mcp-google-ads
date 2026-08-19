//! End-to-end coverage for negative keyword lists (shared sets), through the
//! real apply path against a stubbed Google Ads API.
//!
//! The unit tests in `shared_sets_write` assert plan *shape*. These assert what
//! actually reaches the wire — specifically that the whole list→keywords→campaign
//! chain travels as ONE atomic mutate. If it were split across requests, a
//! failure halfway through would leave a list attached to campaigns while
//! holding none of its keywords: campaigns that look protected and are not.

mod common;

use common::test_config;
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use mcp_google_ads::tools::shared_sets_write::{
    create_negative_keyword_list, detach_negative_keyword_list,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Stub `googleAds:mutate` and return the server (kept alive by the caller).
async fn mock_mutate_ok() -> MockServer {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v23/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "mutateOperationResponses": [
                {"sharedSetResult": {"resourceName": "customers/1234567890/sharedSets/555"}}
            ]
        })))
        .mount(&mock)
        .await;
    mock
}

/// The single request body received by the mock.
async fn only_request_body(mock: &MockServer) -> serde_json::Value {
    let requests = mock
        .received_requests()
        .await
        .expect("mock recorded requests");
    assert_eq!(
        requests.len(),
        1,
        "the whole chain must travel as one atomic mutate, got {} requests",
        requests.len()
    );
    serde_json::from_slice(&requests[0].body).expect("request body is JSON")
}

#[tokio::test]
async fn create_list_applies_as_one_atomic_mutate() {
    let mock = mock_mutate_ok().await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v23", mock.uri()));

    let config = test_config();
    let preview = create_negative_keyword_list(
        &config,
        "1234567890",
        "NEG - Genericas Clinica",
        vec!["gratis".to_string(), "sus".to_string(), "ubs".to_string()],
        "PHRASE",
        &["111".to_string(), "222".to_string()],
    )
    .expect("plan builds");

    let plan_id = preview["plan_id"]
        .as_str()
        .expect("plan_id present")
        .to_string();

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id,
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply succeeds");
    assert_eq!(result["status"], "APPLIED");

    let body = only_request_body(&mock).await;

    // partialFailure must stay false: a half-committed list is worse than none.
    assert_eq!(body["partialFailure"], serde_json::json!(false));

    let ops = body["mutateOperations"]
        .as_array()
        .expect("mutateOperations array");
    assert_eq!(ops.len(), 6, "1 set + 3 keywords + 2 campaign links");

    let temp = "customers/1234567890/sharedSets/-1";
    assert_eq!(
        ops[0].pointer("/sharedSetOperation/create/type"),
        Some(&serde_json::json!("NEGATIVE_KEYWORDS"))
    );
    assert_eq!(
        ops[0].pointer("/sharedSetOperation/create/resourceName"),
        Some(&serde_json::json!(temp))
    );

    // The set must be created before anything references its temp ID.
    let first_reference = ops
        .iter()
        .position(|op| {
            op.pointer("/sharedCriterionOperation/create/sharedSet")
                == Some(&serde_json::json!(temp))
        })
        .expect("at least one criterion references the temp set");
    assert!(
        first_reference > 0,
        "temp resource must be created before it is referenced"
    );

    let keywords: Vec<&str> = ops
        .iter()
        .filter_map(|op| {
            op.pointer("/sharedCriterionOperation/create/keyword/text")
                .and_then(|v| v.as_str())
        })
        .collect();
    assert_eq!(keywords, vec!["gratis", "sus", "ubs"]);

    let links: Vec<&str> = ops
        .iter()
        .filter_map(|op| {
            op.pointer("/campaignSharedSetOperation/create/campaign")
                .and_then(|v| v.as_str())
        })
        .collect();
    assert_eq!(
        links,
        vec![
            "customers/1234567890/campaigns/111",
            "customers/1234567890/campaigns/222"
        ]
    );

    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
}

#[tokio::test]
async fn detach_is_blocked_without_double_confirmation() {
    let config = test_config();
    let preview = detach_negative_keyword_list(&config, "1234567890", "555", &["111".to_string()])
        .expect("plan builds");
    let plan_id = preview["plan_id"]
        .as_str()
        .expect("plan_id present")
        .to_string();

    // No mock is mounted: if the guard leaked, the apply would attempt real HTTP.
    let err = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id,
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect_err("detach strips live exclusions — must require confirmed_twice");
    assert!(err.to_string().to_lowercase().contains("confirm"));
}
