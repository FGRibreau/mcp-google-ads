//! `dismiss_recommendation` must POST to `recommendations:dismiss`, NOT to
//! `googleAds:mutate`. v25 has no `dismissRecommendationOperation` key on
//! `MutateOperation` — the v0.2.x code routed it through mutate and got 400s.

mod common;

use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn dismiss_recommendation_hits_dedicated_endpoint() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    // Expected: POST /v25/customers/1234567890/recommendations:dismiss
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:dismiss"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "resourceName": "customers/1234567890/recommendations/REC1" }]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    // Catch-all: if anything hits mutate, fail loudly.
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(500).set_body_string("MUST NOT HIT MUTATE"))
        .expect(0)
        .mount(&mock)
        .await;

    let resource_names = vec!["customers/1234567890/recommendations/REC1".to_string()];
    let response = client
        .dismiss_recommendations("1234567890", resource_names)
        .await
        .expect("dismiss call should succeed against mock");

    assert_eq!(response.results.len(), 1);
}

#[tokio::test]
async fn dismiss_recommendation_sends_resource_names_in_body() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    // Verify the body contains the resource name under operations[].resourceName.
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:dismiss"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": []
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let resource = "customers/1234567890/recommendations/REC-DISMISS".to_string();
    let _ = client
        .dismiss_recommendations("1234567890", vec![resource.clone()])
        .await;

    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["operations"][0]["resourceName"], resource);
}

#[tokio::test]
async fn mutate_endpoint_must_not_be_hit_by_dismiss() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    // No mock for `recommendations:dismiss` is intentional — but no mock for
    // mutate either. Any POST will fail with 404 from wiremock unmatched.
    // We assert directly: any request beyond the expected one fails the test.
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:dismiss"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0..=1)
        .mount(&mock)
        .await;

    let _ = client
        .dismiss_recommendations(
            "1234567890",
            vec!["customers/1234567890/recommendations/X".into()],
        )
        .await
        .unwrap();
}
