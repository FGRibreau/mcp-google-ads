//! A parameter the tool does not define must be an error, never a shrug.
//!
//! serde ignores unknown fields by default, so a caller who passes `days: 2` to
//! a tool that only understands `date_range_start`/`date_range_end` gets a
//! successful answer computed over the DEFAULT window. Nothing signals the
//! mistake: the reply looks like the one that was asked for. That silently
//! produced a wrong reading of a real campaign — two windows that were believed
//! to be 14 and 2 days were both 30, and the conclusion drawn from comparing
//! them was consequently false.
//!
//! Also pins the search-terms row cap, which used to be hardcoded, so the
//! caller can ask for a slice small enough to actually read.

mod common;

use mcp_google_ads::{DateRangeParams, SearchTermsToolParams};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[test]
fn an_undefined_parameter_is_rejected_rather_than_ignored() {
    let err = serde_json::from_value::<DateRangeParams>(json!({
        "date_range_start": "2026-08-19",
        "date_range_end": "2026-08-20",
        "days": 2
    }))
    .expect_err("`days` is not a field of this tool and must not be swallowed");

    assert!(
        err.to_string().contains("days"),
        "the error must name the offending key so the caller can fix it, got: {err}"
    );
}

#[test]
fn the_defined_parameters_still_deserialize() {
    // The guard must reject what is undefined without breaking what is defined:
    // a rejection rule that also rejects valid input is worse than none.
    let params = serde_json::from_value::<DateRangeParams>(json!({
        "customer_id": "123-456-7890",
        "date_range_start": "2026-08-19",
        "date_range_end": "2026-08-20"
    }))
    .expect("a fully specified, valid payload must still parse");
    assert_eq!(params.date_range_start.as_deref(), Some("2026-08-19"));

    // Every field is optional, so an empty object stays valid.
    serde_json::from_value::<DateRangeParams>(json!({}))
        .expect("omitting every optional field must remain valid");
}

#[test]
fn search_terms_accepts_a_row_limit() {
    let params = serde_json::from_value::<SearchTermsToolParams>(json!({
        "date_range_start": "2026-08-19",
        "date_range_end": "2026-08-20",
        "limit": 25
    }))
    .expect("limit is a defined parameter of the search terms tool");
    assert_eq!(params.limit, Some(25));
}

#[tokio::test]
async fn the_requested_limit_reaches_the_query() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    mcp_google_ads::tools::keywords::get_search_terms(&client, "1234567890", None, None, Some(25))
        .await
        .expect("search should succeed against the stub");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let query = body["query"].as_str().unwrap_or_default();

    assert!(
        query.contains("LIMIT 25"),
        "the caller's limit must reach the GAQL query, got: {query}"
    );
}

#[tokio::test]
async fn an_absent_limit_keeps_the_documented_default() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    mcp_google_ads::tools::keywords::get_search_terms(&client, "1234567890", None, None, None)
        .await
        .expect("search should succeed against the stub");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let query = body["query"].as_str().unwrap_or_default();

    assert!(
        query.contains("LIMIT 200"),
        "omitting the limit must keep the documented 200-row default, got: {query}"
    );
}

#[tokio::test]
async fn an_out_of_range_limit_is_refused_before_any_http() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    // Nothing may be sent: an unbounded row count is rejected client-side.
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("MUST NOT BE CALLED"))
        .expect(0)
        .mount(&mock)
        .await;

    for bad in [0_u32, 100_001] {
        mcp_google_ads::tools::keywords::get_search_terms(
            &client,
            "1234567890",
            None,
            None,
            Some(bad),
        )
        .await
        .expect_err(&format!("limit {bad} is out of range and must be refused"));
    }
}
