//! `GOOGLE_ADS_MCP_TRANSPORT=http` must actually produce a working MCP
//! endpoint, not just a config flag nobody wires up. This drives the real
//! `StreamableHttpService` + `axum` stack `src/main.rs::run_http` builds —
//! binding an ephemeral port, sending a real `initialize` POST over HTTP, and
//! asserting the wire response — rather than only checking `Config` parsing.

use mcp_google_ads::config::Config;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

async fn spawn_test_server() -> std::net::SocketAddr {
    let service = StreamableHttpService::new(
        || {
            mcp_google_ads::GoogleAdsMcp::new(Config::default())
                .map_err(|e| std::io::Error::other(e.to_string()))
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binds an ephemeral loopback port");
    let addr = listener.local_addr().expect("listener has a local addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    addr
}

#[tokio::test]
async fn initialize_over_http_returns_this_servers_identity() {
    let addr = spawn_test_server().await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": rmcp::model::ProtocolVersion::LATEST,
                    "capabilities": {},
                    "clientInfo": {"name": "streamable-http-test", "version": "0.0.1"}
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("the MCP endpoint accepts an HTTP POST");

    assert!(
        response.status().is_success(),
        "initialize should succeed, got {}",
        response.status()
    );

    let body = response.text().await.expect("response body is readable");
    // Streamable HTTP responds as SSE: an empty priming `data:` event first
    // (for reconnection), then the actual JSON-RPC result as its own `data:`
    // event. Skip the priming event rather than assuming the first one has
    // the payload.
    let json_line = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find(|data| !data.is_empty())
        .unwrap_or(&body);
    let wire: serde_json::Value =
        serde_json::from_str(json_line).expect("the payload is a JSON-RPC response");

    assert_eq!(wire["result"]["serverInfo"]["name"], "mcp-google-ads");
    assert!(
        wire["result"]["capabilities"]["tools"].is_object(),
        "the HTTP transport must advertise the same tools capability as stdio, got: {}",
        wire["result"]["capabilities"]
    );
}

#[tokio::test]
async fn unknown_path_is_not_served() {
    let addr = spawn_test_server().await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/not-mcp"))
        .send()
        .await
        .expect("the server responds even for an unmounted path");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}
