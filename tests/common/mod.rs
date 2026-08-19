//! Shared helpers for wiremock-driven integration tests.
//!
//! These tests run without any real Google Ads credentials — the client's
//! `GOOGLE_ADS_API_BASE_URL` override + the in-process auth bypass (when no
//! credentials file is present) means we can stub the entire HTTP layer.

#![allow(dead_code)]

use mcp_google_ads::client::GoogleAdsClient;
use mcp_google_ads::config::Config;
use std::path::PathBuf;
use wiremock::MockServer;

/// Build a test [`Config`] pointing at non-existent credentials paths so the
/// client falls through to its test bearer-token bypass.
pub fn test_config() -> Config {
    Config {
        google: mcp_google_ads::config::GoogleConfig {
            credentials_path: PathBuf::from("/tmp/__nonexistent_creds__.json"),
            token_path: PathBuf::from("/tmp/__nonexistent_token__.json"),
        },
        ads: mcp_google_ads::config::AdsConfig {
            developer_token: "test-developer-token".to_string(),
            customer_id: "1234567890".to_string(),
            login_customer_id: None,
        },
        safety: mcp_google_ads::config::SafetyConfig {
            max_daily_budget: 10_000.0,
            max_bid_increase_pct: 1_000,
            require_dry_run: false,
            log_file: PathBuf::from("/tmp/__test_audit__.log"),
            blocked_operations: Vec::new(),
        },
        read_only: false,
    }
}

/// Spawn a [`MockServer`] and build a [`GoogleAdsClient`] pointed at it.
///
/// Returns the mock server (keep alive for the duration of the test) and a
/// ready-to-use client.
pub async fn spawn_mock_google_ads() -> (MockServer, GoogleAdsClient) {
    let mock = MockServer::start().await;
    let base_url = format!("{}/v25", mock.uri());
    let config = test_config();
    let client = GoogleAdsClient::with_base_url(&config, base_url).unwrap();
    (mock, client)
}
