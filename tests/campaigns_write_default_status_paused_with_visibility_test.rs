//! `draft_campaign` default status is PAUSED, response carries
//! `status_after_apply` + `next_action_hint`. Opt-in to ENABLED works.

use mcp_google_ads::config::Config;
use mcp_google_ads::models::AdStatus;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::{draft_campaign, DraftCampaignParams};

fn p<'a>(config: &'a Config, status: Option<AdStatus>) -> DraftCampaignParams<'a> {
    DraftCampaignParams {
        config,
        customer_id: "123-456-7890",
        campaign_name: "Campaign One",
        daily_budget: 5.0,
        bidding_strategy: "MAXIMIZE_CONVERSIONS",
        target_cpa: None,
        target_roas: None,
        channel_type: "SEARCH",
        ad_group_name: "AG1",
        keywords: vec![],
        geo_target_ids: vec![],
        language_ids: vec![],
        status,
    }
}

#[test]
fn default_status_paused_visible_in_response_and_payload() {
    let config = Config::default();
    let preview = draft_campaign(&p(&config, None)).unwrap();
    assert_eq!(preview["status_after_apply"], "PAUSED");
    assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let campaign_create = &plan.mutate_operations[1]["campaignOperation"]["create"];
    assert_eq!(campaign_create["status"], "PAUSED");

    // Ad group inherits the campaign-level status.
    let ad_group_create = plan
        .mutate_operations
        .iter()
        .find_map(|op| op.get("adGroupOperation").and_then(|v| v.get("create")))
        .expect("ad group create op present");
    assert_eq!(ad_group_create["status"], "PAUSED");
}

#[test]
fn explicit_enabled_propagates_to_payload() {
    let config = Config::default();
    let preview = draft_campaign(&p(&config, Some(AdStatus::Enabled))).unwrap();
    assert_eq!(preview["status_after_apply"], "ENABLED");
    assert!(preview.get("next_action_hint").is_none() || preview["next_action_hint"].is_null());

    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    let campaign_create = &plan.mutate_operations[1]["campaignOperation"]["create"];
    assert_eq!(campaign_create["status"], "ENABLED");
}
