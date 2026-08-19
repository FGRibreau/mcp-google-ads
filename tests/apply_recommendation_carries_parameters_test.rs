//! `ApplyRecommendationOperation` carries an `apply_parameters` oneof, and
//! several recommendation types cannot be applied without it. Sending only
//! `resourceName` is what made a live SITELINK_ASSET apply fail.
//!
//! Asserts the parameters actually reach the wire, and that a key Google does
//! not define is refused client-side rather than spent on a round trip.

mod common;

use mcp_google_ads::client::APPLY_RECOMMENDATION_PARAMETER_KEYS;
use mcp_google_ads::config::Config;
use mcp_google_ads::tools::recommendations::apply_recommendation;
use proptest::prelude::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn parameters_reach_the_operation_body() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    let parameters = json!({
        "sitelinkAsset": {
            "adAssetApplyParameters": {
                "newAssets": [{
                    "sitelinkAsset": {
                        "linkText": "Hosted in the EU",
                        "description1": "Data plane in France, all plans",
                        "description2": "Subprocessors listed publicly"
                    },
                    "finalUrls": ["https://www.hook0.com/eu-webhook-infrastructure"]
                }],
                "scope": "CAMPAIGN"
            }
        }
    });

    client
        .apply_recommendations(
            "1234567890",
            vec!["customers/1234567890/recommendations/REC1".to_string()],
            Some(parameters.clone()),
        )
        .await
        .expect("apply should succeed against the stub");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let operation = &body["operations"][0];

    assert_eq!(
        operation["resourceName"], "customers/1234567890/recommendations/REC1",
        "the resource name must survive alongside the parameters, got: {operation}"
    );
    assert_eq!(
        operation["sitelinkAsset"], parameters["sitelinkAsset"],
        "the apply parameters must reach the wire verbatim, got: {operation}"
    );
}

#[tokio::test]
async fn omitting_parameters_still_sends_a_bare_operation() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/recommendations:apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    client
        .apply_recommendations(
            "1234567890",
            vec!["customers/1234567890/recommendations/REC2".to_string()],
            None,
        )
        .await
        .expect("apply should succeed against the stub");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    let operation = &body["operations"][0];

    assert_eq!(
        operation["resourceName"],
        "customers/1234567890/recommendations/REC2"
    );
    assert_eq!(
        operation.as_object().map(|o| o.len()),
        Some(1),
        "with no parameters the operation must stay exactly as it was, got: {operation}"
    );
}

#[test]
fn a_key_google_does_not_define_is_refused_before_any_http() {
    let config = Config::default();
    let result = apply_recommendation(
        &config,
        "123-456-7890",
        "rec-1",
        Some(json!({ "sitelinkAssets": { "whatever": true } })),
    );
    let message = result
        .expect_err("a misspelled parameter key must not reach the API")
        .to_string();
    assert!(
        message.contains("sitelinkAssets"),
        "the error must name the offending key, got: {message}"
    );
}

#[test]
fn parameters_must_carry_exactly_one_variant() {
    let config = Config::default();

    // `apply_parameters` is a oneof: zero is meaningless, two is ambiguous.
    for payload in [
        json!({}),
        json!({ "sitelinkAsset": {}, "calloutAsset": {} }),
    ] {
        assert!(
            apply_recommendation(&config, "123-456-7890", "rec-1", Some(payload.clone())).is_err(),
            "expected rejection for {payload}"
        );
    }
}

#[test]
fn parameters_must_be_an_object() {
    let config = Config::default();
    for payload in [json!("sitelinkAsset"), json!([1, 2]), json!(7), json!(null)] {
        assert!(
            apply_recommendation(&config, "123-456-7890", "rec-1", Some(payload.clone())).is_err(),
            "expected rejection for {payload}"
        );
    }
}

proptest! {
    /// The whitelist is the contract: every key Google defines is accepted,
    /// and nothing else is. Checking one hand-picked key would not tell us the
    /// list is wired up rather than special-cased.
    #[test]
    fn every_documented_key_is_accepted(index in 0..APPLY_RECOMMENDATION_PARAMETER_KEYS.len()) {
        let config = Config::default();
        let key = APPLY_RECOMMENDATION_PARAMETER_KEYS[index];
        let payload = json!({ key: {} });
        prop_assert!(
            apply_recommendation(&config, "123-456-7890", "rec-1", Some(payload)).is_ok(),
            "documented key {key} must be accepted"
        );
    }

    /// Anything outside the list is refused, whatever it looks like.
    #[test]
    fn undocumented_keys_are_rejected(key in "[a-zA-Z][a-zA-Z0-9]{0,24}") {
        prop_assume!(!APPLY_RECOMMENDATION_PARAMETER_KEYS.contains(&key.as_str()));
        let config = Config::default();
        prop_assert!(
            apply_recommendation(&config, "123-456-7890", "rec-1", Some(json!({ key.clone(): {} })))
                .is_err(),
            "undocumented key {key} must be rejected"
        );
    }
}
