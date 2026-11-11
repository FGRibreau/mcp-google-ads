//! Shared domain models used by tools, safety, and the public MCP surface.

/// Declare an enum whose wire names are Google Ads API enum strings.
///
/// Each variant names its API string once, explicitly, and that one literal
/// drives the serde name, the JSON schema an MCP host sees, and
/// `as_api_str()`: the three cannot drift apart. Explicit names rather than
/// `rename_all` because serde inserts no separator before a digit, so a
/// derived name can silently differ from the API's (see `demographic.rs`).
///
/// `ALL` lists every variant, which keeps the "every variant round-trips"
/// tests exhaustive without a hand-maintained copy of the list.
macro_rules! api_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $api:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(
            serde::Serialize,
            serde::Deserialize,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            schemars::JsonSchema,
        )]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $api)] $variant, )+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// String representation matching the Google Ads REST API enum value.
            pub fn as_api_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $api,)+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_api_str())
            }
        }
    };
}

pub mod ad_group_type;
pub mod ad_rotation_mode;
pub mod ad_status;
pub mod audience_targeting_mode;
pub mod content_label;
pub mod conversion_goal;
pub mod custom_segment;
pub mod demographic;
pub mod display_ad;
pub mod frequency_cap;
pub mod geo_target_type;
pub mod next_action_hint;

pub use ad_group_type::AdGroupType;
pub use ad_rotation_mode::AdRotationMode;
pub use ad_status::AdStatus;
pub use audience_targeting_mode::AudienceTargetingMode;
pub use content_label::ContentLabelType;
pub use conversion_goal::{
    CampaignConversionGoalInput, ConversionActionCategory, ConversionOrigin,
};
pub use custom_segment::CustomSegmentType;
pub use demographic::{AgeRange, Gender, IncomeRange};
pub use display_ad::DisplayAdType;
pub use frequency_cap::{FrequencyCap, FrequencyCapLevel, FrequencyCapTimeUnit};
pub use geo_target_type::GeoTargetType;
pub use next_action_hint::NextActionHint;
