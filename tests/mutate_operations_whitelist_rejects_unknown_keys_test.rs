//! `MutateOperation` keys are validated client-side against the v25 whitelist
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

// ── customAudiences:mutate ─────────────────────────────────────────────────
//
// Custom audiences have their own RPC with bare operations. The googleAds:mutate
// wrapper style is exactly the mistake `create_custom_audience` once shipped
// with, so the wrapper key must be refused here too.

#[test]
fn custom_audience_operations_accept_bare_create_update_remove() {
    let ops = vec![
        json!({"create": {"name": "S", "type": "AUTO", "members": []}}),
        json!({"update": {"resourceName": "customers/1/customAudiences/2"}, "updateMask": "name"}),
        json!({"remove": "customers/1/customAudiences/2"}),
    ];
    assert!(GoogleAdsClient::validate_custom_audience_operations(&ops).is_ok());
}

#[test]
fn custom_audience_operations_reject_the_mutate_wrapper_and_unknown_keys() {
    let wrapped = vec![json!({"customAudienceOperation": {"create": {"name": "S"}}})];
    let err = GoogleAdsClient::validate_custom_audience_operations(&wrapped).unwrap_err();
    assert!(err.contains("customAudienceOperation"));
    assert!(err.contains("customAudiences:mutate"));

    let typo = vec![json!({"crate": {"name": "S"}})];
    assert!(GoogleAdsClient::validate_custom_audience_operations(&typo).is_err());
}

#[test]
fn custom_audience_operations_need_exactly_one_action() {
    let both = vec![json!({"create": {}, "remove": "customers/1/customAudiences/2"})];
    assert!(GoogleAdsClient::validate_custom_audience_operations(&both)
        .unwrap_err()
        .contains("exactly one"));
    let mask_only = vec![json!({"updateMask": "name"})];
    assert!(GoogleAdsClient::validate_custom_audience_operations(&mask_only).is_err());
    assert!(GoogleAdsClient::validate_custom_audience_operations(&[]).is_err());
}

#[test]
fn every_operation_key_the_display_awareness_tools_emit_is_whitelisted() {
    // The source scan in emitted_operations_are_whitelisted_test covers this
    // too; naming them here makes the deliberate dependency visible.
    for key in [
        "adGroupCriterionOperation",
        "campaignCriterionOperation",
        "customerNegativeCriterionOperation",
        "customConversionGoalOperation",
        "conversionGoalCampaignConfigOperation",
        "campaignConversionGoalOperation",
        "assetOperation",
        "adGroupAdOperation",
        "adGroupOperation",
    ] {
        assert!(
            mcp_google_ads::client::VALID_MUTATE_OPERATION_KEYS.contains(&key),
            "{key} must be whitelisted"
        );
    }
    assert!(
        !mcp_google_ads::client::VALID_MUTATE_OPERATION_KEYS.contains(&"customAudienceOperation"),
        "custom audiences are not a googleAds:mutate operation in v25"
    );
}
