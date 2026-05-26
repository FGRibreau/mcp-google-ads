//! Opt-in `status=Enabled` must propagate to the payload and the response,
//! and the response must NOT carry a `next_action_hint` (no further step
//! needed when the ad is already serving).

use mcp_google_ads::config::Config;
use mcp_google_ads::models::AdStatus;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ads_write::{draft_responsive_search_ad, DraftRsaParams};

#[test]
fn explicit_enabled_status_propagates_to_payload() {
    let config = Config::default();
    let preview = draft_responsive_search_ad(&DraftRsaParams {
        config: &config,
        customer_id: "123-456-7890",
        ad_group_id: "AG1",
        headlines: vec!["A".into(), "B".into(), "C".into()],
        descriptions: vec!["Desc 1".into(), "Desc 2".into()],
        final_url: "https://example.com",
        path1: None,
        path2: None,
        status: Some(AdStatus::Enabled),
    })
    .unwrap();

    assert_eq!(preview["status_after_apply"], "ENABLED");
    assert!(
        preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null(),
        "no hint should be emitted when the ad is already ENABLED"
    );

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    assert_eq!(
        plan.mutate_operations[0]["adGroupAdOperation"]["create"]["status"], "ENABLED",
        "explicit ENABLED must reach the mutate payload"
    );
}
