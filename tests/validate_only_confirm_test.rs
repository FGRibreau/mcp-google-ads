//! `confirm_and_apply(validate_only = true)` is how new tools get tested
//! against a live account without changing it: the plan goes to Google with
//! `validateOnly: true`, Google checks every operation, nothing is written,
//! and the plan stays so it can be applied for real afterwards.
//!
//! Also covers the custom audience route (`customAudiences:mutate`), which
//! custom segments need because they are not a `googleAds:mutate` operation.
//!
//! Each test builds its own client against its own mock server, so the tests
//! can run in parallel without the process-wide base URL override.

mod common;

use mcp_google_ads::client::GoogleAdsClient;
use mcp_google_ads::config::Config;
use mcp_google_ads::error::McpGoogleAdsError;
use mcp_google_ads::safety::preview::{
    get_plan, remove_plan, store_plan, ChangePlan, PlanDispatch,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply_with_client, ConfirmApplyInput};
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mock_and_client() -> (MockServer, GoogleAdsClient) {
    common::spawn_mock_google_ads().await
}

fn mutate_plan(requires_double_confirm: bool) -> ChangePlan {
    ChangePlan::new(
        "add_ad_group_placements".into(),
        "ad_group_criterion".into(),
        "111".into(),
        "1234567890".into(),
        json!({"placement_urls": ["news.example"]}),
        requires_double_confirm,
        vec![json!({
            "adGroupCriterionOperation": {
                "create": {
                    "adGroup": "customers/1234567890/adGroups/111",
                    "placement": {"url": "news.example"}
                }
            }
        })],
    )
}

fn custom_audience_plan() -> ChangePlan {
    ChangePlan::new(
        "create_custom_segment".into(),
        "custom_audience".into(),
        "new".into(),
        "1234567890".into(),
        json!({"name": "S"}),
        false,
        vec![json!({
            "create": {
                "name": "S",
                "type": "AUTO",
                "members": [{"memberType": "KEYWORD", "keyword": "running shoes"}]
            }
        })],
    )
    .with_dispatch(PlanDispatch::CustomAudiences)
}

/// The strictest configuration: dry runs required.
fn guarded_config() -> Config {
    let mut config = common::test_config();
    config.safety.require_dry_run = true;
    config
}

fn validate(plan_id: &str) -> ConfirmApplyInput {
    ConfirmApplyInput {
        plan_id: plan_id.to_string(),
        // The MCP default: validate_only must win over it.
        dry_run: true,
        validate_only: true,
        ..Default::default()
    }
}

#[tokio::test]
async fn validate_only_sends_validate_only_and_keeps_the_plan() {
    let (mock, client) = mock_and_client().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&mock)
        .await;

    // Destructive-flagged and under require_dry_run: neither guard applies,
    // because nothing is written.
    let plan = mutate_plan(true);
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let result = confirm_and_apply_with_client(&client, &guarded_config(), validate(&plan_id))
        .await
        .expect("validated");

    assert_eq!(result["status"], "VALIDATED");
    assert_eq!(result["validated_by_google"], true);
    assert_eq!(result["applied"], false);
    assert_eq!(result["plan_id"], plan_id.as_str());
    assert!(result["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Nothing was changed"));
    assert!(get_plan(&plan_id).is_some(), "plan kept for the real apply");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["validateOnly"], true);
    assert_eq!(body["partialFailure"], false);
    assert_eq!(
        body["mutateOperations"][0]["adGroupCriterionOperation"]["create"]["placement"]["url"],
        "news.example"
    );
    remove_plan(&plan_id);
}

