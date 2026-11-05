//! `add_ad_group_negative_keywords` excludes keywords at the ad group level,
//! each with its own match type.
//!
//! Only campaign-level negatives (one match type for all) existed. Asserts the
//! request that reaches the wire — one negative ad group criterion per keyword —
//! and each rejection.

mod common;

use common::test_config;
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use mcp_google_ads::tools::keywords_write::{add_ad_group_negative_keywords, NegativeKeywordInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn negative(text: &str, match_type: &str) -> NegativeKeywordInput {
    NegativeKeywordInput {
        text: text.to_string(),
        match_type: match_type.to_string(),
    }
}

#[tokio::test]
async fn ad_group_negatives_reach_google_with_their_own_match_types() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"mutateOperationResponses": []})),
        )
        .mount(&mock)
        .await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let config = test_config();
    let preview = add_ad_group_negative_keywords(
        &config,
        "123-456-7890",
        "201860356700",
        &[
            negative("women", "BROAD"),
            negative(" free shipping ", "PHRASE"),
        ],
    )
    .expect("plan builds");
    confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: preview["plan_id"].as_str().unwrap().to_string(),
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply succeeds");

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let created: Vec<&serde_json::Value> = body["mutateOperations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| &op["adGroupCriterionOperation"]["create"])
        .collect();
    assert_eq!(created.len(), 2);
    for c in &created {
        assert_eq!(c["adGroup"], "customers/1234567890/adGroups/201860356700");
        assert_eq!(c["negative"], true);
    }
    assert_eq!(
        created[0]["keyword"],
        serde_json::json!({"text": "women", "matchType": "BROAD"})
    );
    assert_eq!(
        created[1]["keyword"],
        serde_json::json!({"text": "free shipping", "matchType": "PHRASE"})
    );
}

#[test]
fn an_inconsistent_request_is_rejected() {
    let config = test_config();
    let long = "one two three four five six seven eight nine ten eleven";
    for (keywords, expected) in [
        (vec![], "At least one keyword"),
        (vec![negative("  ", "EXACT")], "must not be empty"),
        (vec![negative("women", "BROADER")], "Invalid match type"),
        (
            vec![negative(long, "PHRASE")],
            "exceeds 80 characters or 10 words",
        ),
        (
            vec![
                negative("Free  Shipping", "PHRASE"),
                negative("free shipping", "PHRASE"),
            ],
            "appears twice",
        ),
    ] {
        let err = add_ad_group_negative_keywords(&config, "1234567890", "555", &keywords)
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{expected}: {err}");
    }

    // The same text under two match types is two distinct exclusions.
    add_ad_group_negative_keywords(
        &config,
        "1234567890",
        "555",
        &[negative("free", "EXACT"), negative("free", "PHRASE")],
    )
    .unwrap();
}
