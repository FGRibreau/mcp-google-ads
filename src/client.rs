use serde::{Deserialize, Serialize};

use crate::auth;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};

/// Default Google Ads REST API base URL.
///
/// Overridable at construction via [`GoogleAdsClient::with_base_url`] or via
/// the `GOOGLE_ADS_API_BASE_URL` environment variable. The override is the
/// hook used by integration tests to point the client at a `wiremock` server.
pub const DEFAULT_BASE_URL: &str = "https://googleads.googleapis.com/v25";

/// Environment variable that, when set, overrides the API base URL.
///
/// Setting this to `__test__` activates a stubbed access token path that
/// bypasses the OAuth2 flow entirely — used by `wiremock`-driven tests so
/// they don't need credentials files.
pub const BASE_URL_ENV: &str = "GOOGLE_ADS_API_BASE_URL";

/// Whitelist of top-level `MutateOperation` keys accepted by Google Ads v25.
///
/// `mutate_operations` payloads with any key not in this list are rejected
/// client-side before any HTTP traffic — this is the guard that catches
/// `dismissRecommendationOperation` / `applyRecommendationOperation` mistakes
/// (those operations live on dedicated RPCs, not on `googleAds:mutate`).
///
/// Source: Google Ads API v25 `MutateOperation.operation` oneof definition:
/// <https://developers.google.com/google-ads/api/reference/rpc/v25/MutateOperation>.
pub const VALID_MUTATE_OPERATION_KEYS: &[&str] = &[
    "adGroupAdLabelOperation",
    "adGroupAdOperation",
    "adGroupAssetOperation",
    "adGroupBidModifierOperation",
    "adGroupCriterionCustomizerOperation",
    "adGroupCriterionLabelOperation",
    "adGroupCriterionOperation",
    "adGroupCustomizerOperation",
    "adGroupLabelOperation",
    "adGroupOperation",
    "adOperation",
    "adParameterOperation",
    "assetGroupAssetOperation",
    "assetGroupListingGroupFilterOperation",
    "assetGroupOperation",
    "assetGroupSignalOperation",
    "assetOperation",
    "assetSetAssetOperation",
    "assetSetOperation",
    "audienceOperation",
    "biddingDataExclusionOperation",
    "biddingSeasonalityAdjustmentOperation",
    "biddingStrategyOperation",
    "bookCampaignsOperation",
    "campaignAssetOperation",
    "campaignAssetSetOperation",
    "campaignBidModifierOperation",
    "campaignBudgetOperation",
    "campaignConversionGoalOperation",
    "campaignCriterionOperation",
    "campaignCustomizerOperation",
    "campaignDraftOperation",
    "campaignGroupOperation",
    "campaignLabelOperation",
    "campaignOperation",
    "campaignSharedSetOperation",
    "conversionActionOperation",
    "conversionCustomVariableOperation",
    "conversionGoalCampaignConfigOperation",
    "conversionValueRuleOperation",
    "conversionValueRuleSetOperation",
    "customConversionGoalOperation",
    "customerAssetOperation",
    "customerConversionGoalOperation",
    "customerCustomizerOperation",
    "customerLabelOperation",
    "customerNegativeCriterionOperation",
    "customerOperation",
    "customizerAttributeOperation",
    "experimentArmOperation",
    "experimentOperation",
    "keywordPlanAdGroupKeywordOperation",
    "keywordPlanAdGroupOperation",
    "keywordPlanCampaignKeywordOperation",
    "keywordPlanCampaignOperation",
    "keywordPlanOperation",
    "labelOperation",
    "quoteCampaignsOperation",
    "recommendationSubscriptionOperation",
    "remarketingActionOperation",
    "sharedCriterionOperation",
    "sharedSetOperation",
    "smartCampaignSettingOperation",
    "userListOperation",
];

/// A single mutate operation for the Google Ads API.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MutateOperation {
    #[serde(flatten)]
    pub operation: serde_json::Value,
}

/// Response from a mutate request.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MutateResponse {
    #[serde(default)]
    pub mutate_operation_responses: Vec<serde_json::Value>,
    #[serde(default)]
    pub partial_failure_error: Option<serde_json::Value>,
}

/// Response from `recommendations:apply`.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRecommendationResponse {
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
    #[serde(default)]
    pub partial_failure_error: Option<serde_json::Value>,
}

