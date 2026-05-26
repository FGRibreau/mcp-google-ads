//! `create_ad_group` default status must be PAUSED — consistent with the
//! other write tools. v0.2.x defaulted to ENABLED, the only outlier.

use mcp_google_ads::config::Config;
use mcp_google_ads::models::AdStatus;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ad_groups_write::create_ad_group;

#[test]
fn default_status_paused_consistent_with_other_write_tools() {
    let config = Config::default();
    let preview = create_ad_group(&config, "123-456-7890", "CAMP1", "AG1", None, None).unwrap();
    assert_eq!(preview["status_after_apply"], "PAUSED");
    assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let create = &plan.mutate_operations[0]["adGroupOperation"]["create"];
    assert_eq!(create["status"], "PAUSED");
}

#[test]
fn explicit_enabled_emits_no_hint() {
    let config = Config::default();
    let preview = create_ad_group(
        &config,
        "123-456-7890",
        "CAMP1",
        "AG1",
        None,
        Some(AdStatus::Enabled),
    )
    .unwrap();
    assert_eq!(preview["status_after_apply"], "ENABLED");
    assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let create = &plan.mutate_operations[0]["adGroupOperation"]["create"];
    assert_eq!(create["status"], "ENABLED");
}
