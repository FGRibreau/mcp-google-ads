//! Black-box + property-based coverage for per-keyword `final_url` overrides
//! in `draft_keywords`.
//!
//! A keyword with a `final_url` must carry `ad_group_criterion.final_urls`
//! (`finalUrls` in the REST payload); a keyword without one must not. Tests use
//! the public crate API only (no mocks) and inspect the stored plan.

use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::keywords_write::{draft_keywords, KeywordWithMatchType};
use proptest::prelude::*;

/// Collect all `adGroupCriterionOperation.create` objects behind a preview.
fn created_criteria(preview: &serde_json::Value) -> Vec<serde_json::Value> {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    plan.mutate_operations
        .iter()
        .filter_map(|op| op.pointer("/adGroupCriterionOperation/create").cloned())
        .collect()
}

#[test]
fn keyword_with_final_url_routes_to_landing_page() {
    let config = Config::default();
    let preview = draft_keywords(
        &config,
        "1234567890",
        "111",
        vec![KeywordWithMatchType {
            text: "buy webhooks".to_string(),
            match_type: "EXACT".to_string(),
            final_url: Some("https://www.hook0.com/pricing".to_string()),
        }],
    )
    .expect("plan builds");

    let criteria = created_criteria(&preview);
    assert_eq!(criteria.len(), 1);
    assert_eq!(
        criteria[0]["finalUrls"],
        serde_json::json!(["https://www.hook0.com/pricing"])
    );
}

#[test]
fn keyword_without_final_url_has_no_final_urls_key() {
    let config = Config::default();
    let preview = draft_keywords(
        &config,
        "1234567890",
        "111",
        vec![KeywordWithMatchType {
            text: "webhook platform".to_string(),
            match_type: "PHRASE".to_string(),
            final_url: None,
        }],
    )
    .expect("plan builds");

    let criteria = created_criteria(&preview);
    assert_eq!(criteria.len(), 1);
    assert!(criteria[0].get("finalUrls").is_none());
}

#[test]
fn invalid_final_url_is_rejected() {
    let config = Config::default();
    let err = draft_keywords(
        &config,
        "1234567890",
        "111",
        vec![KeywordWithMatchType {
            text: "kw".to_string(),
            match_type: "EXACT".to_string(),
            final_url: Some("javascript:alert(1)".to_string()),
        }],
    )
    .expect_err("non-http(s) final_url rejected");
    assert!(err.to_string().contains("http(s) URL"));
}

/// Strategy for a valid match type.
fn match_type() -> impl Strategy<Value = String> {
    prop_oneof![Just("EXACT"), Just("PHRASE"), Just("BROAD")].prop_map(String::from)
}

/// Strategy for an optional valid http(s) final URL.
fn opt_final_url() -> impl Strategy<Value = Option<String>> {
    proptest::option::of(
        (
            prop_oneof![Just("https://"), Just("http://")],
            "[a-z]{1,40}",
        )
            .prop_map(|(scheme, host)| format!("{}{}.example.com/lp", scheme, host)),
    )
}

proptest! {
    /// Invariant: one criterion op per input keyword, and each op carries
    /// `finalUrls == [url]` exactly when the keyword had `Some(url)`, and no
    /// `finalUrls` key otherwise.
    #[test]
    fn draft_keywords_final_urls_track_inputs(
        specs in proptest::collection::vec(
            ("[a-z ]{1,30}", match_type(), opt_final_url()),
            1..6,
        ),
    ) {
        let config = Config::default();
        let keywords: Vec<KeywordWithMatchType> = specs
            .iter()
            .map(|(text, mt, url)| KeywordWithMatchType {
                text: text.clone(),
                match_type: mt.clone(),
                final_url: url.clone(),
            })
            .collect();

        let preview = draft_keywords(&config, "1234567890", "111", keywords)
            .expect("valid keywords build a plan");
        let criteria = created_criteria(&preview);

        prop_assert_eq!(criteria.len(), specs.len());

        for (criterion, (_text, _mt, url)) in criteria.iter().zip(specs.iter()) {
            match url {
                Some(u) => {
                    prop_assert_eq!(&criterion["finalUrls"], &serde_json::json!([u]));
                }
                None => {
                    prop_assert!(criterion.get("finalUrls").is_none());
                }
            }
        }
    }
}
