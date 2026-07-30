mod common;

use mcp_google_ads::client::{GoogleAdsClient, MutateOperation};
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REQUEST_DUMP: &str = "/tmp/mcp-google-ads-last-request.json";
const ERROR_DUMP: &str = "/tmp/mcp-google-ads-last-error.json";

fn remove_fixed_debug_files() {
    let _ = std::fs::remove_file(REQUEST_DUMP);
    let _ = std::fs::remove_file(ERROR_DUMP);
}

fn campaign_operation() -> Vec<MutateOperation> {
    vec![MutateOperation {
        operation: json!({
            "campaignOperation": {
                "create": {
                    "name": "non-secret-test-campaign"
                }
            }
        }),
    }]
}

#[tokio::test]
async fn mutate_never_writes_unredacted_request_or_error_bodies_to_fixed_files() {
    remove_fixed_debug_files();

    let success_mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mutateOperationResponses": []
        })))
        .expect(1)
        .mount(&success_mock)
        .await;

    let success_client = GoogleAdsClient::with_base_url(
        &common::test_config(),
        format!("{}/v25", success_mock.uri()),
    )
    .unwrap();
    success_client
        .mutate("1234567890", campaign_operation())
        .await
        .expect("stubbed mutate should succeed");
    let success_request_dumped = Path::new(REQUEST_DUMP).exists();

    remove_fixed_debug_files();

    let error_mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "non-secret test error",
                "status": "INVALID_ARGUMENT"
            }
        })))
        .expect(1)
        .mount(&error_mock)
        .await;

    let error_client =
        GoogleAdsClient::with_base_url(&common::test_config(), format!("{}/v25", error_mock.uri()))
            .unwrap();
    assert!(error_client
        .mutate("1234567890", campaign_operation())
        .await
        .is_err());
    let error_request_dumped = Path::new(REQUEST_DUMP).exists();
    let error_body_dumped = Path::new(ERROR_DUMP).exists();

    remove_fixed_debug_files();

    assert!(
        !success_request_dumped && !error_request_dumped && !error_body_dumped,
        "mutate must not persist unredacted request or error bodies at fixed paths"
    );
}
