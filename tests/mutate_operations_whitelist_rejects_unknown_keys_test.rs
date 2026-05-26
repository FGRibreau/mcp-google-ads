//! `MutateOperation` keys are validated client-side against the v23 whitelist
//! BEFORE any HTTP traffic. `dismissRecommendationOperation` is the canonical
//! offender — accepting it was the v0.2.x root cause behind Bug 2.

use mcp_google_ads::client::{GoogleAdsClient, MutateOperation};
use serde_json::json;

#[test]
fn rejects_dismiss_recommendation_operation() {
    let ops = vec![MutateOperation {
        operation: json!({"dismissRecommendationOperation": {"resourceName": "x"}}),
    }];
    let err = GoogleAdsClient::validate_mutate_operations(&ops).unwrap_err();
    assert!(err.contains("dismissRecommendationOperation"));
    assert!(err.contains("apply_recommendations / dismiss_recommendations"));
}

#[test]
fn rejects_apply_recommendation_operation() {
    let ops = vec![MutateOperation {
        operation: json!({"applyRecommendationOperation": {"resourceName": "x"}}),
    }];
    assert!(GoogleAdsClient::validate_mutate_operations(&ops).is_err());
}

#[test]
fn rejects_typo_in_known_key() {
    let ops = vec![MutateOperation {
        operation: json!({"adGroupOperationS": {"create": {}}}), // typo
    }];
    let err = GoogleAdsClient::validate_mutate_operations(&ops).unwrap_err();
    assert!(err.contains("Unknown MutateOperation key"));
}

#[test]
fn accepts_known_keys() {
    let ops = vec![
        MutateOperation {
            operation: json!({"adGroupOperation": {"create": {}}}),
        },
        MutateOperation {
            operation: json!({"campaignBudgetOperation": {"create": {}}}),
        },
        MutateOperation {
            operation: json!({"adGroupAdOperation": {"create": {}}}),
        },
        MutateOperation {
            operation: json!({"campaignOperation": {"create": {}}}),
        },
    ];
    assert!(GoogleAdsClient::validate_mutate_operations(&ops).is_ok());
}

#[test]
fn rejects_first_offender_in_mixed_batch() {
    let ops = vec![
        MutateOperation {
            operation: json!({"adGroupOperation": {"create": {}}}),
        },
        MutateOperation {
            operation: json!({"dismissRecommendationOperation": {"resourceName": "x"}}),
        },
    ];
    let err = GoogleAdsClient::validate_mutate_operations(&ops).unwrap_err();
    // Index 1 because the first op was valid.
    assert!(err.contains("index 1"));
}
