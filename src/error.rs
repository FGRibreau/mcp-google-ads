use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpGoogleAdsError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Google Ads API error: {message}")]
    GoogleAds {
        message: String,
        error_code: Option<String>,
        details: Vec<String>,
    },
    #[error("Safety violation: {0}")]
    Safety(String),
    /// A mutate was rejected by Google's ad policy. Carries the parsed
    /// violations so the caller can see which text tripped which policy and
    /// whether an exemption can be requested.
    #[error("{message}")]
    PolicyExemption {
        message: String,
        violations: serde_json::Value,
    },
    #[error("Operation failed (partial failure): {0}")]
    PartialFailure(serde_json::Value),
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error(
        "Safety guard: require_dry_run is enabled but dry_run=false. \
         Either run with dry_run=true first, or set bypass_require_dry_run=true \
         to explicitly opt out of the guard for this single apply."
    )]
    DryRunRequired,
    #[error("Safety guard: this plan requires double confirmation. Pass confirmed_twice=true to proceed.")]
    DoubleConfirmRequired,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl McpGoogleAdsError {
    /// The API puts the actionable text (field paths, codes such as
    /// `FIELD_HAS_SUBFIELDS`) in `details`, which the `Display` impl drops.
    /// Pull those out so tool responses can carry them.
    pub fn api_error_messages(&self) -> Vec<String> {
        let Self::GoogleAds { details, .. } = self else {
            return Vec::new();
        };
        details
            .iter()
            .flat_map(
                |detail| match serde_json::from_str::<serde_json::Value>(detail) {
                    Ok(parsed) => match parsed.get("errors").and_then(|e| e.as_array()) {
                        Some(errors) => errors.iter().map(format_api_error).collect(),
                        None => vec![detail.clone()],
                    },
                    Err(_) => vec![detail.clone()],
                },
            )
            .collect()
    }

    /// Error payload for a tool response, including the API's own details.
    pub fn to_json(&self) -> serde_json::Value {
        let mut payload = serde_json::json!({ "error": self.to_string() });
        let api_errors = self.api_error_messages();
        if !api_errors.is_empty() {
            payload["api_errors"] = serde_json::json!(api_errors);
        }
        if let Self::GoogleAds {
            error_code: Some(code),
            ..
        } = self
        {
            payload["error_code"] = serde_json::json!(code);
        }
        payload
    }
}

/// Render one `GoogleAdsError` as "message [errorCode=value]".
fn format_api_error(error: &serde_json::Value) -> String {
    let message = error.get("message").and_then(|m| m.as_str()).unwrap_or("");
    match error.get("errorCode").and_then(|c| c.as_object()) {
        Some(code) => {
            let code = code
                .iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(",");
            format!("{message} [{code}]")
        }
        None => message.to_string(),
    }
}

pub type Result<T> = std::result::Result<T, McpGoogleAdsError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_ads_error_surfaces_api_details() {
        let err = McpGoogleAdsError::GoogleAds {
            message: "[400 Bad Request] Request contains an invalid argument.".to_string(),
            error_code: Some("INVALID_ARGUMENT".to_string()),
            details: vec![r#"{"errors":[{"errorCode":{"fieldMaskError":"FIELD_HAS_SUBFIELDS"},"message":"The field mask updated a field with subfields: 'target_roas'."}]}"#.to_string()],
        };

        let payload = err.to_json();
        assert_eq!(payload["error_code"], "INVALID_ARGUMENT");
        let surfaced = payload["api_errors"][0].as_str().unwrap();
        assert!(surfaced.contains("target_roas"), "got {surfaced}");
        assert!(
            surfaced.contains("fieldMaskError=FIELD_HAS_SUBFIELDS"),
            "got {surfaced}"
        );
    }

    #[test]
    fn non_api_errors_carry_no_details() {
        let err = McpGoogleAdsError::Validation("no changes specified".to_string());
        let payload = err.to_json();
        assert!(payload.get("api_errors").is_none());
        assert!(payload.get("error_code").is_none());
        assert_eq!(payload["error"], "Validation error: no changes specified");
    }
}
