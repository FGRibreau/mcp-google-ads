use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};

const GOOGLE_ADS_SCOPE: &str = "https://www.googleapis.com/auth/adwords";

/// Determine auth mode from credentials file content.
/// Service account JSON files contain a "type": "service_account" field.
/// OAuth2 installed app credentials contain "installed" or "web" top-level keys.
#[derive(Debug)]
enum AuthMode {
    ServiceAccount,
    AuthorizedUser,
}

fn detect_auth_mode(credentials: &serde_json::Value) -> Result<AuthMode> {
    if let Some(t) = credentials.get("type").and_then(|v| v.as_str()) {
        if t == "service_account" {
            return Ok(AuthMode::ServiceAccount);
        }
        if t == "authorized_user" {
            return Ok(AuthMode::AuthorizedUser);
        }
    }

    // Check for OAuth2 installed app format (Google Cloud Console download)
    if credentials.get("installed").is_some() || credentials.get("web").is_some() {
        return Ok(AuthMode::AuthorizedUser);
    }

    Err(McpGoogleAdsError::Auth(
        "Unable to determine auth mode from credentials file. Expected service_account or OAuth2 installed app format.".to_string(),
    ))
}

/// Get an access token for the Google Ads API.
///
/// Supports two modes:
/// - Service account: uses a service account JSON key file
/// - Authorized user: uses client_id/client_secret from credentials.json and refresh_token from token.json
pub async fn get_access_token(config: &Config) -> Result<String> {
    let credentials_path = &config.google.credentials_path;

    if !credentials_path.exists() {
        return Err(McpGoogleAdsError::Auth(format!(
            "Credentials file not found: {}",
            credentials_path.display()
        )));
    }

    let credentials_bytes = tokio::fs::read(credentials_path)
        .await
        .map_err(|e| McpGoogleAdsError::Auth(format!("Failed to read credentials file: {}", e)))?;

    let credentials_json: serde_json::Value = serde_json::from_slice(&credentials_bytes)?;

    let mode = detect_auth_mode(&credentials_json)?;

    match mode {
        AuthMode::ServiceAccount => get_service_account_token(credentials_path).await,
        AuthMode::AuthorizedUser => get_authorized_user_token(config, &credentials_json).await,
    }
}

async fn get_service_account_token(credentials_path: &std::path::Path) -> Result<String> {
    let key = yup_oauth2::read_service_account_key(credentials_path)
        .await
        .map_err(|e| {
            McpGoogleAdsError::Auth(format!("Failed to read service account key: {}", e))
        })?;

    let auth = yup_oauth2::ServiceAccountAuthenticator::builder(key)
        .build()
        .await
        .map_err(|e| {
            McpGoogleAdsError::Auth(format!(
                "Failed to build service account authenticator: {}",
                e
            ))
        })?;

    let token = auth.token(&[GOOGLE_ADS_SCOPE]).await.map_err(|e| {
        McpGoogleAdsError::Auth(format!("Failed to get service account token: {}", e))
    })?;

    token
        .token()
        .map(|t| t.to_string())
        .ok_or_else(|| McpGoogleAdsError::Auth("Service account token was empty".to_string()))
}

