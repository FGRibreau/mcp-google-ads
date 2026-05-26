//! Ad/campaign/asset-group lifecycle status enum.
//!
//! Maps directly to the Google Ads API status enum strings (UPPERCASE).
//! Default is [`AdStatus::Paused`] for safety — new entities are created
//! paused so they don't accidentally run before review.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Lifecycle status for an ad, ad group, campaign, or asset group.
///
/// Serialized as `"ENABLED"`, `"PAUSED"`, or `"REMOVED"` to match the
/// Google Ads API enum strings.
///
/// Default is [`AdStatus::Paused`] — newly drafted entities ship paused
/// so an agent can review before traffic flows. Pass `status: "ENABLED"`
/// explicitly to opt out of this safety default.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum AdStatus {
    Enabled,
    #[default]
    Paused,
    Removed,
}

impl AdStatus {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            AdStatus::Enabled => "ENABLED",
            AdStatus::Paused => "PAUSED",
            AdStatus::Removed => "REMOVED",
        }
    }
}

impl std::fmt::Display for AdStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_paused() {
        assert_eq!(AdStatus::default(), AdStatus::Paused);
    }

    #[test]
    fn serializes_uppercase() {
        assert_eq!(
            serde_json::to_string(&AdStatus::Enabled).unwrap(),
            "\"ENABLED\""
        );
        assert_eq!(
            serde_json::to_string(&AdStatus::Paused).unwrap(),
            "\"PAUSED\""
        );
        assert_eq!(
            serde_json::to_string(&AdStatus::Removed).unwrap(),
            "\"REMOVED\""
        );
    }

    #[test]
    fn deserializes_uppercase() {
        let p: AdStatus = serde_json::from_str("\"PAUSED\"").unwrap();
        assert_eq!(p, AdStatus::Paused);
        let e: AdStatus = serde_json::from_str("\"ENABLED\"").unwrap();
        assert_eq!(e, AdStatus::Enabled);
    }

    #[test]
    fn rejects_unknown_variant() {
        let r: Result<AdStatus, _> = serde_json::from_str("\"DRAFT\"");
        assert!(r.is_err());
    }

    #[test]
    fn as_api_str_matches() {
        assert_eq!(AdStatus::Enabled.as_api_str(), "ENABLED");
        assert_eq!(AdStatus::Paused.as_api_str(), "PAUSED");
        assert_eq!(AdStatus::Removed.as_api_str(), "REMOVED");
    }
}
