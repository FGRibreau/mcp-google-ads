//! Device enum for campaign device criteria.
//!
//! Maps to the Google Ads API `Device` enum strings. A campaign holds one device
//! criterion per device, under a fixed criterion ID, whose bid modifier adjusts
//! (or, at 0, excludes) delivery on that device.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A device a campaign bid modifier applies to.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Device {
    Desktop,
    Mobile,
    Tablet,
}

impl Device {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Device::Desktop => "DESKTOP",
            Device::Mobile => "MOBILE",
            Device::Tablet => "TABLET",
        }
    }

    /// The fixed criterion ID of this device's campaign criterion.
    pub fn criterion_id(&self) -> &'static str {
        match self {
            Device::Desktop => "30000",
            Device::Mobile => "30001",
            Device::Tablet => "30002",
        }
    }
}
