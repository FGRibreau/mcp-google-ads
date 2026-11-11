//! Four of the Display awareness tools read the account before they plan:
//! a create that collides with what exists fails the whole atomic mutate, and
//! a repeated field written under a mask replaces what was there. These tests
//! pin the GAQL each read sends and how the rows are parsed, against a mock.

mod common;

use mcp_google_ads::config::Config;
use mcp_google_ads::models::{AudienceTargetingMode, ContentLabelType};
use mcp_google_ads::safety::preview::{get_plan, remove_plan};
use mcp_google_ads::tools::{ad_groups_write, conversion_goals, devices, placements};
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, ResponseTemplate};

const SEARCH_PATH: &str = "/v25/customers/1234567890/googleAds:search";

async fn sent_query(mock: &wiremock::MockServer) -> String {
    let requests = mock.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    body["query"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn account_exclusions_are_read_and_matched() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/1",
                    "type": "PLACEMENT", "placement": {"url": "example.com"}}},
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/2",
                    "type": "MOBILE_APP_CATEGORY",
                    "mobileAppCategory": {"mobileAppCategoryConstant": "mobileAppCategoryConstants/60008"}}},
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/3",
                    "type": "MOBILE_APPLICATION", "mobileApplication": {"appId": "2-com.example.game"}}},
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/4",
                    "type": "CONTENT_LABEL", "contentLabel": {"type": "TRAGEDY"}}},
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/5",
                    "type": "CONTENT_LABEL", "contentLabel": {"type": "UNKNOWN"}}},
                {"customerNegativeCriterion": {
                    "resourceName": "customers/1234567890/customerNegativeCriteria/6",
                    "type": "YOUTUBE_CHANNEL", "youtubeChannel": {"channelId": "UC1"}}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let existing = placements::fetch_account_exclusions(&client, "1234567890")
        .await
        .expect("read");
    assert!(sent_query(&mock)
        .await
        .contains("FROM customer_negative_criterion"));
    assert_eq!(existing.placement_urls, vec!["example.com"]);
    assert_eq!(existing.mobile_app_category_ids, vec!["60008"]);
    assert_eq!(existing.mobile_app_ids, vec!["2-com.example.game"]);
    assert_eq!(existing.content_labels, vec![ContentLabelType::Tragedy]);

    let config = Config::default();
    let preview = placements::add_account_exclusions(
        &config,
        "1234567890",
        &["https://EXAMPLE.com".to_string()],
        &["60008".to_string()],
        &["2-com.example.game".to_string()],
        &[ContentLabelType::Tragedy, ContentLabelType::Juvenile],
        &existing,
    )
    .expect("plan");
    assert_eq!(preview["changes"]["total_new_exclusions"], 1);
    let plan_id = preview["plan_id"].as_str().unwrap();
    let plan = get_plan(plan_id).unwrap();
    assert_eq!(
        plan.mutate_operations,
        vec![json!({
            "customerNegativeCriterionOperation": {
                "create": {"contentLabel": {"type": "JUVENILE"}}
            }
        })]
    );
    remove_plan(plan_id);
}

#[tokio::test]
async fn device_criteria_are_read_for_one_campaign() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                {"campaignCriterion": {
                    "resourceName": "customers/1234567890/campaignCriteria/555~30001",
                    "device": {"type": "MOBILE"}, "bidModifier": 0.8}},
                {"campaignCriterion": {
                    "resourceName": "customers/1234567890/campaignCriteria/555~30000",
                    "device": {"type": "DESKTOP"}}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let existing = devices::fetch_campaign_device_criteria(&client, "1234567890", "555")
        .await
        .expect("read");
    let query = sent_query(&mock).await;
    assert!(query.contains("campaign.id = 555"));
    assert!(query.contains("campaign_criterion.type = 'DEVICE'"));
    assert_eq!(existing.len(), 2);
    assert_eq!(existing[0].device, "MOBILE");
    assert_eq!(existing[0].bid_modifier, Some(0.8));
    assert_eq!(existing[1].bid_modifier, None);
}

#[tokio::test]
async fn target_restrictions_are_read_and_merged() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"adGroup": {
                "resourceName": "customers/1234567890/adGroups/111",
                "id": "111",
                "targetingSetting": {"targetRestrictions": [
                    {"targetingDimension": "AUDIENCE", "bidOnly": true},
                    {"targetingDimension": "TOPIC", "bidOnly": false}
                ]}
            }}]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let current = ad_groups_write::fetch_ad_group_target_restrictions(&client, "1234567890", "111")
        .await
        .expect("read");
    assert!(sent_query(&mock)
        .await
        .contains("ad_group.targeting_setting.target_restrictions"));
    assert_eq!(current.len(), 2);

    let merged =
        ad_groups_write::merge_audience_restriction(&current, AudienceTargetingMode::Targeting);
    assert_eq!(
        merged,
        vec![
            json!({"targetingDimension": "AUDIENCE", "bidOnly": false}),
            json!({"targetingDimension": "TOPIC", "bidOnly": false}),
        ]
    );
}

#[tokio::test]
async fn a_missing_ad_group_is_an_error_and_a_bad_id_sends_nothing() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .expect(1)
        .mount(&mock)
        .await;

    let missing =
        ad_groups_write::fetch_ad_group_target_restrictions(&client, "1234567890", "999").await;
    assert!(missing.unwrap_err().to_string().contains("not found"));

    // A non-numeric ID never reaches GAQL (the mock expects exactly one call).
    let bad =
        ad_groups_write::fetch_ad_group_target_restrictions(&client, "1234567890", "111 OR 1=1")
            .await;
    assert!(bad.unwrap_err().to_string().contains("numeric"));
}

#[tokio::test]
async fn campaign_conversion_goals_are_read_with_false_as_the_default() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(method("POST"))
        .and(path(SEARCH_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                {"campaignConversionGoal": {
                    "resourceName": "customers/1234567890/campaignConversionGoals/555~SUBMIT_LEAD_FORM~WEBSITE",
                    "category": "SUBMIT_LEAD_FORM", "origin": "WEBSITE", "biddable": true}},
                {"campaignConversionGoal": {
                    "resourceName": "customers/1234567890/campaignConversionGoals/555~PAGE_VIEW~WEBSITE",
                    "category": "PAGE_VIEW", "origin": "WEBSITE"}}
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let goals = conversion_goals::fetch_campaign_conversion_goals(&client, "1234567890", "555")
        .await
        .expect("read");
    assert!(sent_query(&mock)
        .await
        .contains("FROM campaign_conversion_goal"));
    assert_eq!(goals.len(), 2);
    assert!(goals[0].biddable);
    assert!(!goals[1].biddable, "an omitted boolean is false");
}

#[tokio::test]
async fn reads_refuse_non_numeric_campaign_ids_before_http() {
    let (mock, client) = common::spawn_mock_google_ads().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;

    assert!(
        devices::fetch_campaign_device_criteria(&client, "1234567890", "Display")
            .await
            .is_err()
    );
    assert!(
        conversion_goals::fetch_campaign_conversion_goals(&client, "1234567890", "1; DROP")
            .await
            .is_err()
    );
}
