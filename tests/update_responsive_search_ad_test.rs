//! Black-box + property-based coverage for `update_responsive_search_ad`.
//!
//! Editing an RSA in place must emit a single `adOperation.update` whose
//! `updateMask` names exactly the fields the caller supplied — no more, no
//! less. That mask is the whole safety property of the tool: a field absent
//! from it is left untouched on the live ad, while a field wrongly present
//! overwrites creative the caller never mentioned. Re-creating the ad instead
//! would reset the asset performance labels and the ad-level learning, which is
//! why this path exists at all.
//!
//! Tests drive the public crate API only (no mocks); the apply-path test uses a
//! real local HTTP server (`wiremock`) to pin what actually reaches the wire.

mod common;

use std::collections::BTreeSet;

use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ads_write::{update_responsive_search_ad, UpdateRsaParams};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use mcp_google_ads::UpdateRsaToolParams;
use proptest::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// An `UpdateRsaParams` with every creative field omitted.
///
/// Each test opts into exactly the fields it is about, so "what was provided"
/// and "what the mask names" can be compared without a default leaking in.
fn params<'a>(config: &'a Config, ad_id: &'a str) -> UpdateRsaParams<'a> {
    UpdateRsaParams {
        config,
        customer_id: "123-456-7890",
        ad_id,
        headlines: None,
        descriptions: None,
        final_url: None,
        path1: None,
        path2: None,
    }
}

fn headlines(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("Headline {}", i + 1)).collect()
}

fn descriptions(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("Description number {}", i + 1))
        .collect()
}

/// Return the single `adOperation` object behind a preview.
fn ad_operation(preview: &serde_json::Value) -> serde_json::Value {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    assert_eq!(
        plan.mutate_operations.len(),
        1,
        "an in-place ad edit is a single-operation mutate"
    );
    plan.mutate_operations[0]
        .pointer("/adOperation")
        .cloned()
        .expect("adOperation present")
}

/// The `updateMask` of a preview, split into a set of field paths.
fn mask_fields(preview: &serde_json::Value) -> BTreeSet<String> {
    ad_operation(preview)["updateMask"]
        .as_str()
        .expect("updateMask is a string")
        .split(',')
        .map(str::to_string)
        .collect()
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

// ── Partial update semantics ────────────────────────────────────────────

#[test]
fn updating_only_headlines_masks_only_headlines() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(headlines(3)),
        ..params(&config, "9876543210")
    })
    .expect("plan builds");

    assert_eq!(
        mask_fields(&preview),
        set(&["responsiveSearchAd.headlines"])
    );

    let op = ad_operation(&preview);
    // The descriptions the ad already carries must not appear in the payload:
    // an absent field is an untouched field, not a cleared one.
    assert!(
        op["update"]["responsiveSearchAd"]
            .get("descriptions")
            .is_none(),
        "omitted descriptions must not be written, got: {}",
        op["update"]
    );
    assert!(
        op["update"].get("finalUrls").is_none(),
        "omitted final_url must not be written"
    );
    assert!(op["update"]["responsiveSearchAd"].get("path1").is_none());
    assert!(op["update"]["responsiveSearchAd"].get("path2").is_none());
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/ads/9876543210"
    );
}

#[test]
fn updating_only_descriptions_masks_only_descriptions() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        descriptions: Some(descriptions(2)),
        ..params(&config, "555")
    })
    .expect("plan builds");

    assert_eq!(
        mask_fields(&preview),
        set(&["responsiveSearchAd.descriptions"])
    );
    let op = ad_operation(&preview);
    assert!(
        op["update"]["responsiveSearchAd"]
            .get("headlines")
            .is_none(),
        "omitted headlines must not be written"
    );
}