/// Response from `recommendations:dismiss`.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DismissRecommendationResponse {
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
}

/// Google Ads REST API client.
pub struct GoogleAdsClient {
    http: reqwest::Client,
    config: Config,
    base_url: String,
}

impl GoogleAdsClient {
    /// Create a new Google Ads API client using the configured base URL.
    ///
    /// The base URL is resolved from the `GOOGLE_ADS_API_BASE_URL` env var if
    /// set, otherwise falls back to [`DEFAULT_BASE_URL`].
    pub fn new(config: &Config) -> Result<Self> {
        let base_url = std::env::var(BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(config, base_url)
    }

    /// Create a new client with an explicit base URL — used by tests to
    /// point at a `wiremock` server.
    pub fn with_base_url(config: &Config, base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder().build().map_err(|e| {
            McpGoogleAdsError::Config(format!("Failed to build HTTP client: {}", e))
        })?;

        Ok(Self {
            http,
            config: config.clone(),
            base_url: base_url.into(),
        })
    }

    /// The base URL this client is targeting (for tests/diagnostics).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Strip dashes from a customer ID (e.g., "123-456-7890" -> "1234567890").
    pub fn normalize_customer_id(id: &str) -> String {
        id.replace('-', "")
    }

    /// Build common headers for Google Ads API requests.
    ///
    /// When the base URL points anywhere other than the real Google Ads host
    /// (e.g. a `wiremock` server) and no credentials file exists, this falls
    /// back to a static dummy bearer token so tests don't need an OAuth2 flow.
    async fn build_headers(&self) -> Result<reqwest::header::HeaderMap> {
        let token = if self.is_test_base_url() && !self.config.google.credentials_path.exists() {
            "test-access-token".to_string()
        } else {
            auth::get_access_token(&self.config).await?
        };

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", token)
                .parse()
                .map_err(|e| McpGoogleAdsError::Auth(format!("Invalid token header: {}", e)))?,
        );
        let dev_token = if self.config.ads.developer_token.is_empty() {
            "test-developer-token".to_string()
        } else {
            self.config.ads.developer_token.clone()
        };
        headers.insert(
            "developer-token",
            dev_token.parse().map_err(|e| {
                McpGoogleAdsError::Config(format!("Invalid developer token: {}", e))
            })?,
        );

        if let Some(ref login_customer_id) = self.config.ads.login_customer_id {
            let normalized = Self::normalize_customer_id(login_customer_id);
            headers.insert(
                "login-customer-id",
                normalized.parse().map_err(|e| {
                    McpGoogleAdsError::Config(format!("Invalid login customer ID: {}", e))
                })?,
            );
        }

        Ok(headers)
    }

    fn is_test_base_url(&self) -> bool {
        !self
            .base_url
            .starts_with("https://googleads.googleapis.com")
    }

    /// Execute a GAQL query via the Google Ads search endpoint.
    /// Handles pagination automatically.
    pub async fn search(&self, customer_id: &str, query: &str) -> Result<Vec<serde_json::Value>> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!(
            "{}/customers/{}/googleAds:search",
            self.base_url, normalized_id
        );
        let headers = self.build_headers().await?;

        let mut all_results = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut body = serde_json::json!({
                "query": query,
            });

            if let Some(ref token) = page_token {
                body.as_object_mut()
                    .ok_or_else(|| {
                        McpGoogleAdsError::Json(serde_json::Error::io(std::io::Error::other(
                            "Failed to build request body",
                        )))
                    })?
                    .insert(
                        "pageToken".to_string(),
                        serde_json::Value::String(token.clone()),
                    );
            }

            let response = self
                .http
                .post(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let error_body = response.text().await.unwrap_or_default();
                return Err(parse_google_ads_error(status, &error_body));
            }

            let response_json: serde_json::Value = response.json().await?;

            if let Some(results) = response_json.get("results").and_then(|r| r.as_array()) {
                all_results.extend(results.iter().cloned());
            }

