//! The MCP handshake must keep advertising this server's own identity.
//!
//! `get_info()` is what every MCP client reads when it connects: the name it
//! displays and matches its configuration against, the version, and the tools
//! capability without which it never asks for a tool list. rmcp's model structs
//! are `#[non_exhaustive]`, so this response can only be built through
//! constructors whose defaults come from *rmcp*'s own build environment —
//! `Implementation::default()` would quietly advertise the Rust crate name
//! `mcp_google_ads` instead of the published `mcp-google-ads`, and a capability
//! builder left unconfigured would hide every tool. Pin the wire-visible values.

use mcp_google_ads::config::Config;
use mcp_google_ads::GoogleAdsMcp;
use rmcp::ServerHandler;

#[test]
fn handshake_advertises_package_identity_and_tools() {
    let server = GoogleAdsMcp::new(Config::default()).expect("server builds from a default config");
    let info = server.get_info();

    assert_eq!(
        info.server_info.name, "mcp-google-ads",
        "clients match their configuration against the published package name"
    );
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        info.server_info.description.as_deref(),
        Some("MCP server for Google Ads API with safety guardrails")
    );
    assert_eq!(
        info.protocol_version,
        rmcp::model::ProtocolVersion::LATEST,
        "the server speaks the newest protocol version its rmcp release supports"
    );
    assert!(
        info.capabilities.tools.is_some(),
        "without the tools capability no client ever calls tools/list"
    );

    let instructions = info
        .instructions
        .expect("clients are told what this server does");
    assert!(
        instructions.contains("Google Ads"),
        "instructions should name the API being driven, got: {instructions}"
    );
}

#[test]
fn handshake_serialises_to_the_shape_clients_read() {
    let server = GoogleAdsMcp::new(Config::default()).expect("server builds from a default config");
    let wire = serde_json::to_value(server.get_info()).expect("ServerInfo serialises");

    assert_eq!(wire["serverInfo"]["name"], "mcp-google-ads");
    assert_eq!(wire["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        wire["capabilities"]["tools"].is_object(),
        "tools capability must survive serialisation, got: {}",
        wire["capabilities"]
    );
    assert!(
        wire["instructions"].is_string(),
        "instructions must survive serialisation"
    );
}