#[tokio::test]
async fn a_rejected_validation_is_an_error_and_the_plan_survives() {
    let (mock, client) = mock_and_client().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "code": 400,
                "message": "Request contains an invalid argument.",
                "status": "INVALID_ARGUMENT",
                "details": [{
                    "@type": "type.googleapis.com/google.ads.googleads.v25.errors.GoogleAdsFailure",
                    "errors": [{
                        "errorCode": {"criterionError": "INVALID_PLACEMENT_URL"},
                        "message": "The placement URL is invalid."
                    }],
                    "requestId": "abc123"
                }]
            }
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let plan = mutate_plan(false);
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let err = confirm_and_apply_with_client(&client, &guarded_config(), validate(&plan_id))
        .await
        .expect_err("Google rejected it");
    assert!(matches!(err, McpGoogleAdsError::GoogleAds { .. }));
    assert!(err
        .api_error_messages()
        .iter()
        .any(|m| m.contains("INVALID_PLACEMENT_URL")));
    assert!(get_plan(&plan_id).is_some());
    remove_plan(&plan_id);
}

#[tokio::test]
async fn a_real_apply_carries_no_validate_only_flag() {
    let (mock, client) = mock_and_client().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": [
                {"adGroupCriterionResult": {"resourceName": "customers/1234567890/adGroupCriteria/111~5"}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let plan = mutate_plan(false);
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.require_dry_run = false;
    let result = confirm_and_apply_with_client(
        &client,
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("applied");
    assert_eq!(result["status"], "APPLIED");
    assert!(get_plan(&plan_id).is_none(), "applied plans are removed");

    let body: serde_json::Value = mock.received_requests().await.unwrap()[0]
        .body_json()
        .unwrap();
    assert!(body.get("validateOnly").is_none());
}

#[tokio::test]
async fn custom_segment_plans_validate_and_apply_through_custom_audiences_mutate() {
    let (mock, client) = mock_and_client().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/customAudiences:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"resourceName": "customers/1234567890/customAudiences/987654"}]
        })))
        .expect(2)
        .mount(&mock)
        .await;

    let plan = custom_audience_plan();
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let validated = confirm_and_apply_with_client(&client, &guarded_config(), validate(&plan_id))
        .await
        .expect("validated");
    assert_eq!(validated["status"], "VALIDATED");
    assert!(get_plan(&plan_id).is_some());

    let mut config = common::test_config();
    config.safety.require_dry_run = false;
    let applied = confirm_and_apply_with_client(
        &client,
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("applied");
    assert_eq!(applied["status"], "APPLIED");
    assert_eq!(applied["custom_segment_ids"], json!(["987654"]));
    assert_eq!(applied["operation"], "create_custom_segment");

    let requests = mock.received_requests().await.unwrap();
    let first: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(first["validateOnly"], true);
    assert_eq!(first["operations"][0]["create"]["name"], "S");
    let second: serde_json::Value = requests[1].body_json().unwrap();
    assert!(second.get("validateOnly").is_none());
    assert!(
        second.get("mutateOperations").is_none(),
        "custom audiences are not googleAds:mutate operations"
    );
}

#[tokio::test]
async fn recommendation_plans_say_they_were_not_validated_and_send_nothing() {
    let (mock, client) = mock_and_client().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let plan = ChangePlan::new(
        "apply_recommendation".into(),
        "recommendation".into(),
        "rec-1".into(),
        "1234567890".into(),
        json!({}),
        false,
        vec![],
    )
    .with_dispatch(PlanDispatch::ApplyRecommendation {
        resource_names: vec!["customers/1234567890/recommendations/rec-1".into()],
        apply_parameters: None,
    });
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let result = confirm_and_apply_with_client(&client, &guarded_config(), validate(&plan_id))
        .await
        .expect("answered");
    assert_eq!(result["status"], "NOT_VALIDATED");
    assert_eq!(result["validated_by_google"], false);
    assert_eq!(result["applied"], false);
    assert!(get_plan(&plan_id).is_some());
    remove_plan(&plan_id);
}

#[tokio::test]
async fn without_validate_only_the_guards_still_hold() {
    let (mock, client) = mock_and_client().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let plan = mutate_plan(false);
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let err = confirm_and_apply_with_client(
        &client,
        &guarded_config(),
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect_err("require_dry_run");
    assert!(matches!(err, McpGoogleAdsError::DryRunRequired));
    remove_plan(&plan_id);
}