            match response_json.get("nextPageToken").and_then(|t| t.as_str()) {
                Some(next_token) => {
                    page_token = Some(next_token.to_string());
                }
                None => break,
            }
        }

        Ok(all_results)
    }

    /// Call the Keyword Planner generateKeywordIdeas endpoint.
    pub async fn generate_keyword_ideas(
        &self,
        customer_id: &str,
        seed_keywords: Vec<String>,
        page_size: Option<u32>,
    ) -> Result<Vec<serde_json::Value>> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!(
            "{}/customers/{}:generateKeywordIdeas",
            self.base_url, normalized_id
        );
        let headers = self.build_headers().await?;

        let body = serde_json::json!({
            "keywordSeed": {
                "keywords": seed_keywords
            },
            "language": "languageConstants/1000",
            "pageSize": page_size.unwrap_or(50),
            "keywordPlanNetwork": "GOOGLE_SEARCH"
        });

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_google_ads_error(status, &error_body));
        }

        let response_json: serde_json::Value = response.json().await?;
        let results = response_json
            .get("results")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(results)
    }

    /// Validate that every operation in `operations` uses a top-level key
    /// from [`VALID_MUTATE_OPERATION_KEYS`]. Returns the first offending key.
    pub fn validate_mutate_operations(
        operations: &[MutateOperation],
    ) -> std::result::Result<(), String> {
        for (idx, op) in operations.iter().enumerate() {
            let obj = match op.operation.as_object() {
                Some(o) => o,
                None => {
                    return Err(format!(
                        "MutateOperation at index {} is not a JSON object",
                        idx
                    ));
                }
            };
            for key in obj.keys() {
                if !VALID_MUTATE_OPERATION_KEYS.contains(&key.as_str()) {
                    return Err(format!(
                        "Unknown MutateOperation key '{}' at index {}. Recommendation operations \
                         must use apply_recommendations / dismiss_recommendations — they are NOT \
                         valid keys on googleAds:mutate in v25.",
                        key, idx
                    ));
                }
            }
        }
        Ok(())
    }

    /// Execute a mutate request against the Google Ads API.
    ///
    /// Rejects unknown top-level operation keys client-side before any HTTP
    /// traffic (see [`Self::validate_mutate_operations`]).
    pub async fn mutate(
        &self,
        customer_id: &str,
        operations: Vec<MutateOperation>,
    ) -> Result<MutateResponse> {
        Self::validate_mutate_operations(&operations).map_err(McpGoogleAdsError::Validation)?;

        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!(
            "{}/customers/{}/googleAds:mutate",
            self.base_url, normalized_id
        );
        let headers = self.build_headers().await?;

        let body = serde_json::json!({
            "mutateOperations": operations,
            // Atomic on purpose: with partialFailure=true Google COMMITS the
            // operations that succeed and only reports errors for the rest, so a
            // failed multi-operation plan (e.g. draft_campaign's budget + campaign
            // + ad group chain) leaves orphans behind — and since we surface any
            // partial failure as a full error and keep the plan for retry, each
            // retry would commit another orphan. With partialFailure=false any
            // failing operation aborts the whole request and nothing is written.
            "partialFailure": false,
        });

        // Debug: write request body to /tmp/mcp-google-ads-last-request.json for inspection
        if let Ok(body_pretty) = serde_json::to_string_pretty(&body) {
            let _ = std::fs::write("/tmp/mcp-google-ads-last-request.json", &body_pretty);
        }

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            // Debug: write full error body for inspection
            let _ = std::fs::write("/tmp/mcp-google-ads-last-error.json", &error_body);
            return Err(parse_google_ads_error(status, &error_body));
        }

        let mutate_response: MutateResponse = response.json().await?;
        Ok(mutate_response)
    }

    /// Apply recommendations via the dedicated `recommendations:apply` RPC.
    ///
    /// `resource_names` must be full resource paths:
    /// `customers/{cid}/recommendations/{recommendation_id}`.
    pub async fn apply_recommendations(
        &self,
        customer_id: &str,
        resource_names: Vec<String>,
    ) -> Result<ApplyRecommendationResponse> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!(
            "{}/customers/{}/recommendations:apply",
            self.base_url, normalized_id
        );
        let headers = self.build_headers().await?;

        let operations: Vec<serde_json::Value> = resource_names
            .iter()
            .map(|rn| serde_json::json!({ "resourceName": rn }))
            .collect();

        let body = serde_json::json!({
            "operations": operations,
            "partialFailure": true,
        });

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_google_ads_error(status, &error_body));
        }

        let parsed: ApplyRecommendationResponse = response.json().await?;
        Ok(parsed)
    }

    /// Dismiss recommendations via the dedicated `recommendations:dismiss` RPC.
    pub async fn dismiss_recommendations(
        &self,
        customer_id: &str,
        resource_names: Vec<String>,
    ) -> Result<DismissRecommendationResponse> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!(
            "{}/customers/{}/recommendations:dismiss",
            self.base_url, normalized_id
        );
        let headers = self.build_headers().await?;

        let operations: Vec<serde_json::Value> = resource_names
            .iter()
            .map(|rn| serde_json::json!({ "resourceName": rn }))
            .collect();

        let body = serde_json::json!({
            "operations": operations,
        });

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_google_ads_error(status, &error_body));
        }

        let parsed: DismissRecommendationResponse = response.json().await?;
        Ok(parsed)
    }
}

