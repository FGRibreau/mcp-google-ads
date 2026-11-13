//! Ad group type enum.
//!
//! Maps directly to the Google Ads API `AdGroup.type` enum strings. The type is
//! immutable once the ad group exists and must match the campaign's advertising
//! channel: a Display campaign rejects a `SEARCH_STANDARD` ad group and vice
//! versa.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Type of a new ad group.
///
/// Serialized as `"SEARCH_STANDARD"` or `"DISPLAY_STANDARD"` to match the Google
/// Ads API enum values. Defaults to [`AdGroupType::SearchStandard`], which is
/// what `create_ad_group` always wrote before the type became selectable.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdGroupType {
    /// Standard ad group for a Search campaign.
    #[default]
    SearchStandard,
    /// Standard ad group for a Display campaign.
    DisplayStandard,
}

impl AdGroupType {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            AdGroupType::SearchStandard => "SEARCH_STANDARD",
            AdGroupType::DisplayStandard => "DISPLAY_STANDARD",
        }
    }
}

impl std::fmt::Display for AdGroupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_search_standard() {
        assert_eq!(AdGroupType::default(), AdGroupType::SearchStandard);
    }

    #[test]
    fn round_trips_screaming_snake_case() {
        for (variant, wire) in [
            (AdGroupType::SearchStandard, "\"SEARCH_STANDARD\""),
            (AdGroupType::DisplayStandard, "\"DISPLAY_STANDARD\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), wire);
            let back: AdGroupType = serde_json::from_str(wire).unwrap();
            assert_eq!(back, variant);
            assert_eq!(format!("\"{}\"", variant.as_api_str()), wire);
        }
    }

    #[test]
    fn rejects_unknown_variant() {
        let r: Result<AdGroupType, _> = serde_json::from_str("\"VIDEO_BUMPER\"");
        assert!(r.is_err());
    }
}
