//! Bid adjustments on a campaign's criteria (devices, locations, ad schedules)
//! and the removal of a targeted language.
//!
//! Asserts the request that reaches the wire — one update of `bidModifier` per
//! criterion, under `campaignCriteria/{campaign}~{criterion}`, a device under
//! its fixed criterion ID — and that a language removal is flagged destructive.

mod common;

use common::test_config;
use mcp_google_ads::models::Device;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaign_criteria_write::{
    remove_language_target, set_campaign_bid_modifiers, BidModifierInput,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn device(device: Device, bid_modifier: f64) -> BidModifierInput {
    BidModifierInput {
        device: Some(device),
        criterion_id: None,
        bid_modifier,
    }
}

fn criterion(id: &str, bid_modifier: f64) -> BidModifierInput {
    BidModifierInput {
        device: None,
        criterion_id: Some(id.to_string()),
        bid_modifier,
    }
}

#[tokio::test]
async fn bid_modifiers_reach_google_on_each_criterion() {
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
    let preview = set_campaign_bid_modifiers(
        &config,
        "123-456-7890",
        "777",
        &[
            device(Device::Tablet, 0.0),
            device(Device::Mobile, 1.3),
            criterion("2250", 1.2),
            criterion("340000123", 0.8),
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
    let ops = body["mutateOperations"].as_array().unwrap();
    let sent: Vec<(String, f64, String)> = ops
        .iter()
        .map(|op| {
            let op = &op["campaignCriterionOperation"];
            (
                op["update"]["resourceName"].as_str().unwrap().to_string(),
                op["update"]["bidModifier"].as_f64().unwrap(),
                op["updateMask"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let base = "customers/1234567890/campaignCriteria/777~";
    assert_eq!(
        sent,
        vec![
            (format!("{base}30002"), 0.0, "bidModifier".to_string()),
            (format!("{base}30001"), 1.3, "bidModifier".to_string()),
            (format!("{base}2250"), 1.2, "bidModifier".to_string()),
            (format!("{base}340000123"), 0.8, "bidModifier".to_string()),
        ]
    );
}

#[test]
fn an_inconsistent_bid_modifier_is_rejected() {
    let config = test_config();
    let both = BidModifierInput {
        device: Some(Device::Mobile),
        criterion_id: Some("2250".to_string()),
        bid_modifier: 1.2,
    };
    let neither = BidModifierInput {
        device: None,
        criterion_id: None,
        bid_modifier: 1.2,
    };
    for (modifiers, expected) in [
        (vec![], "At least one bid modifier"),
        (vec![both], "exactly one of device or criterion_id"),
        (vec![neither], "exactly one of device or criterion_id"),
        (vec![criterion("2250", 0.0)], "0 only excludes a device"),
        (vec![device(Device::Mobile, 12.0)], "out of [0.1, 10]"),
        (
            vec![criterion("geoTargetConstants/2250", 1.2)],
            "numeric ID",
        ),
        (
            vec![device(Device::Mobile, 1.2), criterion("30001", 1.3)],
            "Criterion 30001 is adjusted twice",
        ),
    ] {
        let err = set_campaign_bid_modifiers(&config, "1234567890", "777", &modifiers)
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{expected}: {err}");
    }
}

#[test]
fn a_language_removal_targets_its_criterion_and_needs_double_confirmation() {
    let config = test_config();
    for language_id in ["1002", "languageConstants/1002"] {
        let preview = remove_language_target(&config, "1234567890", "777", language_id).unwrap();
        assert_eq!(preview["requires_double_confirm"], true);

        let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();
        assert_eq!(
            plan.mutate_operations[0]["campaignCriterionOperation"]["remove"],
            "customers/1234567890/campaignCriteria/777~1002"
        );
    }

    let err = remove_language_target(&config, "1234567890", "777", "French")
        .unwrap_err()
        .to_string();
    assert!(err.contains("numeric language constant ID"), "{err}");
}
