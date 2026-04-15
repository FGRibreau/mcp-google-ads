//! Integration tests for mcp-google-ads.
//!
//! These tests call the real Google Ads API using a TEST account.
//! They require the following environment variables (note the TEST_ prefix):
//!
//! - GOOGLE_ADS_TEST_DEVELOPER_TOKEN
//! - GOOGLE_ADS_TEST_CUSTOMER_ID
//! - GOOGLE_ADS_TEST_CREDENTIALS_PATH
//! - GOOGLE_ADS_TEST_TOKEN_PATH
//! - GOOGLE_ADS_TEST_LOGIN_CUSTOMER_ID (optional, for MCC)
//!
//! These are intentionally separate from production GOOGLE_ADS_* vars
//! to prevent accidentally running tests against a real account.
//!
//! Run with: cargo test --test integration_test

use mcp_google_ads::client::GoogleAdsClient;
use mcp_google_ads::config::Config;

/// Build a Config from GOOGLE_ADS_TEST_* env vars.
/// Panics immediately with an actionable message if any required var is missing.
fn test_config() -> Config {
    let developer_token = std::env::var("GOOGLE_ADS_TEST_DEVELOPER_TOKEN").unwrap_or_else(|_| {
        panic!(
            "GOOGLE_ADS_TEST_DEVELOPER_TOKEN is not set.\n\
             Integration tests require a Google Ads test account.\n\
             Set GOOGLE_ADS_TEST_* env vars (not GOOGLE_ADS_* — those are for production)."
        )
    });

    let customer_id = std::env::var("GOOGLE_ADS_TEST_CUSTOMER_ID").unwrap_or_else(|_| {
        panic!(
            "GOOGLE_ADS_TEST_CUSTOMER_ID is not set.\n\
             This should be the Customer ID of your Google Ads TEST account (e.g. 123-456-7890)."
        )
    });

    let credentials_path = std::env::var("GOOGLE_ADS_TEST_CREDENTIALS_PATH").unwrap_or_else(|_| {
        panic!(
            "GOOGLE_ADS_TEST_CREDENTIALS_PATH is not set.\n\
             Point this to your OAuth2 Desktop App credentials.json file."
        )
    });

    let token_path = std::env::var("GOOGLE_ADS_TEST_TOKEN_PATH").unwrap_or_else(|_| {
        panic!(
            "GOOGLE_ADS_TEST_TOKEN_PATH is not set.\n\
             Point this to your token.json file containing the refresh_token."
        )
    });

    let login_customer_id = std::env::var("GOOGLE_ADS_TEST_LOGIN_CUSTOMER_ID")
        .ok()
        .filter(|v| !v.is_empty());

    Config {
        google: mcp_google_ads::config::GoogleConfig {
            credentials_path: std::path::PathBuf::from(credentials_path),
            token_path: std::path::PathBuf::from(token_path),
        },
        ads: mcp_google_ads::config::AdsConfig {
            developer_token,
            customer_id,
            login_customer_id,
        },
        safety: mcp_google_ads::config::SafetyConfig::default(),
        read_only: false,
    }
}

fn test_client() -> GoogleAdsClient {
    let config = test_config();
    GoogleAdsClient::new(&config).expect("Failed to create GoogleAdsClient")
}

fn test_customer_id() -> String {
    GoogleAdsClient::normalize_customer_id(
        &std::env::var("GOOGLE_ADS_TEST_CUSTOMER_ID").unwrap(),
    )
}

// ── Connection ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_connection_and_auth() {
    let client = test_client();
    let cid = test_customer_id();

    let result = client
        .search(&cid, "SELECT customer.id FROM customer LIMIT 1")
        .await;

    assert!(
        result.is_ok(),
        "Failed to connect to Google Ads API: {:?}",
        result.err()
    );

    let rows = result.unwrap();
    assert!(
        !rows.is_empty(),
        "Expected at least one result from customer query"
    );
}

// ── Read: Campaigns ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_campaigns() {
    let client = test_client();
    let cid = test_customer_id();

    let result = client
        .search(
            &cid,
            "SELECT campaign.id, campaign.name, campaign.status \
             FROM campaign \
             WHERE campaign.status != 'REMOVED' \
             LIMIT 10",
        )
        .await;

    assert!(
        result.is_ok(),
        "Failed to list campaigns: {:?}",
        result.err()
    );
    // Test account may have 0 campaigns — that's OK, we just verify the query works
}

// ── Read: Keywords ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_keywords() {
    let client = test_client();
    let cid = test_customer_id();

    let result = client
        .search(
            &cid,
            "SELECT ad_group_criterion.keyword.text, \
                    ad_group_criterion.keyword.match_type \
             FROM keyword_view \
             LIMIT 10",
        )
        .await;

    assert!(
        result.is_ok(),
        "Failed to list keywords: {:?}",
        result.err()
    );
}

// ── Read: Geo targets ───────────────────────────────────────────────────

#[tokio::test]
async fn test_search_geo_targets() {
    let client = test_client();
    let cid = test_customer_id();

    let result = client
        .search(
            &cid,
            "SELECT geo_target_constant.id, \
                    geo_target_constant.name, \
                    geo_target_constant.country_code \
             FROM geo_target_constant \
             WHERE geo_target_constant.name LIKE '%France%' \
             LIMIT 5",
        )
        .await;

    assert!(
        result.is_ok(),
        "Failed to search geo targets: {:?}",
        result.err()
    );

    let rows = result.unwrap();
    assert!(
        !rows.is_empty(),
        "Expected at least one geo target matching 'France'"
    );
}

// ── Read: Account hierarchy (if MCC) ────────────────────────────────────

