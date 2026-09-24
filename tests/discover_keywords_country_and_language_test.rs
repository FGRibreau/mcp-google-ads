//! `discover_keywords` wrapped `KeywordPlanIdeaService.GenerateKeywordIdeas`
//! but never passed `geoTargetConstants` through, and hardcoded
//! `languageConstants/1000` (English). Every caller therefore got worldwide,
//! English-only volumes no matter which market they asked about.
//!
//! Assert the wrapper now resolves a `country` / `language` against the live
//! API and forwards both on the generateKeywordIdeas request, while the
//! no-argument call keeps its previous behaviour.
//!
//! Black-box via wiremock: the resolver lookups and the ideas request are all
//! stubbed, and we inspect the outgoing bodies — no code is mocked.

mod common;

use mcp_google_ads::client::GoogleAdsClient;
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_PATH: &str = "/v25/customers/1234567890/googleAds:search";
const IDEAS_PATH: &str = "/v25/customers/1234567890:generateKeywordIdeas";

/// Stub the geo_target_constant lookup with a single United Kingdom row.
async fn mock_country_lookup(mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .and(body_string_contains("geo_target_constant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "geoTargetConstant": {
                    "id": "2826",
                    "name": "United Kingdom",
                    "canonicalName": "United Kingdom",
                    "countryCode": "GB",
                    "targetType": "Country"
                }
            }]
        })))
        .mount(mock)
        .await;
}

/// Stub the language_constant lookup with a single English (UK) row.
async fn mock_language_lookup(mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .and(body_string_contains("language_constant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "languageConstant": {
                    "id": "1000",
                    "code": "en",
                    "name": "English"
                }
            }]
        })))
        .mount(mock)
        .await;
}

/// Stub generateKeywordIdeas with an empty result set.
async fn mock_ideas(mock: &MockServer) {
    Mock::given(method("POST"))
        .and(path(IDEAS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(mock)
        .await;
}

async fn spawn_all() -> (MockServer, GoogleAdsClient) {
    let (mock, client) = common::spawn_mock_google_ads().await;
    mock_country_lookup(&mock).await;
    mock_language_lookup(&mock).await;
    mock_ideas(&mock).await;
    (mock, client)
}

/// Body of the first request sent to the generateKeywordIdeas endpoint.
async fn ideas_body(mock: &MockServer) -> Value {
    let requests = mock.received_requests().await.unwrap();
    let ideas = requests
        .iter()
        .find(|r| r.url.path().ends_with(":generateKeywordIdeas"))
        .expect("expected a generateKeywordIdeas request");
    ideas.body_json().unwrap()
}

/// GAQL query strings of every search request sent, in order.
async fn sent_queries(mock: &MockServer) -> Vec<String> {
    let requests = mock.received_requests().await.unwrap();
    requests
        .iter()
        .filter(|r| r.url.path().ends_with("googleAds:search"))
        .map(|r| {
            let body: Value = r.body_json().unwrap();
            body["query"].as_str().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn country_code_is_resolved_and_forwarded_as_geo_target_constant() {
    let (mock, client) = spawn_all().await;

    mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        Some("GB"),
        None,
    )
    .await
    .expect("discover_keywords should succeed");

    let body = ideas_body(&mock).await;
    assert_eq!(
        body["geoTargetConstants"],
        json!(["geoTargetConstants/2826"]),
        "resolved country must be forwarded as geoTargetConstants. Body: {body}"
    );
}

#[tokio::test]
async fn country_name_is_resolved_case_insensitively() {
    let (mock, client) = spawn_all().await;

    mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        Some("united kingdom"),
        None,
    )
    .await
    .expect("a lowercase country name should still resolve");

    let body = ideas_body(&mock).await;
    assert_eq!(
        body["geoTargetConstants"],
        json!(["geoTargetConstants/2826"]),
        "country name lookup must match case-insensitively. Body: {body}"
    );
}

#[tokio::test]
async fn country_lookup_filters_to_country_target_type() {
    let (mock, client) = spawn_all().await;

    let _ = mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        Some("GB"),
        None,
    )
    .await;

    let queries = sent_queries(&mock).await;
    let geo_query = queries
        .iter()
        .find(|q| q.contains("geo_target_constant"))
        .expect("expected a geo_target_constant lookup");
    assert!(
        geo_query.contains("geo_target_constant.target_type = 'Country'"),
        "a country lookup must not match cities or regions. Query: {geo_query}"
    );
    assert!(
        geo_query.contains("geo_target_constant.country_code = 'GB'"),
        "a two-letter input must resolve via country_code. Query: {geo_query}"
    );
}

#[tokio::test]
async fn language_is_resolved_and_forwarded() {
    let (mock, client) = spawn_all().await;

    mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        None,
        Some("en"),
    )
    .await
    .expect("discover_keywords should succeed");

    let body = ideas_body(&mock).await;
    assert_eq!(
        body["language"], "languageConstants/1000",
        "resolved language must be forwarded. Body: {body}"
    );

    let queries = sent_queries(&mock).await;
    assert!(
        queries
            .iter()
            .any(|q| q.contains("language_constant.code = 'en'")),
        "a short language input must resolve via language_constant.code. Queries: {queries:?}"
    );
}

#[tokio::test]
async fn omitting_country_sends_no_geo_target_constants() {
    let (mock, client) = spawn_all().await;

    mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        None,
        None,
    )
    .await
    .expect("discover_keywords should succeed");

    let body = ideas_body(&mock).await;
    assert!(
        body.get("geoTargetConstants").is_none(),
        "no country means no geo filter, not an empty array. Body: {body}"
    );
    assert_eq!(
        body["language"], "languageConstants/1000",
        "the previous English default must be preserved. Body: {body}"
    );
}

#[tokio::test]
async fn omitting_country_skips_the_resolver_lookup() {
    let (mock, client) = spawn_all().await;

    let _ = mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        None,
        None,
    )
    .await;

    let queries = sent_queries(&mock).await;
    assert!(
        queries.is_empty(),
        "a call with no country or language must not pay for a lookup. Queries: {queries:?}"
    );
}

#[tokio::test]
async fn unknown_country_is_a_validation_error_not_a_silent_worldwide_query() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&mock)
        .await;
    mock_ideas(&mock).await;

    let err = mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["running shoes".to_string()],
        Some("Atlantis"),
        None,
    )
    .await
    .expect_err("an unresolvable country must fail loudly");

    let msg = err.to_string();
    assert!(
        msg.contains("Atlantis"),
        "the error must name the input that failed. Got: {msg}"
    );

    let requests = mock.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|r| r.url.path().ends_with(":generateKeywordIdeas")),
        "a failed country resolution must abort before requesting ideas"
    );
}

#[tokio::test]
async fn ambiguous_country_match_lists_the_candidates() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                { "geoTargetConstant": { "id": "2276", "name": "Germany", "countryCode": "DE", "targetType": "Country" } },
                { "geoTargetConstant": { "id": "2826", "name": "German Republic", "countryCode": "GB", "targetType": "Country" } }
            ]
        })))
        .mount(&mock)
        .await;
    mock_ideas(&mock).await;

    let err = mcp_google_ads::tools::keyword_planner::discover_keywords(
        &client,
        "1234567890",
        vec!["schuhe".to_string()],
        Some("German"),
        None,
    )
    .await
    .expect_err("an ambiguous country must fail rather than guess");

    let msg = err.to_string();
    assert!(
        msg.contains("Germany") && msg.contains("German Republic"),
        "the error must list the candidates so the caller can disambiguate. Got: {msg}"
    );
}
