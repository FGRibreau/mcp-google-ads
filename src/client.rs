use serde::{Deserialize, Serialize};

use crate::auth;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};

const BASE_URL: &str = "https://googleads.googleapis.com/v23";

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

/// Google Ads REST API client.
pub struct GoogleAdsClient {
    http: reqwest::Client,
    config: Config,
}

impl GoogleAdsClient {
    /// Create a new Google Ads API client.
    pub fn new(config: &Config) -> Result<Self> {
        let http = reqwest::Client::builder().build().map_err(|e| {
            McpGoogleAdsError::Config(format!("Failed to build HTTP client: {}", e))
        })?;

        Ok(Self {
            http,
            config: config.clone(),
        })
    }

    /// Strip dashes from a customer ID (e.g., "123-456-7890" -> "1234567890").
    pub fn normalize_customer_id(id: &str) -> String {
        id.replace('-', "")
    }

    /// Build common headers for Google Ads API requests.
    async fn build_headers(&self) -> Result<reqwest::header::HeaderMap> {
        let token = auth::get_access_token(&self.config).await?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", token)
                .parse()
                .map_err(|e| McpGoogleAdsError::Auth(format!("Invalid token header: {}", e)))?,
        );
        headers.insert(
            "developer-token",
            self.config.ads.developer_token.parse().map_err(|e| {
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

    /// Execute a GAQL query via the Google Ads search endpoint.
    /// Handles pagination automatically.
    pub async fn search(&self, customer_id: &str, query: &str) -> Result<Vec<serde_json::Value>> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!("{}/customers/{}/googleAds:search", BASE_URL, normalized_id);
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
            BASE_URL, normalized_id
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

    /// Execute a mutate request against the Google Ads API.
    pub async fn mutate(
        &self,
        customer_id: &str,
        operations: Vec<MutateOperation>,
    ) -> Result<MutateResponse> {
        let normalized_id = Self::normalize_customer_id(customer_id);
        let url = format!("{}/customers/{}/googleAds:mutate", BASE_URL, normalized_id);
        let headers = self.build_headers().await?;

        let body = serde_json::json!({
            "mutateOperations": operations,
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

        let mutate_response: MutateResponse = response.json().await?;
        Ok(mutate_response)
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
}