async fn get_authorized_user_token(
    config: &Config,
    credentials_json: &serde_json::Value,
) -> Result<String> {
    // Extract client_id and client_secret from credentials.json
    // Format: {"installed": {"client_id": "...", "client_secret": "..."}} or
    //         {"web": {"client_id": "...", "client_secret": "..."}} or
    //         {"type": "authorized_user", "client_id": "...", "client_secret": "...", "refresh_token": "..."}
    let (client_id, client_secret, refresh_token) =
        if let Some(t) = credentials_json.get("type").and_then(|v| v.as_str()) {
            if t == "authorized_user" {
                // Direct authorized_user format (e.g., from gcloud)
                let client_id = credentials_json
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpGoogleAdsError::Auth(
                            "Missing client_id in authorized_user credentials".to_string(),
                        )
                    })?;
                let client_secret = credentials_json
                    .get("client_secret")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpGoogleAdsError::Auth(
                            "Missing client_secret in authorized_user credentials".to_string(),
                        )
                    })?;
                let refresh_token = credentials_json
                    .get("refresh_token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpGoogleAdsError::Auth(
                            "Missing refresh_token in authorized_user credentials".to_string(),
                        )
                    })?;
                (
                    client_id.to_string(),
                    client_secret.to_string(),
                    refresh_token.to_string(),
                )
            } else {
                return Err(McpGoogleAdsError::Auth(format!(
                    "Unexpected credential type: {}",
                    t
                )));
            }
        } else {
            // OAuth2 installed/web app format
            let app_config = credentials_json
                .get("installed")
                .or_else(|| credentials_json.get("web"))
                .ok_or_else(|| {
                    McpGoogleAdsError::Auth(
                        "Credentials file missing 'installed' or 'web' section".to_string(),
                    )
                })?;

            let client_id = app_config
                .get("client_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpGoogleAdsError::Auth("Missing client_id in credentials".to_string())
                })?;
            let client_secret = app_config
                .get("client_secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpGoogleAdsError::Auth("Missing client_secret in credentials".to_string())
                })?;

            // Read refresh_token from token.json
            let token_path = &config.google.token_path;
            if !token_path.exists() {
                return Err(McpGoogleAdsError::Auth(format!(
                    "Token file not found: {}. Run the auth flow first to obtain a refresh token.",
                    token_path.display()
                )));
            }

            let token_bytes = tokio::fs::read(token_path).await.map_err(|e| {
                McpGoogleAdsError::Auth(format!("Failed to read token file: {}", e))
            })?;
            let token_json: serde_json::Value = serde_json::from_slice(&token_bytes)?;
            let refresh_token = token_json
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpGoogleAdsError::Auth("Missing refresh_token in token file".to_string())
                })?;

            (
                client_id.to_string(),
                client_secret.to_string(),
                refresh_token.to_string(),
            )
        };

    let secret = yup_oauth2::authorized_user::AuthorizedUserSecret {
        client_id,
        client_secret,
        refresh_token,
        key_type: "authorized_user".to_string(),
    };

    let auth = yup_oauth2::AuthorizedUserAuthenticator::builder(secret)
        .build()
        .await
        .map_err(|e| {
            McpGoogleAdsError::Auth(format!(
                "Failed to build authorized user authenticator: {}",
                e
            ))
        })?;

    let token = auth
        .token(&[GOOGLE_ADS_SCOPE])
        .await
        .map_err(|e| McpGoogleAdsError::Auth(format!("Failed to get OAuth2 token: {}", e)))?;

    token
        .token()
        .map(|t| t.to_string())
        .ok_or_else(|| McpGoogleAdsError::Auth("OAuth2 token was empty".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_service_account() {
        let creds = json!({"type": "service_account"});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_ok());
        assert!(matches!(mode.unwrap(), AuthMode::ServiceAccount));
    }

    #[test]
    fn test_detect_authorized_user() {
        let creds = json!({"type": "authorized_user"});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_ok());
        assert!(matches!(mode.unwrap(), AuthMode::AuthorizedUser));
    }

    #[test]
    fn test_detect_installed_app() {
        let creds = json!({"installed": {"client_id": "test-id", "client_secret": "test-secret"}});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_ok());
        assert!(matches!(mode.unwrap(), AuthMode::AuthorizedUser));
    }

    #[test]
    fn test_detect_web_app() {
        let creds = json!({"web": {"client_id": "test-id", "client_secret": "test-secret"}});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_ok());
        assert!(matches!(mode.unwrap(), AuthMode::AuthorizedUser));
    }

    #[test]
    fn test_detect_unknown() {
        let creds = json!({"type": "something_else"});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_err());
    }

    #[test]
    fn test_detect_empty() {
        let creds = json!({});
        let mode = detect_auth_mode(&creds);
        assert!(mode.is_err());
    }
}