#[tokio::test]
async fn test_list_accounts() {
    let config = test_config();
    let client = GoogleAdsClient::new(&config).expect("Failed to create client");

    // Use login_customer_id (MCC) if available, otherwise customer_id
    let mcc_id = config
        .ads
        .login_customer_id
        .unwrap_or(config.ads.customer_id.clone());
    let mcc_id = GoogleAdsClient::normalize_customer_id(&mcc_id);

    let result = client
        .search(
            &mcc_id,
            "SELECT customer_client.id, customer_client.descriptive_name, \
                    customer_client.status \
             FROM customer_client \
             LIMIT 20",
        )
        .await;

    assert!(
        result.is_ok(),
        "Failed to list accounts: {:?}",
        result.err()
    );

    let rows = result.unwrap();
    assert!(
        !rows.is_empty(),
        "Expected at least one account in the hierarchy"
    );
}

// ── Write: Campaign CRUD (draft → confirm dry_run → cleanup) ────────────

#[tokio::test]
async fn test_draft_campaign_dry_run() {
    let config = test_config();
    let cid = GoogleAdsClient::normalize_customer_id(&config.ads.customer_id);

    // Draft a campaign
    let result = mcp_google_ads::tools::campaigns_write::draft_campaign(
        &mcp_google_ads::tools::campaigns_write::DraftCampaignParams {
            config: &config,
            customer_id: &cid,
            campaign_name: "Integration Test Campaign",
            daily_budget: 10.0,
            bidding_strategy: "MAXIMIZE_CLICKS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "Test Ad Group",
            keywords: vec![],
            geo_target_ids: vec!["2250".to_string()], // France
            language_ids: vec!["1002".to_string()],   // French
        },
    );

    assert!(
        result.is_ok(),
        "Failed to draft campaign: {:?}",
        result.err()
    );

    let preview = result.unwrap();
    let preview_obj: serde_json::Value = serde_json::from_str(&preview.to_string()).unwrap();
    assert_eq!(preview_obj["status"], "PENDING_CONFIRMATION");
    assert!(!preview_obj["plan_id"].as_str().unwrap().is_empty());

    // Confirm with dry_run=true (should NOT create anything)
    let plan_id = preview_obj["plan_id"].as_str().unwrap();
    let dry_result =
        mcp_google_ads::tools::confirm::confirm_and_apply(&config, plan_id, true).await;

    assert!(
        dry_result.is_ok(),
        "Dry run confirm failed: {:?}",
        dry_result.err()
    );

    let dry_output: serde_json::Value =
        serde_json::from_str(&dry_result.unwrap().to_string()).unwrap();
    assert_eq!(dry_output["dry_run"], true);
}

// ── Write: RSA draft (dry run only) ─────────────────────────────────────

#[tokio::test]
async fn test_draft_rsa_preview() {
    let config = test_config();
    let cid = GoogleAdsClient::normalize_customer_id(&config.ads.customer_id);

    let result = mcp_google_ads::tools::ads_write::draft_responsive_search_ad(
        &mcp_google_ads::tools::ads_write::DraftRsaParams {
            config: &config,
            customer_id: &cid,
            ad_group_id: "999999999", // fake ID — we only test the preview, not execution
            headlines: vec![
                "Headline One".to_string(),
                "Headline Two".to_string(),
                "Headline Three".to_string(),
            ],
            descriptions: vec![
                "Description one for the ad".to_string(),
                "Description two for the ad".to_string(),
            ],
            final_url: "https://example.com",
            path1: Some("test"),
            path2: None,
        },
    );

    assert!(result.is_ok(), "Failed to draft RSA: {:?}", result.err());

    let preview = result.unwrap();
    let obj: serde_json::Value = serde_json::from_str(&preview.to_string()).unwrap();
    assert_eq!(obj["status"], "PENDING_CONFIRMATION");
    assert_eq!(obj["operation"], "draft_responsive_search_ad");
}

// ── Safety: Budget cap enforcement ──────────────────────────────────────

#[tokio::test]
async fn test_budget_cap_blocks_excessive_budget() {
    let mut config = test_config();
    config.safety.max_daily_budget = 5.0; // Low cap for testing
    let cid = GoogleAdsClient::normalize_customer_id(&config.ads.customer_id);

    let result = mcp_google_ads::tools::campaigns_write::draft_campaign(
        &mcp_google_ads::tools::campaigns_write::DraftCampaignParams {
            config: &config,
            customer_id: &cid,
            campaign_name: "Should Be Blocked",
            daily_budget: 100.0, // Exceeds 5.0 cap
            bidding_strategy: "MAXIMIZE_CLICKS",
            target_cpa: None,
            target_roas: None,
            channel_type: "SEARCH",
            ad_group_name: "Blocked Group",
            keywords: vec![],
            geo_target_ids: vec!["2250".to_string()],
            language_ids: vec!["1002".to_string()],
        },
    );

    assert!(result.is_err(), "Expected budget cap to block this campaign");
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("exceeds maximum"),
        "Error should mention budget cap: {}",
        err
    );
}

// ── Safety: Read-only mode ──────────────────────────────────────────────

#[test]
fn test_read_only_config() {
    let mut config = test_config();
    config.read_only = true;

    // Just verify it constructs without error — config.read_only is private
    let _server = mcp_google_ads::GoogleAdsMcp::new(config).unwrap();
}

// ── GAQL error handling ─────────────────────────────────────────────────

#[tokio::test]
async fn test_invalid_gaql_returns_error() {
    let client = test_client();
    let cid = test_customer_id();

    let result = client
        .search(&cid, "SELECT nonexistent.field FROM campaign")
        .await;

    assert!(
        result.is_err(),
        "Expected error for invalid GAQL field, got success"
    );
}
