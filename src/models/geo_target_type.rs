//! Campaign geo target type enum.
//!
//! Maps directly to the Google Ads API `Campaign.geo_target_type_setting`
//! enum strings (UPPERCASE / SCREAMING_SNAKE_CASE). The same two values are
//! valid for both `positive_geo_target_type` and `negative_geo_target_type`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Geo target type for a campaign's `geo_target_type_setting`.
///
/// Serialized as `"PRESENCE_OR_INTEREST"` or `"PRESENCE"` to match the
/// Google Ads API enum values. Applies to both the positive and the negative
/// geo target type fields.
///
/// - `PRESENCE_OR_INTEREST` (the API default): ads can serve to people who are
///   in, regularly in, OR who show interest in the targeted locations.
/// - `PRESENCE`: ads serve only to people who are physically in (or regularly
///   in) the targeted locations — "Presence: People in or regularly in your
///   targeted locations" in the Google Ads UI. The tighter setting most
///   advertisers want to avoid paying for out-of-area interest traffic.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeoTargetType {
    /// People in, regularly in, or interested in the targeted locations.
    PresenceOrInterest,
    /// Only people physically in (or regularly in) the targeted locations.
    Presence,
}

impl GeoTargetType {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            GeoTargetType::PresenceOrInterest => "PRESENCE_OR_INTEREST",
            GeoTargetType::Presence => "PRESENCE",
        }
    }
}

impl std::fmt::Display for GeoTargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&GeoTargetType::PresenceOrInterest).unwrap(),
            "\"PRESENCE_OR_INTEREST\""
        );
        assert_eq!(
            serde_json::to_string(&GeoTargetType::Presence).unwrap(),
            "\"PRESENCE\""
        );
    }

    #[test]
    fn deserializes_screaming_snake_case() {
        let poi: GeoTargetType = serde_json::from_str("\"PRESENCE_OR_INTEREST\"").unwrap();
        assert_eq!(poi, GeoTargetType::PresenceOrInterest);
        let p: GeoTargetType = serde_json::from_str("\"PRESENCE\"").unwrap();
        assert_eq!(p, GeoTargetType::Presence);
    }

    #[test]
    fn rejects_unknown_variant() {
        let r: Result<GeoTargetType, _> = serde_json::from_str("\"SEARCH_INTEREST\"");
        assert!(r.is_err());
    }

    #[test]
    fn as_api_str_matches() {
        assert_eq!(
            GeoTargetType::PresenceOrInterest.as_api_str(),
            "PRESENCE_OR_INTEREST"
        );
        assert_eq!(GeoTargetType::Presence.as_api_str(), "PRESENCE");
    }
}
