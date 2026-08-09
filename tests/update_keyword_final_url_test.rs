//! Black-box + property-based coverage for `update_keyword_final_url`.
//!
//! Updating a keyword's landing page in place must emit a single
//! `adGroupCriterionOperation.update` whose `updateMask` is scoped to
//! `finalUrls` and whose `resourceName` targets the exact
//! `adGroupCriteria/{ad_group_id}~{criterion_id}` — this is what preserves the
//! keyword's quality-score history instead of resetting it via remove +
//! re-create. Tests use the public crate API only (no mocks) and inspect the
//! stored plan.

use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::keywords_write::update_keyword_final_url;
use proptest::prelude::*;

/// Return the single `adGroupCriterionOperation` object behind a preview.
fn criterion_operation(preview: &serde_json::Value) -> serde_json::Value {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    assert_eq!(
        plan.mutate_operations.len(),
        1,
        "final URL update is a single-operation mutate"
    );
    plan.mutate_operations[0]
        .pointer("/adGroupCriterionOperation")
        .cloned()
        .expect("adGroupCriterionOperation present")
}

#[test]
fn update_routes_keyword_to_dedicated_page_in_place() {
    let config = Config::default();
    let preview = update_keyword_final_url(
        &config,
        "123-456-7890",
        "555",
        "666",
        "https://www.hook0.com/webhook-api",
    )
    .expect("plan builds");

    // Non-destructive: an in-place update keeps the criterion (and its history).
    assert_eq!(preview["requires_double_confirm"], false);

    let op = criterion_operation(&preview);
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/adGroupCriteria/555~666"
    );
    assert_eq!(
        op["update"]["finalUrls"],
        serde_json::json!(["https://www.hook0.com/webhook-api"])
    );
    // The field mask must isolate finalUrls so nothing else on the criterion
    // (bid, status, keyword text) is touched.
    assert_eq!(op["updateMask"], "finalUrls");
}

#[test]
fn update_never_emits_a_remove_or_create() {
    let config = Config::default();
    let preview = update_keyword_final_url(
        &config,
        "1234567890",
        "111",
        "222",
        "https://www.hook0.com/webhook-service",
    )
    .expect("plan builds");

    let op = criterion_operation(&preview);
    assert!(op.get("remove").is_none(), "must not remove the keyword");
    assert!(op.get("create").is_none(), "must not re-create the keyword");
    assert!(op.get("update").is_some(), "must update in place");
}

#[test]
fn invalid_final_url_is_rejected() {
    let config = Config::default();
    let err = update_keyword_final_url(&config, "1234567890", "111", "222", "ftp://nope")
        .expect_err("non-http(s) URL rejected");
    assert!(err.to_string().contains("http(s) URL"));
}

proptest! {
    /// For any well-formed ids and any absolute http(s) URL, the emitted update
    /// always targets `adGroupCriteria/{ad_group_id}~{criterion_id}` and masks
    /// exactly `finalUrls`.
    #[test]
    fn update_targets_the_right_criterion_and_masks_final_urls(
        ad_group_id in "[1-9][0-9]{0,9}",
        criterion_id in "[1-9][0-9]{0,9}",
        path in "[a-z][a-z-]{0,40}",
    ) {
        let config = Config::default();
        let url = format!("https://www.hook0.com/{}", path);
        let preview = update_keyword_final_url(
            &config,
            "1234567890",
            &ad_group_id,
            &criterion_id,
            &url,
        )
        .expect("valid input builds plan");

        let op = criterion_operation(&preview);
        let expected_resource = format!(
            "customers/1234567890/adGroupCriteria/{}~{}",
            ad_group_id, criterion_id
        );
        prop_assert_eq!(&op["update"]["resourceName"], &serde_json::json!(expected_resource));
        prop_assert_eq!(&op["update"]["finalUrls"], &serde_json::json!([url]));
        prop_assert_eq!(&op["updateMask"], &serde_json::json!("finalUrls"));
    }
}