/// Parse a Google Ads API error response into a McpGoogleAdsError.
fn parse_google_ads_error(status: reqwest::StatusCode, body: &str) -> McpGoogleAdsError {
    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(body);

    match parsed {
        Ok(json) => {
            let error_obj = json.get("error");

            let message = error_obj
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown Google Ads API error")
                .to_string();

            let error_code = error_obj
                .and_then(|e| e.get("status"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());

            let details = error_obj
                .and_then(|e| e.get("details"))
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|d| serde_json::to_string(d).ok())
                        .collect()
                })
                .unwrap_or_default();

            McpGoogleAdsError::GoogleAds {
                message: format!("[{}] {}", status, message),
                error_code,
                details,
            }
        }
        Err(_) => McpGoogleAdsError::GoogleAds {
            message: format!("[{}] {}", status, body),
            error_code: None,
            details: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_customer_id() {
        assert_eq!(
            GoogleAdsClient::normalize_customer_id("123-456-7890"),
            "1234567890"
        );
        assert_eq!(
            GoogleAdsClient::normalize_customer_id("1234567890"),
            "1234567890"
        );
    }

    #[test]
    fn test_parse_google_ads_error_json() {
        let body = r#"{"error":{"message":"Request had invalid authentication credentials.","status":"UNAUTHENTICATED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo"}]}}"#;
        let err = parse_google_ads_error(reqwest::StatusCode::UNAUTHORIZED, body);
        match err {
            McpGoogleAdsError::GoogleAds {
                message,
                error_code,
                details,
            } => {
                assert!(message.contains("invalid authentication"));
                assert_eq!(error_code, Some("UNAUTHENTICATED".to_string()));
                assert_eq!(details.len(), 1);
            }
            _ => panic!("Expected GoogleAds error"),
        }
    }

    #[test]
    fn test_parse_google_ads_error_plain() {
        let err = parse_google_ads_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "something broke",
        );
        match err {
            McpGoogleAdsError::GoogleAds {
                message,
                error_code,
                details,
            } => {
                assert!(message.contains("something broke"));
                assert!(error_code.is_none());
                assert!(details.is_empty());
            }
            _ => panic!("Expected GoogleAds error"),
        }
    }

    #[test]
    fn test_validate_mutate_operations_accepts_known_keys() {
        let ops = vec![
            MutateOperation {
                operation: serde_json::json!({"adGroupOperation": {"create": {}}}),
            },
            MutateOperation {
                operation: serde_json::json!({"campaignBudgetOperation": {"create": {}}}),
            },
        ];
        assert!(GoogleAdsClient::validate_mutate_operations(&ops).is_ok());
    }

    #[test]
    fn test_validate_mutate_operations_rejects_recommendation_ops() {
        let ops = vec![MutateOperation {
            operation: serde_json::json!({"dismissRecommendationOperation": {"resourceName": "x"}}),
        }];
        let err = GoogleAdsClient::validate_mutate_operations(&ops).unwrap_err();
        assert!(err.contains("dismissRecommendationOperation"));
        assert!(err.contains("apply_recommendations / dismiss_recommendations"));
    }

    #[test]
    fn test_validate_mutate_operations_rejects_apply_recommendation() {
        let ops = vec![MutateOperation {
            operation: serde_json::json!({"applyRecommendationOperation": {"resourceName": "x"}}),
        }];
        assert!(GoogleAdsClient::validate_mutate_operations(&ops).is_err());
    }

    #[test]
    fn test_with_base_url() {
        let cfg = Config::default();
        let client = GoogleAdsClient::with_base_url(&cfg, "http://localhost:9999/v25").unwrap();
        assert_eq!(client.base_url(), "http://localhost:9999/v25");
    }
}
