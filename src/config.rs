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

fn env_u16_or(key: &str, default: u16) -> u16 {
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

/// `None` leaves the HTTP transport's own secure default (loopback only) in
/// place. `Some(vec![])` is the sentinel for "accept any `Host` header" (the
/// caller opted in with `*`) — `rmcp`'s `StreamableHttpServerConfig` treats an
/// empty `allowed_hosts` list as "allow all", so this doubles as the exact
/// value that call site needs.
fn env_allowed_hosts(key: &str) -> Option<Vec<String>> {
    let raw = std::env::var(key).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw == "*" {
        return Some(Vec::new());
    }
    Some(raw.split(',').map(|s| s.trim().to_string()).collect())
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

/// Which wire transport the MCP server listens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// stdio (the default) — one client per process, spoken by every MCP host.
    #[default]
    Stdio,
    /// Streamable HTTP, per the MCP spec — lets multiple clients connect over
    /// the network instead of launching a subprocess per client.
    Http,
}

fn env_transport_or(key: &str, default: Transport) -> Transport {
    match std::env::var(key) {
        Ok(v) => match v.to_lowercase().as_str() {
            "stdio" => Transport::Stdio,
            "http" | "streamable-http" | "streamable_http" => Transport::Http,
            _ => default,
        },
        Err(_) => default,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub google: GoogleConfig,
    pub ads: AdsConfig,
    pub safety: SafetyConfig,
    /// When true, only READ tools are exposed. All write/mutate tools are hidden.
    pub read_only: bool,
    /// Which transport `main` serves the MCP protocol over.
    pub transport: Transport,
    /// Bind port for the `Http` transport (ignored for `Stdio`). Always binds
    /// all interfaces (`0.0.0.0`) — this server is meant to sit behind a
    /// container port mapping or reverse proxy, not to pick its own bind IP.
    pub http_port: u16,
    /// Inbound `Host` header allowlist for the `Http` transport (ignored for
    /// `Stdio`). `None` keeps the transport's own secure default (loopback
    /// only: `localhost`, `127.0.0.1`, `::1`) — a DNS-rebinding guard that is
    /// independent of which interface the socket is bound to. `Some(vec![])`
    /// means "accept any Host header" (opted in via `*`).
    pub allowed_hosts: Option<Vec<String>>,
    /// Shared secret every request to the `Http` transport must present in an
    /// `X-API-Key` header (ignored for `Stdio`). `None` (unset or empty)
    /// leaves the transport unauthenticated — anyone who can reach the port
    /// and pass the `Host` allowlist can call every tool.
    pub api_key: Option<String>,
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
    /// | `GOOGLE_ADS_MCP_TRANSPORT` | `stdio` (`stdio` or `http`) |
    /// | `GOOGLE_ADS_MCP_HTTP_PORT` | `8080` |
    /// | `GOOGLE_ADS_MCP_ALLOWED_HOSTS` | (unset — loopback only) |
    /// | `GOOGLE_ADS_MCP_API_KEY` | (unset — unauthenticated) |
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
            transport: env_transport_or("GOOGLE_ADS_MCP_TRANSPORT", Transport::Stdio),
            http_port: env_u16_or("GOOGLE_ADS_MCP_HTTP_PORT", 8080),
            allowed_hosts: env_allowed_hosts("GOOGLE_ADS_MCP_ALLOWED_HOSTS"),
            api_key: std::env::var("GOOGLE_ADS_MCP_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// `Config::load` reads process-wide environment variables, and the harness
    /// runs tests as concurrent threads inside one process. Without
    /// serialisation, one test's `remove_var` lands between another's
    /// `set_var` and its `Config::load`, so the second reads a value it never
    /// set. Every test that drives `Config::load` through the environment takes
    /// this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A panicking test must not wedge the others, so a poisoned lock is
    /// recovered instead of propagated.
    fn env_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
        let _env = env_guard();

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
        let _env = env_guard();

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
        let _env = env_guard();

        std::env::set_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID", "");
        let config = Config::load().unwrap();
        assert!(config.ads.login_customer_id.is_none());
        std::env::remove_var("GOOGLE_ADS_LOGIN_CUSTOMER_ID");
    }

    #[test]
    fn test_transport_defaults_to_stdio() {
        let _env = env_guard();

        std::env::remove_var("GOOGLE_ADS_MCP_TRANSPORT");
        std::env::remove_var("GOOGLE_ADS_MCP_HTTP_PORT");
        let config = Config::load().unwrap();
        assert_eq!(config.transport, Transport::Stdio);
        assert_eq!(config.http_port, 8080);
    }

    #[test]
    fn test_transport_http_variants_and_case_insensitivity() {
        let _env = env_guard();

        for value in ["http", "HTTP", "streamable-http", "streamable_http"] {
            std::env::set_var("GOOGLE_ADS_MCP_TRANSPORT", value);
            assert_eq!(
                env_transport_or("GOOGLE_ADS_MCP_TRANSPORT", Transport::Stdio),
                Transport::Http,
                "value {value:?} should select the Http transport"
            );
        }
        std::env::remove_var("GOOGLE_ADS_MCP_TRANSPORT");
    }

    #[test]
    fn test_transport_unrecognized_value_falls_back_to_default() {
        let _env = env_guard();

        std::env::set_var("GOOGLE_ADS_MCP_TRANSPORT", "carrier-pigeon");
        assert_eq!(
            env_transport_or("GOOGLE_ADS_MCP_TRANSPORT", Transport::Stdio),
            Transport::Stdio
        );
        std::env::remove_var("GOOGLE_ADS_MCP_TRANSPORT");
    }

    #[test]
    fn test_http_port_from_env() {
        let _env = env_guard();

        std::env::set_var("GOOGLE_ADS_MCP_HTTP_PORT", "9090");
        let config = Config::load().unwrap();
        assert_eq!(config.http_port, 9090);
        std::env::remove_var("GOOGLE_ADS_MCP_HTTP_PORT");
    }

    #[test]
    fn test_allowed_hosts_unset_keeps_the_secure_default() {
        let _env = env_guard();

        std::env::remove_var("GOOGLE_ADS_MCP_ALLOWED_HOSTS");
        let config = Config::load().unwrap();
        assert_eq!(config.allowed_hosts, None);
    }

    #[test]
    fn test_allowed_hosts_wildcard_is_the_allow_all_sentinel() {
        std::env::set_var("__MCP_TEST_ALLOWED_HOSTS__", "*");
        assert_eq!(
            env_allowed_hosts("__MCP_TEST_ALLOWED_HOSTS__"),
            Some(Vec::new())
        );
        std::env::remove_var("__MCP_TEST_ALLOWED_HOSTS__");
    }

    #[test]
    fn test_api_key_unset_is_none() {
        let _env = env_guard();

        std::env::remove_var("GOOGLE_ADS_MCP_API_KEY");
        let config = Config::load().unwrap();
        assert_eq!(config.api_key, None);
    }

    #[test]
    fn test_api_key_empty_is_none() {
        let _env = env_guard();

        std::env::set_var("GOOGLE_ADS_MCP_API_KEY", "");
        let config = Config::load().unwrap();
        assert_eq!(config.api_key, None);
        std::env::remove_var("GOOGLE_ADS_MCP_API_KEY");
    }

    #[test]
    fn test_api_key_from_env() {
        let _env = env_guard();

        std::env::set_var("GOOGLE_ADS_MCP_API_KEY", "s3cr3t");
        let config = Config::load().unwrap();
        assert_eq!(config.api_key, Some("s3cr3t".to_string()));
        std::env::remove_var("GOOGLE_ADS_MCP_API_KEY");
    }

    #[test]
    fn test_allowed_hosts_explicit_list() {
        std::env::set_var(
            "__MCP_TEST_ALLOWED_HOSTS__",
            "example.com, example.com:8080",
        );
        assert_eq!(
            env_allowed_hosts("__MCP_TEST_ALLOWED_HOSTS__"),
            Some(vec![
                "example.com".to_string(),
                "example.com:8080".to_string()
            ])
        );
        std::env::remove_var("__MCP_TEST_ALLOWED_HOSTS__");
    }
}
