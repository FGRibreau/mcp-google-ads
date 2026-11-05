//! `add_ad_group_audiences` writes an ad group's audiences, targeted or
//! excluded, and how its targeted audiences apply (targeting or observation).
//!
//! `add_audience_targeting` only wrote user lists at the campaign level; a
//! Search plan carries its audiences per ad group. Asserts the request that
//! reaches the wire — the AUDIENCE target restriction and one criterion per
//! audience under the right info field and resource — and each rejection.

mod common;

use common::test_config;
use mcp_google_ads::models::AudienceKind;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::audiences::{
    add_ad_group_audiences, resolve_ad_group_target_restrictions, AdGroupAudienceInput,
};
use mcp_google_ads::tools::confirm::{confirm_and_apply, ConfirmApplyInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The restrictions a live Search ad group holds (read on a v25 account), audience included.
fn existing_restrictions() -> Vec<serde_json::Value> {
    serde_json::json!([
        {"bidOnly": true, "targetingDimension": "AGE_RANGE"},
        {"bidOnly": true, "targetingDimension": "GENDER"},
        {"bidOnly": false, "targetingDimension": "AUDIENCE"},
        {"bidOnly": true, "targetingDimension": "PARENTAL_STATUS"},
        {"bidOnly": true, "targetingDimension": "INCOME_RANGE"}
    ])
    .as_array()
    .unwrap()
    .clone()
}

fn audience(kind: AudienceKind, id: &str, negative: bool) -> AdGroupAudienceInput {
    AdGroupAudienceInput {
        kind,
        id: id.to_string(),
        negative,
    }
}

#[tokio::test]
async fn audiences_and_their_targeting_mode_reach_google_in_one_mutate() {
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
    let preview = add_ad_group_audiences(
        &config,
        "123-456-7890",
        "201860356700",
        &[
            audience(AudienceKind::UserInterest, "80432", false),
            audience(AudienceKind::UserList, "6612", true),
            audience(AudienceKind::CustomAudience, "551", false),
            audience(AudienceKind::CombinedAudience, "772", false),
        ],
        Some("OBSERVATION"),
        &existing_restrictions(),
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
    assert_eq!(ops.len(), 5);

    let ad_group = "customers/1234567890/adGroups/201860356700";
    let restriction = &ops[0]["adGroupOperation"];
    assert_eq!(restriction["update"]["resourceName"], ad_group);
    // The other dimensions are sent back unchanged; only AUDIENCE is replaced.
    assert_eq!(
        restriction["update"]["targetingSetting"]["targetRestrictions"],
        serde_json::json!([
            {"bidOnly": true, "targetingDimension": "AGE_RANGE"},
            {"bidOnly": true, "targetingDimension": "GENDER"},
            {"bidOnly": true, "targetingDimension": "PARENTAL_STATUS"},
            {"bidOnly": true, "targetingDimension": "INCOME_RANGE"},
            {"targetingDimension": "AUDIENCE", "bidOnly": true}
        ])
    );
    assert_eq!(
        restriction["updateMask"],
        "targetingSetting.targetRestrictions"
    );

    let created: Vec<&serde_json::Value> = ops[1..]
        .iter()
        .map(|op| &op["adGroupCriterionOperation"]["create"])
        .collect();
    assert!(created.iter().all(|c| c["adGroup"] == ad_group));
    assert_eq!(
        created[0]["userInterest"]["userInterestCategory"],
        "customers/1234567890/userInterests/80432"
    );
    assert_eq!(created[0]["negative"], false);
    assert_eq!(
        created[1]["userList"]["userList"],
        "customers/1234567890/userLists/6612"
    );
    assert_eq!(created[1]["negative"], true);
    assert_eq!(
        created[2]["customAudience"]["customAudience"],
        "customers/1234567890/customAudiences/551"
    );
    assert_eq!(
        created[3]["combinedAudience"]["combinedAudience"],
        "customers/1234567890/combinedAudiences/772"
    );
}

#[test]
fn targeting_restricts_delivery_and_can_be_set_alone() {
    let config = test_config();
    let preview =
        add_ad_group_audiences(&config, "1234567890", "555", &[], Some("TARGETING"), &[]).unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    assert_eq!(plan.mutate_operations.len(), 1);
    assert_eq!(
        plan.mutate_operations[0]["adGroupOperation"]["update"]["targetingSetting"]
            ["targetRestrictions"][0]["bidOnly"],
        false
    );
}

#[test]
fn audiences_alone_keep_the_targeting_setting() {
    let config = test_config();
    let preview = add_ad_group_audiences(
        &config,
        "1234567890",
        "555",
        &[audience(AudienceKind::UserList, "6612", false)],
        None,
        &[],
    )
    .unwrap();
    let plan = get_plan(preview["plan_id"].as_str().unwrap()).unwrap();

    assert_eq!(plan.mutate_operations.len(), 1);
    assert!(plan.mutate_operations[0].get("adGroupOperation").is_none());
}

#[test]
fn an_inconsistent_request_is_rejected() {
    let config = test_config();
    let twice = [
        audience(AudienceKind::UserList, "6612", false),
        audience(AudienceKind::UserList, "6612", true),
    ];
    for (audiences, mode, expected) in [
        (&[][..], None, "Pass audiences, a targeting_mode, or both"),
        (&[][..], Some("RESTRICT"), "Invalid targeting mode"),
        (&twice[..], None, "appears twice"),
        (
            &[audience(AudienceKind::UserList, "userLists/6612", false)][..],
            None,
            "must be numeric",
        ),
    ] {
        let err = add_ad_group_audiences(&config, "1234567890", "555", audiences, mode, &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{expected}: {err}");
    }
}

#[tokio::test]
async fn the_current_restrictions_are_read_from_the_ad_group() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{"adGroup": {
                "resourceName": "customers/1234567890/adGroups/201860356700",
                "targetingSetting": {"targetRestrictions": existing_restrictions()}
            }}]
        })))
        .mount(&mock)
        .await;

    let restrictions = resolve_ad_group_target_restrictions(&client, "1234567890", "201860356700")
        .await
        .unwrap();
    assert_eq!(restrictions, existing_restrictions());

    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("WHERE ad_group.id = 201860356700"),
        "{body}"
    );
}

#[tokio::test]
async fn an_ad_group_without_restrictions_reads_as_an_empty_list() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path("/v25/customers/1234567890/googleAds:search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{"adGroup": {"resourceName": "customers/1234567890/adGroups/555"}}]
        })))
        .mount(&mock)
        .await;

    let restrictions = resolve_ad_group_target_restrictions(&client, "1234567890", "555")
        .await
        .unwrap();
    assert!(restrictions.is_empty());
}
