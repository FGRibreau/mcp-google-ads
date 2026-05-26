//! `create_pmax_campaign(start_paused=false)` must produce a payload with
//! `status: "ENABLED"`. v0.2.x ignored the param and always shipped PAUSED.

use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::pmax::{create_pmax_campaign, CreatePmaxCampaignParams};

fn base(config: &Config, start_paused: bool) -> CreatePmaxCampaignParams<'_> {
    CreatePmaxCampaignParams {
        config,
        customer_id: "123-456-7890",
        campaign_name: "PMax Test",
        daily_budget: 10.0,
        bidding_strategy: "MAXIMIZE_CONVERSIONS",
        final_urls: vec!["https://example.com".into()],
        headlines: vec!["H1".into(), "H2".into(), "H3".into()],
        long_headlines: vec!["Long H1".into()],
        descriptions: vec!["D1".into(), "D2".into()],
        business_name: "BizCo",
        geo_target_ids: vec!["2840".into()],
        start_paused,
    }
}

#[test]
fn start_paused_false_sets_enabled_in_campaign_payload() {
    let config = Config::default();
    let preview = create_pmax_campaign(&base(&config, false)).unwrap();
    assert_eq!(preview["status_after_apply"], "ENABLED");

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();

    // The campaign create op is the second one (index 1) after the budget.
    let campaign_create = &plan.mutate_operations[1]["campaignOperation"]["create"];
    assert_eq!(
        campaign_create["status"], "ENABLED",
        "PMax campaign must carry ENABLED in payload when start_paused=false"
    );
    // The asset group too — inherits the campaign-level status.
    let asset_group_create = plan
        .mutate_operations
        .iter()
        .find_map(|op| op.get("assetGroupOperation").and_then(|v| v.get("create")))
        .expect("asset group create op present");
    assert_eq!(asset_group_create["status"], "ENABLED");
}

#[test]
fn start_paused_true_sets_paused_in_campaign_payload() {
    let config = Config::default();
    let preview = create_pmax_campaign(&base(&config, true)).unwrap();
    assert_eq!(preview["status_after_apply"], "PAUSED");

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let campaign_create = &plan.mutate_operations[1]["campaignOperation"]["create"];
    assert_eq!(campaign_create["status"], "PAUSED");
}
