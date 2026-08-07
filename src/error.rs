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

pub type Result<T> = std::result::Result<T, McpGoogleAdsError>;
