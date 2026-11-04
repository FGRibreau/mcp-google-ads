//! `set_campaign_conversion_goals` sets what a campaign optimises on: the
//! account goals, its own (category, origin) goals, or an existing custom goal.
//!
//! Asserts the request that reaches the wire — one atomic mutate carrying the
//! campaign's goal config and each goal's `biddable`, under the resource names
//! Google gives them — and every rejection of an inconsistent request.

mod common;

use common::test_config;
use mcp_google_ads::models::{
    CampaignConversionGoalInput, ConversionActionCategory, ConversionOrigin, GoalConfigLevel,
};
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use mcp_google_ads::tools::conversion_goals_write::set_campaign_conversion_goals;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn goal(category: ConversionActionCategory, biddable: bool) -> CampaignConversionGoalInput {
    CampaignConversionGoalInput {
        category,
        origin: ConversionOrigin::Website,
        biddable,
    }
}

#[tokio::test]
async fn campaign_goals_travel_as_one_mutate_with_the_goal_config() {
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
    let preview = set_campaign_conversion_goals(
        &config,
        "123-456-7890",
        "24358622282",
        GoalConfigLevel::Campaign,
        &[
            goal(ConversionActionCategory::Purchase, true),
            goal(ConversionActionCategory::AddToCart, false),
        ],
        None,
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
    assert_eq!(requests.len(), 1, "one atomic mutate");
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let ops = body["mutateOperations"].as_array().unwrap();
    assert_eq!(ops.len(), 3);

    let config_op = &ops[0]["conversionGoalCampaignConfigOperation"];
    assert_eq!(
        config_op["update"]["resourceName"],
        "customers/1234567890/conversionGoalCampaignConfigs/24358622282"
    );
    assert_eq!(config_op["update"]["goalConfigLevel"], "CAMPAIGN");
    assert_eq!(config_op["update"]["customConversionGoal"], "");
    assert_eq!(
        config_op["updateMask"],
        "goalConfigLevel,customConversionGoal"
    );

    let purchase = &ops[1]["campaignConversionGoalOperation"];
    assert_eq!(
        purchase["update"]["resourceName"],
        "customers/1234567890/campaignConversionGoals/24358622282~PURCHASE~WEBSITE"
    );
    assert_eq!(purchase["update"]["biddable"], true);
    assert_eq!(purchase["updateMask"], "biddable");
    assert_eq!(
        ops[2]["campaignConversionGoalOperation"]["update"]["resourceName"],
        "customers/1234567890/campaignConversionGoals/24358622282~ADD_TO_CART~WEBSITE"
    );
    assert_eq!(
        ops[2]["campaignConversionGoalOperation"]["update"]["biddable"],
        false
    );
}

#[test]
fn an_existing_custom_goal_is_set_on_the_goal_config() {
    let config = test_config();
    let preview = set_campaign_conversion_goals(
        &config,
        "1234567890",
        "777",
        GoalConfigLevel::Campaign,
        &[],
        Some("4521"),
    )
    .unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    assert_eq!(plan.mutate_operations.len(), 1);
    assert_eq!(
        plan.mutate_operations[0]["conversionGoalCampaignConfigOperation"]["update"]
            ["customConversionGoal"],
        "customers/1234567890/customConversionGoals/4521"
    );
}

#[test]
fn following_the_account_goals_clears_any_custom_goal() {
    let config = test_config();
    let preview = set_campaign_conversion_goals(
        &config,
        "1234567890",
        "777",
        GoalConfigLevel::Customer,
        &[],
        None,
    )
    .unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    let update = &plan.mutate_operations[0]["conversionGoalCampaignConfigOperation"]["update"];
    assert_eq!(update["goalConfigLevel"], "CUSTOMER");
    assert_eq!(update["customConversionGoal"], "");
}

#[test]
fn an_inconsistent_request_is_rejected() {
    let config = test_config();
    let purchase = [goal(ConversionActionCategory::Purchase, true)];
    let not_biddable = [goal(ConversionActionCategory::Purchase, false)];
    let twice = [
        goal(ConversionActionCategory::Purchase, true),
        goal(ConversionActionCategory::Purchase, false),
    ];
    for (level, goals, custom, expected) in [
        (
            GoalConfigLevel::Customer,
            &purchase[..],
            None,
            "pass no campaign_conversion_goals",
        ),
        (
            GoalConfigLevel::Customer,
            &[][..],
            Some("4521"),
            "needs goal_config_level CAMPAIGN",
        ),
        (
            GoalConfigLevel::Campaign,
            &purchase[..],
            Some("4521"),
            "not both",
        ),
        (
            GoalConfigLevel::Campaign,
            &not_biddable[..],
            None,
            "at least one campaign conversion goal is biddable",
        ),
        (
            GoalConfigLevel::Campaign,
            &[][..],
            None,
            "at least one campaign conversion goal is biddable",
        ),
        (
            GoalConfigLevel::Campaign,
            &twice[..],
            None,
            "PURCHASE/WEBSITE appears twice",
        ),
        (
            GoalConfigLevel::Campaign,
            &[][..],
            Some(" "),
            "must not be empty",
        ),
    ] {
        let err = set_campaign_conversion_goals(&config, "1234567890", "777", level, goals, custom)
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{level:?} {custom:?}: {err}");
    }
}
