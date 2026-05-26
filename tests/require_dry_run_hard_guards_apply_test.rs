//! `config.safety.require_dry_run = true` + `dry_run = false` (without
//! `bypass_require_dry_run = true`) must return `Err(DryRunRequired)` BEFORE
//! any HTTP traffic. v0.2.x emitted a cosmetic warning AFTER applying.

mod common;

use mcp_google_ads::error::McpGoogleAdsError;
use mcp_google_ads::safety::preview::{get_plan, remove_plan, store_plan, ChangePlan};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::any;
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn require_dry_run_true_blocks_apply_with_dryrun_false() {
    // Spawn a mock and assert ZERO requests reach it — the guard must
    // short-circuit before any HTTP call.
    let mock = wiremock::MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    let plan = ChangePlan::new(
        "test_op".into(),
        "campaign".into(),
        "1".into(),
        "1234567890".into(),
        serde_json::json!({}),
        false,
        vec![serde_json::json!({"campaignOperation": {"update": {"resourceName": "x"}}})],
    );
    let plan_id = plan.plan_id.clone();
    store_plan(plan);

    let mut config = common::test_config();
    config.safety.require_dry_run = true;

    let err = confirm_and_apply(
        &config,
        ConfirmApplyInput {
            plan_id: plan_id.clone(),
            dry_run: false,
            bypass_require_dry_run: false,
            confirmed_twice: false,
        },
    )
    .await
    .expect_err("must error out with DryRunRequired");

    assert!(matches!(err, McpGoogleAdsError::DryRunRequired));
    // Plan kept so retry is possible.
    assert!(get_plan(&plan_id).is_some());

    remove_plan(&plan_id);
}
