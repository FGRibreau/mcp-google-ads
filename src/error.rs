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
        match self {
            Self::GoogleAds { details, .. } => details
                .iter()
                .flat_map(
                    |detail| match serde_json::from_str::<serde_json::Value>(detail) {
                        Ok(parsed) => {
                            messages_from_failure(&parsed).unwrap_or_else(|| vec![detail.clone()])
                        }
                        Err(_) => vec![detail.clone()],
                    },
                )
                .collect(),
            // A partial failure arrives as HTTP 200 with the real errors buried
            // in `details`. Without this arm the caller only ever saw the
            // top-level sentence, which for an unknown code says nothing at all.
            Self::PartialFailure(payload) => payload
                .get("details")
                .and_then(|d| d.as_array())
                .map(|details| {
                    details
                        .iter()
                        .filter_map(messages_from_failure)
                        .flatten()
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
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

/// Pull the per-operation errors out of one `GoogleAdsFailure` block, tagging
/// each with the request id — the only handle Google support accepts — and
/// with the API version that produced them. Returns `None` when the block is
/// not a failure envelope, so callers can fall back to the raw text.
fn messages_from_failure(detail: &serde_json::Value) -> Option<Vec<String>> {
    let errors = detail.get("errors")?.as_array()?;
    let request_id = detail.get("requestId").and_then(|r| r.as_str());
    let api_version = detail
        .get("@type")
        .and_then(|t| t.as_str())
        .and_then(api_version_from_type);
    Some(
        errors
            .iter()
            .map(|error| format_api_error(error, request_id, api_version))
            .collect(),
    )
}

/// `type.googleapis.com/google.ads.googleads.v25.errors.GoogleAdsFailure` → `v25`.
/// Reading it off the response beats hardcoding a constant: it reports the
/// version Google actually answered in, which is the one that could not name
/// the error code.
fn api_version_from_type(type_url: &str) -> Option<&str> {
    type_url
        .split('.')
        .find(|seg| seg.starts_with('v') && seg[1..].chars().all(|c| c.is_ascii_digit()))
}

/// True when Google could not name the error code — either it sent no code at
/// all, or every field of it degrades to `UNKNOWN`/`UNSPECIFIED`. That happens
/// when the failure belongs to a newer API version than the one requested, and
/// it is precisely the case where the bare message helps nobody.
fn code_is_unnameable(error: &serde_json::Value) -> bool {
    match error.get("errorCode").and_then(|c| c.as_object()) {
        Some(code) if !code.is_empty() => code.values().all(|v| {
            matches!(v.as_str(), Some("UNKNOWN") | Some("UNSPECIFIED"))
                || v.as_object().map(|o| o.is_empty()).unwrap_or(false)
        }),
        _ => true,
    }
}

/// Render the failing field path as `operations[0]` / `campaign.name`.
fn field_path(error: &serde_json::Value) -> Option<String> {
    let elements = error
        .get("location")?
        .get("fieldPathElements")?
        .as_array()?;
    let rendered: Vec<String> = elements
        .iter()
        .filter_map(|el| {
            let name = el.get("fieldName")?.as_str()?;
            Some(match el.get("index").and_then(|i| i.as_i64()) {
                Some(i) => format!("{name}[{i}]"),
                None => name.to_string(),
            })
        })
        .collect();
    match rendered.is_empty() {
        true => None,
        false => Some(rendered.join(".")),
    }
}

/// Render one `GoogleAdsError` as "message [errorCode=value] at path (requestId=…)",
/// expanding the unnameable-code case into something the caller can act on.
fn format_api_error(
    error: &serde_json::Value,
    request_id: Option<&str>,
    api_version: Option<&str>,
) -> String {
    let mut out = error
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();

    if let Some(code) = error.get("errorCode").and_then(|c| c.as_object()) {
        let code = code
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(" [{code}]"));
    }

    if let Some(path) = field_path(error) {
        out.push_str(&format!(" at {path}"));
    }

    if code_is_unnameable(error) {
        let version = api_version.unwrap_or("requested");
        out.push_str(&format!(
            " — Google returned an error code the {version} API version cannot name, \
             so the real reason is hidden rather than absent. The usual cause is a \
             request missing the type-specific parameters that operation requires; \
             quote the request id to Google support to have it named."
        ));
    }

    if let Some(id) = request_id {
        out.push_str(&format!(" (requestId={id})"));
    }

    out
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
