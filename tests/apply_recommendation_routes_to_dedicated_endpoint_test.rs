//! `apply_recommendation` must POST to `recommendations:apply`, NOT to
//! `googleAds:mutate`. Symmetric to the dismiss test.

mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn apply_recommendation_hits_dedicated_endpoint() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "resourceName": "customers/1234567890/recommendations/REC1" }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(500).set_body_string("MUST NOT HIT MUTATE"))
        .expect(0)
        .mount(&mock)
        .await;

    let resource_names = vec!["customers/1234567890/recommendations/REC1".to_string()];
    let response = client
        .apply_recommendations("1234567890", resource_names, None)
        .await
        .expect("apply call should succeed against mock");

    assert_eq!(response.results.len(), 1);
}

#[tokio::test]
async fn apply_recommendation_body_carries_partial_failure_flag() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .expect(1)
        .mount(&mock)
        .await;

    let _ = client
        .apply_recommendations(
            "1234567890",
            vec!["customers/1234567890/recommendations/X".to_string()],
            None,
        )
        .await
        .unwrap();

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["partialFailure"], true);
    assert!(body["operations"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false));
}
