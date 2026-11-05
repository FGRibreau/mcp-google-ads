//! Audience kinds an ad group criterion can carry.
//!
//! Each maps to one Google Ads API criterion info message and the resource it
//! references: a user list, an affinity or in-market segment (user interest), a
//! custom segment, or a combined segment.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The kind of audience an ad group criterion references.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudienceKind {
    /// A user list of the account (remarketing, customer list): `userLists/{id}`.
    UserList,
    /// An affinity or in-market segment: `userInterests/{id}`.
    UserInterest,
    /// A custom segment of the account: `customAudiences/{id}`.
    CustomAudience,
    /// A combined segment of the account: `combinedAudiences/{id}`.
    CombinedAudience,
}

impl AudienceKind {
    /// The criterion field, its info field and the resource collection of this kind.
    pub fn api_fields(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            AudienceKind::UserList => ("userList", "userList", "userLists"),
            AudienceKind::UserInterest => ("userInterest", "userInterestCategory", "userInterests"),
            AudienceKind::CustomAudience => ("customAudience", "customAudience", "customAudiences"),
            AudienceKind::CombinedAudience => {
                ("combinedAudience", "combinedAudience", "combinedAudiences")
            }
        }
    }

    /// String representation matching this enum's serialized value.
    pub fn as_str(&self) -> &'static str {
        match self {
            AudienceKind::UserList => "USER_LIST",
            AudienceKind::UserInterest => "USER_INTEREST",
            AudienceKind::CustomAudience => "CUSTOM_AUDIENCE",
            AudienceKind::CombinedAudience => "COMBINED_AUDIENCE",
        }
    }
}
