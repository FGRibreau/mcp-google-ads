//! `GOOGLE_ADS_MCP_API_KEY` must actually gate the HTTP transport: requests
//! missing (or presenting the wrong) `X-API-Key` header must be rejected
//! before they ever reach the MCP endpoint, and the transport must stay
//! backwards compatible (unauthenticated) when the env var isn't set. This
//! drives the real `axum` + `check_api_key` middleware stack
//! `src/main.rs::run_http` builds, not just the guard function in isolation.

use mcp_google_ads::config::Config;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

async fn spawn_test_server(api_key: Option<String>) -> std::net::SocketAddr {
    let service = StreamableHttpService::new(
        || {
            mcp_google_ads::GoogleAdsMcp::new(Config::default())
                .map_err(|e| std::io::Error::other(e.to_string()))
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router =
        axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn(move |req, next| {
                mcp_google_ads::http_auth::check_api_key(api_key.clone(), req, next)
            }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds an ephemeral loopback port");
    let addr = listener.local_addr().expect("listener has a local addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    addr
}

fn initialize_request() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": rmcp::model::ProtocolVersion::LATEST,
            "capabilities": {},
            "clientInfo": {"name": "api-key-test", "version": "0.0.1"}
        }
    })
}

#[tokio::test]
async fn missing_api_key_header_is_rejected() {
    let addr = spawn_test_server(Some("s3cr3t".to_string())).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(initialize_request().to_string())
        .send()
        .await
        .expect("the server responds");

    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_api_key_header_is_rejected() {
    let addr = spawn_test_server(Some("s3cr3t".to_string())).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("x-api-key", "wrong-key")
        .body(initialize_request().to_string())
        .send()
        .await
        .expect("the server responds");

    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn correct_api_key_header_is_accepted() {
    let addr = spawn_test_server(Some("s3cr3t".to_string())).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("x-api-key", "s3cr3t")
        .body(initialize_request().to_string())
        .send()
        .await
        .expect("the server responds");

    assert!(
        response.status().is_success(),
        "initialize should succeed with the correct API key, got {}",
        response.status()
    );
}

#[tokio::test]
async fn unset_api_key_leaves_the_transport_unauthenticated() {
    let addr = spawn_test_server(None).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(initialize_request().to_string())
        .send()
        .await
        .expect("the server responds");

    assert!(
        response.status().is_success(),
        "initialize should succeed when no API key is configured, got {}",
        response.status()
    );
}
