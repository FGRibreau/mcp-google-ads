//! How an ad group's audience criteria act: restrict reach, or only observe.
//!
//! This is the ad group's `targeting_setting` restriction for the `AUDIENCE`
//! targeting dimension, which covers user lists, user interests (affinity and
//! in-market), custom segments and custom affinity / intent. The Google Ads UI
//! calls the two states "Targeting" and "Observation"; the API stores them as
//! `TargetRestriction { targeting_dimension: AUDIENCE, bid_only }`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Audience targeting mode of an ad group.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudienceTargetingMode {
    /// "Targeting": ads only reach people in the ad group's audience segments
    /// (`bid_only = false`).
    Targeting,
    /// "Observation": the segments do not narrow reach; they are only reported
    /// on and can carry bid adjustments (`bid_only = true`).
    Observation,
}

impl AudienceTargetingMode {
    /// The `TargetRestriction.bid_only` value this mode stands for.
    pub fn bid_only(&self) -> bool {
        matches!(self, AudienceTargetingMode::Observation)
    }

    /// The name used for this mode in tool input and previews.
    pub fn as_str(&self) -> &'static str {
        match self {
            AudienceTargetingMode::Targeting => "TARGETING",
            AudienceTargetingMode::Observation => "OBSERVATION",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bid_only_maps_observation_to_true() {
        assert!(!AudienceTargetingMode::Targeting.bid_only());
        assert!(AudienceTargetingMode::Observation.bid_only());
    }

    #[test]
    fn round_trips_and_rejects_unknown() {
        for (mode, wire) in [
            (AudienceTargetingMode::Targeting, "\"TARGETING\""),
            (AudienceTargetingMode::Observation, "\"OBSERVATION\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), wire);
            assert_eq!(format!("\"{}\"", mode.as_str()), wire);
            let back: AudienceTargetingMode = serde_json::from_str(wire).unwrap();
            assert_eq!(back, mode);
        }
        assert!(serde_json::from_str::<AudienceTargetingMode>("\"BID_ONLY\"").is_err());
    }
}
