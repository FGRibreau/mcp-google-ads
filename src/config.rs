use std::path::PathBuf;

use crate::error::Result;

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_path_or(key: &str, default: &str) -> PathBuf {
    expand_tilde(&env_or(key, default))
}

fn env_f64_or(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32_or(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool_or(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(default)
}

fn env_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct GoogleConfig {
    pub credentials_path: PathBuf,
    pub token_path: PathBuf,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            credentials_path: expand_tilde("~/.mcp-google-ads/credentials.json"),
            token_path: expand_tilde("~/.mcp-google-ads/token.json"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AdsConfig {
    pub developer_token: String,
    pub customer_id: String,
    pub login_customer_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub max_daily_budget: f64,
    pub max_bid_increase_pct: u32,
    pub require_dry_run: bool,
    pub log_file: PathBuf,
    pub blocked_operations: Vec<String>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            max_daily_budget: 50.0,
            max_bid_increase_pct: 100,
            require_dry_run: true,
            log_file: expand_tilde("~/.mcp-google-ads/audit.log"),
            blocked_operations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub google: GoogleConfig,
    pub ads: AdsConfig,
    pub safety: SafetyConfig,
    /// When true, only READ tools are exposed. All write/mutate tools are hidden.
    pub read_only: bool,
}

impl Config {
    /// Load configuration entirely from environment variables.
    ///
    /// | Env var | Default |
    /// |---|---|
    /// | `GOOGLE_ADS_CREDENTIALS_PATH` | `~/.mcp-google-ads/credentials.json` |
    /// | `GOOGLE_ADS_TOKEN_PATH` | `~/.mcp-google-ads/token.json` |
    /// | `GOOGLE_ADS_DEVELOPER_TOKEN` | (empty) |
    /// | `GOOGLE_ADS_CUSTOMER_ID` | (empty) |
    /// | `GOOGLE_ADS_LOGIN_CUSTOMER_ID` | (none) |
    /// | `GOOGLE_ADS_MAX_DAILY_BUDGET` | `50.0` |
    /// | `GOOGLE_ADS_MAX_BID_INCREASE_PCT` | `100` |
    /// | `GOOGLE_ADS_REQUIRE_DRY_RUN` | `true` |
    /// | `GOOGLE_ADS_AUDIT_LOG` | `~/.mcp-google-ads/audit.log` |
    /// | `GOOGLE_ADS_BLOCKED_OPS` | (empty, comma-separated) |
    /// | `GOOGLE_ADS_READ_ONLY` | `false` |
    pub fn load() -> Result<Self> {
        let login_customer_id = std::env::var("GOOGLE_ADS_LOGIN_CUSTOMER_ID")
            .ok()
            .filter(|v| !v.is_empty());

        Ok(Config {
            google: GoogleConfig {
                credentials_path: env_path_or(
                    "GOOGLE_ADS_CREDENTIALS_PATH",
                    "~/.mcp-google-ads/credentials.json",
                ),
                token_path: env_path_or("GOOGLE_ADS_TOKEN_PATH", "~/.mcp-google-ads/token.json"),
            },
            ads: AdsConfig {
                developer_token: env_or("GOOGLE_ADS_DEVELOPER_TOKEN", ""),
                customer_id: env_or("GOOGLE_ADS_CUSTOMER_ID", ""),
                login_customer_id,
            },
            safety: SafetyConfig {
                max_daily_budget: env_f64_or("GOOGLE_ADS_MAX_DAILY_BUDGET", 50.0),
                max_bid_increase_pct: env_u32_or("GOOGLE_ADS_MAX_BID_INCREASE_PCT", 100),
                require_dry_run: env_bool_or("GOOGLE_ADS_REQUIRE_DRY_RUN", true),
                log_file: env_path_or("GOOGLE_ADS_AUDIT_LOG", "~/.mcp-google-ads/audit.log"),
                blocked_operations: env_list("GOOGLE_ADS_BLOCKED_OPS"),
            },
            read_only: env_bool_or("GOOGLE_ADS_READ_ONLY", false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.safety.max_daily_budget, 50.0);
        assert_eq!(config.safety.max_bid_increase_pct, 100);
        assert!(config.safety.require_dry_run);
        assert!(config.safety.blocked_operations.is_empty());
    }

    #[test]
    fn test_expand_tilde() {
        let path = expand_tilde("~/.mcp-google-ads/config.yaml");
        assert!(!path.to_str().unwrap_or_default().starts_with('~'));
    }

    #[test]
    fn test_expand_tilde_no_tilde() {
        let path = expand_tilde("/absolute/path");
        assert_eq!(path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_env_or_default() {
        assert_eq!(
            env_or("__MCP_TEST_NONEXISTENT_VAR__", "fallback"),
            "fallback"
        );
    }

    #[test]
    fn test_env_f64_or_default() {
        assert_eq!(env_f64_or("__MCP_TEST_NONEXISTENT_VAR__", 42.5), 42.5);
    }

    #[test]
    fn test_env_u32_or_default() {
        assert_eq!(env_u32_or("__MCP_TEST_NONEXISTENT_VAR__", 200), 200);
    }

    #[test]
    fn test_env_bool_or_default() {
        assert!(env_bool_or("__MCP_TEST_NONEXISTENT_VAR__", true));
        assert!(!env_bool_or("__MCP_TEST_NONEXISTENT_VAR__", false));
    }

    #[test]
    fn test_env_list_empty() {
        let list = env_list("__MCP_TEST_NONEXISTENT_VAR__");
        assert!(list.is_empty());
    }

    #[test]
    fn test_load_defaults() {
        // Clear any env vars that could interfere
        std::env::remove_var("GOOGLE_ADS_DEVELOPER_TOKEN");
        std::env::remove_var("GOOGLE_ADS_CUSTOMER_ID");
        std::env::remove_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID");
        std::env::remove_var("GOOGLE_ADS_MAX_DAILY_BUDGET");
        std::env::remove_var("GOOGLE_ADS_MAX_BID_INCREASE_PCT");
        std::env::remove_var("GOOGLE_ADS_REQUIRE_DRY_RUN");
        std::env::remove_var("GOOGLE_ADS_BLOCKED_OPS");

        let config = Config::load().unwrap();
        assert_eq!(config.ads.developer_token, "");
        assert_eq!(config.ads.customer_id, "");
        assert!(config.ads.login_customer_id.is_none());
        assert_eq!(config.safety.max_daily_budget, 50.0);
        assert_eq!(config.safety.max_bid_increase_pct, 100);
        assert!(config.safety.require_dry_run);
        assert!(config.safety.blocked_operations.is_empty());
    }

    #[test]
    fn test_load_from_env() {
        std::env::set_var("GOOGLE_ADS_DEVELOPER_TOKEN", "test-dev-token");
        std::env::set_var("GOOGLE_ADS_CUSTOMER_ID", "123-456-7890");
        std::env::set_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID", "999-888-7777");
        std::env::set_var("GOOGLE_ADS_MAX_DAILY_BUDGET", "200.0");
        std::env::set_var("GOOGLE_ADS_MAX_BID_INCREASE_PCT", "50");
        std::env::set_var("GOOGLE_ADS_REQUIRE_DRY_RUN", "false");
        std::env::set_var("GOOGLE_ADS_BLOCKED_OPS", "delete_campaign,remove_entity");
        std::env::set_var("GOOGLE_ADS_CREDENTIALS_PATH", "/tmp/creds.json");
        std::env::set_var("GOOGLE_ADS_TOKEN_PATH", "/tmp/token.json");
        std::env::set_var("GOOGLE_ADS_AUDIT_LOG", "/tmp/audit.log");

        let config = Config::load().unwrap();
        assert_eq!(config.ads.developer_token, "test-dev-token");
        assert_eq!(config.ads.customer_id, "123-456-7890");
        assert_eq!(
            config.ads.login_customer_id,
            Some("999-888-7777".to_string())
        );
        assert_eq!(config.safety.max_daily_budget, 200.0);
        assert_eq!(config.safety.max_bid_increase_pct, 50);
        assert!(!config.safety.require_dry_run);
        assert_eq!(
            config.safety.blocked_operations,
            vec!["delete_campaign", "remove_entity"]
        );
        assert_eq!(
            config.google.credentials_path,
            PathBuf::from("/tmp/creds.json")
        );
        assert_eq!(config.google.token_path, PathBuf::from("/tmp/token.json"));
        assert_eq!(config.safety.log_file, PathBuf::from("/tmp/audit.log"));

        // Clean up
        std::env::remove_var("GOOGLE_ADS_DEVELOPER_TOKEN");
        std::env::remove_var("GOOGLE_ADS_CUSTOMER_ID");
        std::env::remove_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID");
        std::env::remove_var("GOOGLE_ADS_MAX_DAILY_BUDGET");
        std::env::remove_var("GOOGLE_ADS_MAX_BID_INCREASE_PCT");
        std::env::remove_var("GOOGLE_ADS_REQUIRE_DRY_RUN");
        std::env::remove_var("GOOGLE_ADS_BLOCKED_OPS");
        std::env::remove_var("GOOGLE_ADS_CREDENTIALS_PATH");
        std::env::remove_var("GOOGLE_ADS_TOKEN_PATH");
        std::env::remove_var("GOOGLE_ADS_AUDIT_LOG");
    }

    #[test]
    fn test_bool_parsing_variants() {
        std::env::set_var("__MCP_TEST_BOOL__", "true");
        assert!(env_bool_or("__MCP_TEST_BOOL__", false));
        std::env::set_var("__MCP_TEST_BOOL__", "1");
        assert!(env_bool_or("__MCP_TEST_BOOL__", false));
        std::env::set_var("__MCP_TEST_BOOL__", "yes");
        assert!(env_bool_or("__MCP_TEST_BOOL__", false));
        std::env::set_var("__MCP_TEST_BOOL__", "false");
        assert!(!env_bool_or("__MCP_TEST_BOOL__", true));
        std::env::set_var("__MCP_TEST_BOOL__", "0");
        assert!(!env_bool_or("__MCP_TEST_BOOL__", true));
        std::env::set_var("__MCP_TEST_BOOL__", "no");
        assert!(!env_bool_or("__MCP_TEST_BOOL__", true));
        std::env::remove_var("__MCP_TEST_BOOL__");
    }

    #[test]
    fn test_env_list_parsing() {
        std::env::set_var("__MCP_TEST_LIST__", "a, b , c");
        let list = env_list("__MCP_TEST_LIST__");
        assert_eq!(list, vec!["a", "b", "c"]);
        std::env::remove_var("__MCP_TEST_LIST__");
    }

    #[test]
    fn test_login_customer_id_empty_string() {
        std::env::set_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID", "");
        let config = Config::load().unwrap();
        assert!(config.ads.login_customer_id.is_none());
        std::env::remove_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID");
    }
}
