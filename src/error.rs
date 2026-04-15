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
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, McpGoogleAdsError>;
