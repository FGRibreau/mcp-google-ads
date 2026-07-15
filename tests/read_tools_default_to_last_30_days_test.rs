//! Regression: the read tools that advertise a "last 30 days" default were
//! emitting NO `segments.date` predicate when called without dates, so Google
//! Ads returned lifetime totals (observed ~2x the real 30-day figures). Assert
//! each defaults to `segments.date DURING LAST_30_DAYS`, and that an explicit
//! range still wins over the default.
//!
//! Black-box via wiremock: we stub the `googleAds:search` endpoint and inspect
//! the outgoing GAQL query — no code is mocked.

mod common;

use mcp_google_ads::client::GoogleAdsClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Spawn a mock that answers the search endpoint with an empty result set, so
/// any read tool completes after issuing exactly its GAQL query.
async fn mock_empty_search() -> (MockServer, GoogleAdsClient) {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path("/v23/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&mock)
        .await;
    (mock, client)
}

/// The GAQL query string of the first request the tool sent.
async fn sent_query(mock: &MockServer) -> String {
    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    body["query"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn campaign_performance_defaults_to_last_30_days() {
    let (mock, client) = mock_empty_search().await;
    let _ = mcp_google_ads::tools::campaigns::get_campaign_performance(
        &client,
        "1234567890",
        None,
        None,
    )
    .await;
    let q = sent_query(&mock).await;
    assert!(
        q.contains("segments.date DURING LAST_30_DAYS"),
        "campaign perf must default to a 30-day window, not lifetime. Query: {q}"
    );
}

#[tokio::test]
async fn ad_performance_defaults_to_last_30_days() {
    let (mock, client) = mock_empty_search().await;
    let _ = mcp_google_ads::tools::ads::get_ad_performance(&client, "1234567890", None, None).await;
    let q = sent_query(&mock).await;
    assert!(
        q.contains("segments.date DURING LAST_30_DAYS"),
        "ad perf must default to a 30-day window, not lifetime. Query: {q}"
    );
}

#[tokio::test]
async fn keyword_performance_defaults_to_last_30_days() {
    let (mock, client) = mock_empty_search().await;
    let _ =
        mcp_google_ads::tools::keywords::get_keyword_performance(&client, "1234567890", None, None)
            .await;
    let q = sent_query(&mock).await;
    assert!(
        q.contains("segments.date DURING LAST_30_DAYS"),
        "keyword perf must default to a 30-day window, not lifetime. Query: {q}"
    );
}

#[tokio::test]
async fn explicit_dates_win_over_the_30_day_default() {
    let (mock, client) = mock_empty_search().await;
    let _ = mcp_google_ads::tools::campaigns::get_campaign_performance(
        &client,
        "1234567890",
        Some("2026-01-01"),
        Some("2026-01-31"),
    )
    .await;
    let q = sent_query(&mock).await;
    assert!(
        q.contains("2026-01-01") && q.contains("2026-01-31") && q.contains("BETWEEN"),
        "an explicit range must be honoured. Query: {q}"
    );
    assert!(
        !q.contains("LAST_30_DAYS"),
        "the 30-day default must not leak in when explicit dates are given. Query: {q}"
    );
}
