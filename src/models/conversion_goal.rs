//! Conversion goal enums and inputs.
//!
//! Map directly to the Google Ads API `ConversionActionCategory`,
//! `ConversionOrigin` and `GoalConfigLevel` enum strings. A conversion goal is a
//! (category, origin) pair: a campaign bids on the conversion actions of every
//! biddable goal, or on a custom conversion goal instead.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether a campaign follows the account's conversion goals or carries its own.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoalConfigLevel {
    /// The campaign follows the account default goals (`customer_conversion_goal`).
    Customer,
    /// The campaign carries its own goals (`campaign_conversion_goal`) or a custom goal.
    Campaign,
}

impl GoalConfigLevel {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            GoalConfigLevel::Customer => "CUSTOMER",
            GoalConfigLevel::Campaign => "CAMPAIGN",
        }
    }
}

/// Category of a conversion goal (`ConversionActionCategory`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConversionActionCategory {
    Default,
    PageView,
    Purchase,
    Signup,
    Download,
    AddToCart,
    BeginCheckout,
    SubscribePaid,
    PhoneCallLead,
    ImportedLead,
    SubmitLeadForm,
    BookAppointment,
    RequestQuote,
    GetDirections,
    OutboundClick,
    Contact,
    Engagement,
    StoreVisit,
    StoreSale,
    QualifiedLead,
    ConvertedLead,
}

impl ConversionActionCategory {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            ConversionActionCategory::Default => "DEFAULT",
            ConversionActionCategory::PageView => "PAGE_VIEW",
            ConversionActionCategory::Purchase => "PURCHASE",
            ConversionActionCategory::Signup => "SIGNUP",
            ConversionActionCategory::Download => "DOWNLOAD",
            ConversionActionCategory::AddToCart => "ADD_TO_CART",
            ConversionActionCategory::BeginCheckout => "BEGIN_CHECKOUT",
            ConversionActionCategory::SubscribePaid => "SUBSCRIBE_PAID",
            ConversionActionCategory::PhoneCallLead => "PHONE_CALL_LEAD",
            ConversionActionCategory::ImportedLead => "IMPORTED_LEAD",
            ConversionActionCategory::SubmitLeadForm => "SUBMIT_LEAD_FORM",
            ConversionActionCategory::BookAppointment => "BOOK_APPOINTMENT",
            ConversionActionCategory::RequestQuote => "REQUEST_QUOTE",
            ConversionActionCategory::GetDirections => "GET_DIRECTIONS",
            ConversionActionCategory::OutboundClick => "OUTBOUND_CLICK",
            ConversionActionCategory::Contact => "CONTACT",
            ConversionActionCategory::Engagement => "ENGAGEMENT",
            ConversionActionCategory::StoreVisit => "STORE_VISIT",
            ConversionActionCategory::StoreSale => "STORE_SALE",
            ConversionActionCategory::QualifiedLead => "QUALIFIED_LEAD",
            ConversionActionCategory::ConvertedLead => "CONVERTED_LEAD",
        }
    }
}

/// Origin of a conversion goal (`ConversionOrigin`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConversionOrigin {
    Website,
    GoogleHosted,
    App,
    CallFromAds,
    Store,
    YoutubeHosted,
}

impl ConversionOrigin {
    /// String representation matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            ConversionOrigin::Website => "WEBSITE",
            ConversionOrigin::GoogleHosted => "GOOGLE_HOSTED",
            ConversionOrigin::App => "APP",
            ConversionOrigin::CallFromAds => "CALL_FROM_ADS",
            ConversionOrigin::Store => "STORE",
            ConversionOrigin::YoutubeHosted => "YOUTUBE_HOSTED",
        }
    }
}

/// One goal of a campaign: whether it bids on the conversion actions of a (category, origin).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CampaignConversionGoalInput {
    pub category: ConversionActionCategory,
    pub origin: ConversionOrigin,
    /// true: the campaign bids on this goal's conversion actions.
    pub biddable: bool,
}