#[test]
fn updating_only_the_final_url_masks_only_final_urls() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        final_url: Some("https://www.hook0.com/webhook-api"),
        ..params(&config, "555")
    })
    .expect("plan builds");

    assert_eq!(mask_fields(&preview), set(&["finalUrls"]));
    let op = ad_operation(&preview);
    assert_eq!(
        op["update"]["finalUrls"],
        serde_json::json!(["https://www.hook0.com/webhook-api"])
    );
    assert!(
        op["update"].get("responsiveSearchAd").is_none(),
        "no creative field was supplied, so no responsiveSearchAd payload"
    );
}

#[test]
fn updating_every_field_masks_every_field() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(headlines(4)),
        descriptions: Some(descriptions(3)),
        final_url: Some("https://www.hook0.com/webhook-api"),
        path1: Some("webhook"),
        path2: Some("api"),
        ..params(&config, "555")
    })
    .expect("plan builds");

    assert_eq!(
        mask_fields(&preview),
        set(&[
            "responsiveSearchAd.headlines",
            "responsiveSearchAd.descriptions",
            "responsiveSearchAd.path1",
            "responsiveSearchAd.path2",
            "finalUrls",
        ])
    );

    let op = ad_operation(&preview);
    assert_eq!(
        op["update"]["responsiveSearchAd"]["headlines"][0]["text"],
        "Headline 1"
    );
    assert_eq!(
        op["update"]["responsiveSearchAd"]["descriptions"][0]["text"],
        "Description number 1"
    );
    assert_eq!(op["update"]["responsiveSearchAd"]["path1"], "webhook");
    assert_eq!(op["update"]["responsiveSearchAd"]["path2"], "api");
}

#[test]
fn a_path_can_be_updated_without_touching_the_creative() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        path1: Some("pricing"),
        ..params(&config, "555")
    })
    .expect("plan builds");

    assert_eq!(mask_fields(&preview), set(&["responsiveSearchAd.path1"]));
    let op = ad_operation(&preview);
    assert!(op["update"]["responsiveSearchAd"]
        .get("headlines")
        .is_none());
    assert!(op["update"]["responsiveSearchAd"].get("path2").is_none());
}

#[test]
fn the_update_never_removes_or_re_creates_the_ad() {
    let config = Config::default();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(headlines(3)),
        ..params(&config, "555")
    })
    .expect("plan builds");

    let op = ad_operation(&preview);
    assert!(op.get("remove").is_none(), "must not remove the ad");
    assert!(op.get("create").is_none(), "must not re-create the ad");
    assert!(op.get("update").is_some(), "must update in place");
    // In-place edits are not destructive, so no second confirmation is asked.
    assert_eq!(preview["requires_double_confirm"], false);
    assert_eq!(preview["operation"], "update_responsive_search_ad");
    assert_eq!(preview["entity_type"], "ad");
    assert_eq!(preview["entity_id"], "555");
}

#[test]
fn an_update_with_no_field_is_refused_rather_than_a_silent_no_op() {
    let config = Config::default();
    let err = update_responsive_search_ad(&params(&config, "555"))
        .expect_err("an empty field mask writes nothing and must not look like a success");
    let msg = err.to_string();
    assert!(
        msg.contains("At least one of headlines, descriptions, final_url, path1 or path2"),
        "the error must list what can be provided, got: {msg}"
    );
}

// ── Bounds: each one refused above the limit, accepted at it ────────────

#[test]
fn headline_count_bounds_hold_in_both_directions() {
    let config = Config::default();

    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(headlines(3)),
            ..params(&config, "555")
        })
        .is_ok(),
        "3 headlines is the documented minimum and must be accepted"
    );
    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(headlines(15)),
            ..params(&config, "555")
        })
        .is_ok(),
        "15 headlines is the documented maximum and must be accepted"
    );

    for count in [2usize, 16] {
        let msg = update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(headlines(count)),
            ..params(&config, "555")
        })
        .expect_err("out-of-range headline count is refused")
        .to_string();
        assert!(
            msg.contains("headlines") && msg.contains("3-15"),
            "the error must name the field and the bound, got: {msg}"
        );
    }
}

