//! Default RSA draft must:
//! - send `status: "PAUSED"` in the mutate payload
//! - echo `status_after_apply: "PAUSED"` in the response
//! - include a `next_action_hint` pointing at the `enable_entity` MCP tool

use mcp_google_ads::config::Config;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::ads_write::{draft_responsive_search_ad, DraftRsaParams};

#[test]
fn default_status_is_paused_in_payload_and_response() {
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
        status: None,
    })
    .unwrap();

    // Response visibility.
    assert_eq!(preview["status_after_apply"], "PAUSED");
    assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
    assert_eq!(preview["next_action_hint"]["params"]["entity_type"], "ad");

    // Payload inspection: the stored plan's mutate_operations must carry
    // PAUSED in the adGroupAdOperation.create.status field.
    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    assert_eq!(plan.mutate_operations.len(), 1);
    let op = &plan.mutate_operations[0];
    assert_eq!(
        op["adGroupAdOperation"]["create"]["status"], "PAUSED",
        "default RSA payload must carry status=PAUSED"
    );
}
