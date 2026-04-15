use mcp_google_ads::config::Config;
use mcp_google_ads::error::Result;

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
    let server = mcp_google_ads::GoogleAdsMcp::new(config)?;

    let transport = rmcp::transport::io::stdio();

    let service = server.serve(transport).await.map_err(|e| {
        mcp_google_ads::error::McpGoogleAdsError::Config(format!(
            "Failed to start MCP server: {}",
            e
        ))
    })?;

    service.waiting().await.map_err(|e| {
        mcp_google_ads::error::McpGoogleAdsError::Config(format!("Server error: {}", e))
    })?;

    Ok(())
}
