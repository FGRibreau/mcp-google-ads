//! Unknown `AdStatus` variants are rejected at deserialization. This is the
//! serde contract that protects the MCP surface from typo'd input.

use mcp_google_ads::models::AdStatus;

#[test]
fn unknown_status_variant_is_rejected() {
    let r: Result<AdStatus, _> = serde_json::from_str("\"DRAFT\"");
    assert!(r.is_err(), "DRAFT is not a valid AdStatus variant");

    let r: Result<AdStatus, _> = serde_json::from_str("\"enabled\"");
    assert!(
        r.is_err(),
        "lowercase variants must be rejected — Google Ads API uses UPPERCASE"
    );

    let r: Result<AdStatus, _> = serde_json::from_str("\"PENDING\"");
    assert!(r.is_err());
}

#[test]
fn known_variants_round_trip() {
    let serialized = serde_json::to_string(&AdStatus::Paused).unwrap();
    assert_eq!(serialized, "\"PAUSED\"");
    let back: AdStatus = serde_json::from_str(&serialized).unwrap();
    assert_eq!(back, AdStatus::Paused);
}