#[test]
fn headline_length_bound_holds_in_both_directions() {
    let config = Config::default();

    let mut at_limit = headlines(3);
    at_limit[0] = "a".repeat(30);
    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(at_limit),
            ..params(&config, "555")
        })
        .is_ok(),
        "a 30-character headline is exactly at the limit and must be accepted"
    );

    let mut over_limit = headlines(3);
    over_limit[0] = "a".repeat(31);
    let msg = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(over_limit),
        ..params(&config, "555")
    })
    .expect_err("a 31-character headline is refused")
    .to_string();
    assert!(
        msg.contains("Headline") && msg.contains("30 character limit"),
        "the error must name the field and the bound, got: {msg}"
    );
}

#[test]
fn description_count_bounds_hold_in_both_directions() {
    let config = Config::default();

    for count in [2usize, 4] {
        assert!(
            update_responsive_search_ad(&UpdateRsaParams {
                descriptions: Some(descriptions(count)),
                ..params(&config, "555")
            })
            .is_ok(),
            "{count} descriptions is within the documented range"
        );
    }

    for count in [1usize, 5] {
        let msg = update_responsive_search_ad(&UpdateRsaParams {
            descriptions: Some(descriptions(count)),
            ..params(&config, "555")
        })
        .expect_err("out-of-range description count is refused")
        .to_string();
        assert!(
            msg.contains("descriptions") && msg.contains("2-4"),
            "the error must name the field and the bound, got: {msg}"
        );
    }
}

#[test]
fn description_length_bound_holds_in_both_directions() {
    let config = Config::default();

    let mut at_limit = descriptions(2);
    at_limit[0] = "a".repeat(90);
    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            descriptions: Some(at_limit),
            ..params(&config, "555")
        })
        .is_ok(),
        "a 90-character description is exactly at the limit and must be accepted"
    );

    let mut over_limit = descriptions(2);
    over_limit[0] = "a".repeat(91);
    let msg = update_responsive_search_ad(&UpdateRsaParams {
        descriptions: Some(over_limit),
        ..params(&config, "555")
    })
    .expect_err("a 91-character description is refused")
    .to_string();
    assert!(
        msg.contains("Description") && msg.contains("90 character limit"),
        "the error must name the field and the bound, got: {msg}"
    );
}

#[test]
fn final_url_bounds_hold_in_both_directions() {
    let config = Config::default();

    let prefix = "https://";
    let at_limit = format!("{}{}", prefix, "a".repeat(2048 - prefix.len()));
    assert_eq!(at_limit.chars().count(), 2048);
    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            final_url: Some(at_limit.as_str()),
            ..params(&config, "555")
        })
        .is_ok(),
        "a 2048-character URL is exactly at the limit and must be accepted"
    );

    let over_limit = format!("{}a", at_limit);
    let msg = update_responsive_search_ad(&UpdateRsaParams {
        final_url: Some(over_limit.as_str()),
        ..params(&config, "555")
    })
    .expect_err("a 2049-character URL is refused")
    .to_string();
    assert!(
        msg.contains("final_url") && msg.contains("2048 character limit"),
        "the error must name the field and the bound, got: {msg}"
    );
}

#[test]
fn a_relative_or_non_http_final_url_is_refused() {
    let config = Config::default();
    for url in ["ftp://example.com", "/webhook-api", "example.com", ""] {
        let msg = update_responsive_search_ad(&UpdateRsaParams {
            final_url: Some(url),
            ..params(&config, "555")
        })
        .expect_err("only absolute http(s) URLs are accepted")
        .to_string();
        assert!(
            msg.contains("final_url"),
            "the error must name the field, got: {msg}"
        );
    }
}

