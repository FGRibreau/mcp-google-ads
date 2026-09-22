use mcp_google_ads::config::{Config, Transport};
use mcp_google_ads::error::{McpGoogleAdsError, Result};

use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting MCP Google Ads server");

    let config = Config::load()?;

    match config.transport {
        Transport::Stdio => run_stdio(config).await,
        Transport::Http => run_http(config).await,
    }
}

async fn run_stdio(config: Config) -> Result<()> {
    let server = mcp_google_ads::GoogleAdsMcp::new(config)?;

    let transport = rmcp::transport::io::stdio();

    let service = server
        .serve(transport)
        .await
        .map_err(|e| McpGoogleAdsError::Config(format!("Failed to start MCP server: {}", e)))?;

    service
        .waiting()
        .await
        .map_err(|e| McpGoogleAdsError::Config(format!("Server error: {}", e)))?;

    Ok(())
}

/// Serves the MCP protocol over streamable HTTP (per the MCP spec) instead of
/// stdio, so multiple network clients can connect to one long-running process
/// instead of each launching their own subprocess.
async fn run_http(config: Config) -> Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let bind_addr = format!("0.0.0.0:{}", config.http_port);

    let http_config = match &config.allowed_hosts {
        None => StreamableHttpServerConfig::default(),
        Some(hosts) if hosts.is_empty() => {
            tracing::warn!(
                "GOOGLE_ADS_MCP_ALLOWED_HOSTS=* — accepting any Host header (DNS-rebinding protection disabled)"
            );
            StreamableHttpServerConfig::default().disable_allowed_hosts()
        }
        Some(hosts) => StreamableHttpServerConfig::default().with_allowed_hosts(hosts.clone()),
    };

    let service = StreamableHttpService::new(
        move || {
            mcp_google_ads::GoogleAdsMcp::new(config.clone())
                .map_err(|e| std::io::Error::other(e.to_string()))
        },
        LocalSessionManager::default().into(),
        http_config,
    );

    let router = axum::Router::new().nest_service("/mcp", service);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| McpGoogleAdsError::Config(format!("Failed to bind {}: {}", bind_addr, e)))?;

    tracing::info!(
        "MCP Google Ads server listening on http://{}/mcp (streamable HTTP)",
        bind_addr
    );

    axum::serve(listener, router)
        .await
        .map_err(|e| McpGoogleAdsError::Config(format!("HTTP server error: {}", e)))?;

    Ok(())
}
