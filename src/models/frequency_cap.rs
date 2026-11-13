//! Campaign frequency caps.
//!
//! Maps to the Google Ads API `Campaign.frequency_caps` list of
//! `FrequencyCapEntry { key: FrequencyCapKey, cap }`. Only impression caps are
//! modelled: they are the ones that apply to Display campaigns, which is where
//! a cap matters most (a small remarketing list sees the same ad very often
//! without one).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Window a frequency cap counts impressions over.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FrequencyCapTimeUnit {
    /// Per day.
    Day,
    /// Per week.
    Week,
    /// Per month.
    Month,
}

impl FrequencyCapTimeUnit {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            FrequencyCapTimeUnit::Day => "DAY",
            FrequencyCapTimeUnit::Week => "WEEK",
            FrequencyCapTimeUnit::Month => "MONTH",
        }
    }
}

/// What a frequency cap counts impressions of.
///
/// `CAMPAIGN` (the default) caps how often one user sees any ad of the
/// campaign — what the Google Ads UI shows as the campaign's frequency cap.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FrequencyCapLevel {
    /// Impressions of a single ad.
    AdGroupAd,
    /// Impressions of any ad in a single ad group.
    AdGroup,
    /// Impressions of any ad in the campaign.
    #[default]
    Campaign,
}

impl FrequencyCapLevel {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            FrequencyCapLevel::AdGroupAd => "AD_GROUP_AD",
            FrequencyCapLevel::AdGroup => "AD_GROUP",
            FrequencyCapLevel::Campaign => "CAMPAIGN",
        }
    }
}

/// One impression cap: at most `max_impressions` per user per `time_unit`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrequencyCap {
    /// Maximum impressions per user in the window. At least 1.
    pub max_impressions: i32,
    /// The window: DAY, WEEK or MONTH.
    pub time_unit: FrequencyCapTimeUnit,
    /// What is counted: CAMPAIGN (default), AD_GROUP or AD_GROUP_AD.
    #[serde(default)]
    pub level: Option<FrequencyCapLevel>,
}

impl FrequencyCap {
    /// The level this cap counts at, with the `CAMPAIGN` default applied.
    pub fn resolved_level(&self) -> FrequencyCapLevel {
        self.level.unwrap_or_default()
    }

    /// The `FrequencyCapEntry` JSON the REST API expects.
    pub fn to_api_entry(&self) -> serde_json::Value {
        serde_json::json!({
            "key": {
                "level": self.resolved_level().as_api_str(),
                "eventType": "IMPRESSION",
                "timeUnit": self.time_unit.as_api_str(),
                "timeLength": 1
            },
            "cap": self.max_impressions
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_defaults_to_campaign() {
        let cap: FrequencyCap =
            serde_json::from_str(r#"{"max_impressions": 3, "time_unit": "DAY"}"#).unwrap();
        assert_eq!(cap.resolved_level(), FrequencyCapLevel::Campaign);
    }

    #[test]
    fn api_entry_has_the_rest_shape() {
        let cap = FrequencyCap {
            max_impressions: 10,
            time_unit: FrequencyCapTimeUnit::Week,
            level: Some(FrequencyCapLevel::AdGroupAd),
        };
        let entry = cap.to_api_entry();
        assert_eq!(entry["cap"], 10);
        assert_eq!(entry["key"]["level"], "AD_GROUP_AD");
        assert_eq!(entry["key"]["eventType"], "IMPRESSION");
        assert_eq!(entry["key"]["timeUnit"], "WEEK");
        assert_eq!(entry["key"]["timeLength"], 1);
    }

    #[test]
    fn rejects_unknown_fields_and_variants() {
        let unknown_field: Result<FrequencyCap, _> =
            serde_json::from_str(r#"{"max_impressions": 3, "time_unit": "DAY", "cap": 3}"#);
        assert!(unknown_field.is_err());
        let unknown_unit: Result<FrequencyCap, _> =
            serde_json::from_str(r#"{"max_impressions": 3, "time_unit": "HOUR"}"#);
        assert!(unknown_unit.is_err());
    }
}