#[test]
fn display_path_bounds_hold_in_both_directions() {
    let config = Config::default();

    let at_limit = "a".repeat(15);
    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            path1: Some(at_limit.as_str()),
            path2: Some(at_limit.as_str()),
            ..params(&config, "555")
        })
        .is_ok(),
        "a 15-character path is exactly at the limit and must be accepted"
    );

    let over_limit = "a".repeat(16);
    for (field, p1, p2) in [
        ("path1", Some(over_limit.as_str()), None),
        ("path2", None, Some(over_limit.as_str())),
    ] {
        let msg = update_responsive_search_ad(&UpdateRsaParams {
            path1: p1,
            path2: p2,
            ..params(&config, "555")
        })
        .expect_err("a 16-character path is refused")
        .to_string();
        assert!(
            msg.contains(field) && msg.contains("15 character limit"),
            "the error must name the field and the bound, got: {msg}"
        );
    }
}

#[test]
fn an_out_of_bounds_value_is_never_truncated_to_fit() {
    let config = Config::default();
    let mut too_long = headlines(3);
    too_long[0] = "a".repeat(31);

    // The refusal must be total: no plan is stored, so nothing can be applied
    // with a silently shortened headline.
    let result = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(too_long),
        ..params(&config, "555")
    });
    assert!(result.is_err(), "an over-long headline is refused outright");
}

#[test]
fn the_ad_id_must_be_a_bare_numeric_id() {
    let config = Config::default();

    assert!(
        update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(headlines(3)),
            ..params(&config, "1234567890123456789")
        })
        .is_ok(),
        "19 digits is the widest int64 ID and must be accepted"
    );

    for bad in [
        "",
        "customers/1234567890/ads/555",
        "555~666",
        "abc",
        "12345678901234567890",
    ] {
        let msg = update_responsive_search_ad(&UpdateRsaParams {
            headlines: Some(headlines(3)),
            ..params(&config, bad)
        })
        .expect_err("a non-numeric ad ID is refused")
        .to_string();
        assert!(
            msg.contains("ad_id"),
            "the error must name the field, got: {msg}"
        );
    }
}

#[test]
fn a_blocked_operation_is_refused() {
    let config = Config {
        safety: mcp_google_ads::config::SafetyConfig {
            blocked_operations: vec!["update_responsive_search_ad".to_string()],
            ..Config::default().safety
        },
        ..Config::default()
    };
    let msg = update_responsive_search_ad(&UpdateRsaParams {
        headlines: Some(headlines(3)),
        ..params(&config, "555")
    })
    .expect_err("the operation is blocked by configuration")
    .to_string();
    assert!(msg.contains("blocked"), "got: {msg}");
}

// ── Apply path: what actually reaches the wire ──────────────────────────

#[tokio::test]
async fn a_partial_update_reaches_the_wire_with_only_the_supplied_fields() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:mutate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "mutateOperationResponses": [
                {"adResult": {"resourceName": "customers/1234567890/ads/555"}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;
    std::env::set_var("GOOGLE_ADS_API_BASE_URL", format!("{}/v25", mock.uri()));

    let config = common::test_config();
    let preview = update_responsive_search_ad(&UpdateRsaParams {
        config: &config,
        customer_id: "1234567890",
        ad_id: "555",
        headlines: Some(headlines(3)),
        descriptions: None,
        final_url: Some("https://www.hook0.com/webhook-api"),
        path1: None,
        path2: None,
    })
    .expect("plan builds");

    let plan_id = preview["plan_id"]
        .as_str()
        .expect("plan_id present")
        .to_string();

    let result = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id,
            dry_run: false,
            ..Default::default()
        },
    )
    .await
    .expect("apply succeeds");
    assert_eq!(result["status"], "APPLIED");

    let requests = mock
        .received_requests()
        .await
        .expect("mock recorded requests");
    assert_eq!(requests.len(), 1, "an ad edit is one request");
    let body: serde_json::Value =
        serde_json::from_slice(&requests[0].body).expect("request body is JSON");

    let op = &body["mutateOperations"][0]["adOperation"];
    assert_eq!(
        op["updateMask"], "responsiveSearchAd.headlines,finalUrls",
        "the wire mask must name the two supplied fields and nothing else"
    );
    assert_eq!(
        op["update"]["resourceName"], "customers/1234567890/ads/555",
        "the edit must target the ad by its own resource name"
    );
    assert!(
        op["update"]["responsiveSearchAd"]
            .get("descriptions")
            .is_none(),
        "descriptions were not supplied and must not travel, got: {}",
        op["update"]
    );

    std::env::remove_var("GOOGLE_ADS_API_BASE_URL");
}

