//! `update_ad_group` must expose `AdGroup.ad_rotation_mode` so an agent can
//! switch an ad group to "Rotate indefinitely" (ROTATE_FOREVER) without any
//! UI step. Verifies the drafted mutate operation carries the field and the
//! matching update mask entry.

use mcp_google_ads::config::Config;
use mcp_google_ads::models::AdRotationMode;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ad_groups_write::update_ad_group;

#[test]
fn rotation_mode_lands_in_mutate_operation_and_update_mask() {
    let config = Config::default();
    let preview = update_ad_group(
        &config,
        "123-456-7890",
        "199029890591",
        None,
        None,
        Some(AdRotationMode::RotateForever),
    )
    .unwrap();
    assert_eq!(preview["changes"]["ad_rotation_mode"], "ROTATE_FOREVER");

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let op = &plan.mutate_operations[0]["adGroupOperation"];
    assert_eq!(op["update"]["adRotationMode"], "ROTATE_FOREVER");
    assert_eq!(
        op["update"]["resourceName"],
        "customers/1234567890/adGroups/199029890591"
    );
    assert_eq!(op["updateMask"], "adRotationMode");
}

#[test]
fn rotation_mode_combines_with_bid_in_update_mask() {
    let config = Config::default();
    let preview = update_ad_group(
        &config,
        "123-456-7890",
        "555",
        None,
        Some(2_000_000),
        Some(AdRotationMode::Optimize),
    )
    .unwrap();

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let op = &plan.mutate_operations[0]["adGroupOperation"];
    assert_eq!(op["update"]["adRotationMode"], "OPTIMIZE");
    assert_eq!(op["update"]["cpcBidMicros"], "2000000");
    assert_eq!(op["updateMask"], "cpcBidMicros,adRotationMode");
}

#[test]
fn no_fields_at_all_is_rejected_with_rotation_mentioned() {
    let config = Config::default();
    let err = update_ad_group(&config, "123-456-7890", "555", None, None, None)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(err.contains("ad_rotation_mode"));
}
