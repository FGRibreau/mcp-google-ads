//! Target impression share location enum.
//!
//! Maps directly to the Google Ads API `TargetImpressionShareLocation` enum
//! strings, the page area a Target Impression Share strategy aims at.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where on the results page a Target Impression Share strategy aims to show ads.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetImpressionShareLocation {
    /// Anywhere on the results page.
    AnywhereOnPage,
    /// Above the organic results.
    TopOfPage,
    /// The very first ad position.
    AbsoluteTopOfPage,
}

impl TargetImpressionShareLocation {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            TargetImpressionShareLocation::AnywhereOnPage => "ANYWHERE_ON_PAGE",
            TargetImpressionShareLocation::TopOfPage => "TOP_OF_PAGE",
            TargetImpressionShareLocation::AbsoluteTopOfPage => "ABSOLUTE_TOP_OF_PAGE",
        }
    }
}