// ── Tool parameters ────────────────────────────────────────────────────

#[test]
fn a_misspelled_parameter_is_rejected_rather_than_ignored() {
    // Swallowing `headline` would apply an update that changes nothing the
    // caller asked for, and report success.
    let err = serde_json::from_value::<UpdateRsaToolParams>(serde_json::json!({
        "ad_id": "555",
        "headline": ["Webhooks as a service"]
    }))
    .expect_err("an undefined field must not be swallowed");
    assert!(
        err.to_string().contains("headline"),
        "the error must name the offending key, got: {err}"
    );
}

#[test]
fn the_defined_parameters_still_deserialize() {
    let params = serde_json::from_value::<UpdateRsaToolParams>(serde_json::json!({
        "customer_id": "123-456-7890",
        "ad_id": "555",
        "headlines": ["Webhooks as a service", "Ship webhooks in a day", "Open source webhooks"],
        "descriptions": ["Stop building webhook infrastructure.", "Retries, logs, signatures."],
        "final_url": "https://www.hook0.com/webhook-api",
        "path1": "webhook",
        "path2": "api"
    }))
    .expect("a fully specified, valid payload must parse");
    assert_eq!(params.ad_id, "555");

    // Only `ad_id` is required: a partial update names just what it changes.
    serde_json::from_value::<UpdateRsaToolParams>(serde_json::json!({"ad_id": "555"}))
        .expect("omitting every optional field must remain valid");
}

proptest! {
    /// Whatever subset of fields a caller supplies, the emitted mask names
    /// exactly that subset — never a field that was left out, never one short.
    #[test]
    fn the_mask_names_exactly_the_supplied_fields(
        with_headlines in any::<bool>(),
        with_descriptions in any::<bool>(),
        with_final_url in any::<bool>(),
        with_path1 in any::<bool>(),
        with_path2 in any::<bool>(),
        ad_id in "[1-9][0-9]{0,9}",
    ) {
        prop_assume!(
            with_headlines || with_descriptions || with_final_url || with_path1 || with_path2
        );

        let config = Config::default();
        let mut expected: BTreeSet<String> = BTreeSet::new();
        if with_headlines { expected.insert("responsiveSearchAd.headlines".to_string()); }
        if with_descriptions { expected.insert("responsiveSearchAd.descriptions".to_string()); }
        if with_final_url { expected.insert("finalUrls".to_string()); }
        if with_path1 { expected.insert("responsiveSearchAd.path1".to_string()); }
        if with_path2 { expected.insert("responsiveSearchAd.path2".to_string()); }

        let preview = update_responsive_search_ad(&UpdateRsaParams {
            config: &config,
            customer_id: "1234567890",
            ad_id: &ad_id,
            headlines: if with_headlines { Some(headlines(3)) } else { None },
            descriptions: if with_descriptions { Some(descriptions(2)) } else { None },
            final_url: if with_final_url { Some("https://www.hook0.com/webhook-api") } else { None },
            path1: if with_path1 { Some("webhook") } else { None },
            path2: if with_path2 { Some("api") } else { None },
        })
        .expect("a plan builds for any non-empty subset of valid fields");

        prop_assert_eq!(mask_fields(&preview), expected);

        let op = ad_operation(&preview);
        let expected_resource = format!("customers/1234567890/ads/{}", ad_id);
        prop_assert_eq!(&op["update"]["resourceName"], &serde_json::json!(expected_resource));
    }
}
