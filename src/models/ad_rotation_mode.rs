//! Ad group ad rotation mode enum.
//!
//! Maps directly to the Google Ads API `AdGroup.ad_rotation_mode` enum
//! strings (UPPERCASE). `ROTATE_FOREVER` is what the Google Ads UI calls
//! "Rotate indefinitely".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Ad rotation mode for an ad group.
///
/// Serialized as `"OPTIMIZE"` or `"ROTATE_FOREVER"` to match the
/// Google Ads API enum values. There is no default: rotation mode is
/// only written when explicitly requested.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdRotationMode {
    /// Google optimizes serving toward the best-performing ad.
    Optimize,
    /// Ads rotate evenly without end ("Rotate indefinitely" in the UI).
    RotateForever,
}

impl AdRotationMode {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            AdRotationMode::Optimize => "OPTIMIZE",
            AdRotationMode::RotateForever => "ROTATE_FOREVER",
        }
    }
}

impl std::fmt::Display for AdRotationMode {
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
            serde_json::to_string(&AdRotationMode::Optimize).unwrap(),
            "\"OPTIMIZE\""
        );
        assert_eq!(
            serde_json::to_string(&AdRotationMode::RotateForever).unwrap(),
            "\"ROTATE_FOREVER\""
        );
    }

    #[test]
    fn deserializes_screaming_snake_case() {
        let o: AdRotationMode = serde_json::from_str("\"OPTIMIZE\"").unwrap();
        assert_eq!(o, AdRotationMode::Optimize);
        let r: AdRotationMode = serde_json::from_str("\"ROTATE_FOREVER\"").unwrap();
        assert_eq!(r, AdRotationMode::RotateForever);
    }

    #[test]
    fn rejects_unknown_variant() {
        let r: Result<AdRotationMode, _> = serde_json::from_str("\"ROTATE_INDEFINITELY\"");
        assert!(r.is_err());
    }

    #[test]
    fn as_api_str_matches() {
        assert_eq!(AdRotationMode::Optimize.as_api_str(), "OPTIMIZE");
        assert_eq!(AdRotationMode::RotateForever.as_api_str(), "ROTATE_FOREVER");
    }
}
