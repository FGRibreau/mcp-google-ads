//! Conversion goal keys: the category and origin a goal is named by.
//!
//! Account-level (`CustomerConversionGoal`) and campaign-level
//! (`CampaignConversionGoal`) goals are not created by callers: Google
//! generates one per conversion category × origin the account has conversion
//! actions for, and the API only updates them. Their resource names carry the
//! two enum names, e.g. `customers/1/campaignConversionGoals/2~PURCHASE~WEBSITE`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

api_enum! {
    /// Conversion action category (v25 `ConversionActionCategory`, without
    /// `UNSPECIFIED` / `UNKNOWN`).
    pub enum ConversionActionCategory {
        /// Default category.
        Default = "DEFAULT",
        /// Page view.
        PageView = "PAGE_VIEW",
        /// Purchase, sale or order.
        Purchase = "PURCHASE",
        /// Signup.
        Signup = "SIGNUP",
        /// Software download.
        Download = "DOWNLOAD",
        /// Add to cart.
        AddToCart = "ADD_TO_CART",
        /// Begin checkout.
        BeginCheckout = "BEGIN_CHECKOUT",
        /// Paid subscription.
        SubscribePaid = "SUBSCRIBE_PAID",
        /// Phone call lead.
        PhoneCallLead = "PHONE_CALL_LEAD",
        /// Imported lead.
        ImportedLead = "IMPORTED_LEAD",
        /// Lead form submission.
        SubmitLeadForm = "SUBMIT_LEAD_FORM",
        /// Appointment booking.
        BookAppointment = "BOOK_APPOINTMENT",
        /// Quote or price estimate request.
        RequestQuote = "REQUEST_QUOTE",
        /// Directions to a business location.
        GetDirections = "GET_DIRECTIONS",
        /// Click to a partner's site.
        OutboundClick = "OUTBOUND_CLICK",
        /// Contact (call, SMS, email, chat).
        Contact = "CONTACT",
        /// Website engagement.
        Engagement = "ENGAGEMENT",
        /// Store visit.
        StoreVisit = "STORE_VISIT",
        /// In-store sale.
        StoreSale = "STORE_SALE",
        /// Qualified lead.
        QualifiedLead = "QUALIFIED_LEAD",
        /// Converted lead.
        ConvertedLead = "CONVERTED_LEAD",
        /// YouTube follow-on views.
        YoutubeFollowOnViews = "YOUTUBE_FOLLOW_ON_VIEWS",
        /// In-app ad revenue.
        InAppAdRevenue = "IN_APP_AD_REVENUE",
    }
}

api_enum! {
    /// Where a conversion happens (v25 `ConversionOrigin`, without
    /// `UNSPECIFIED` / `UNKNOWN`).
    pub enum ConversionOrigin {
        /// On the advertiser's website.
        Website = "WEBSITE",
        /// On a Google-hosted surface (e.g. a lead form).
        GoogleHosted = "GOOGLE_HOSTED",
        /// In an app.
        App = "APP",
        /// A call from an ad.
        CallFromAds = "CALL_FROM_ADS",
        /// In a physical store.
        Store = "STORE",
        /// On YouTube.
        YoutubeHosted = "YOUTUBE_HOSTED",
        /// Local Services Ads.
        LocalServicesAds = "LOCAL_SERVICES_ADS",
    }
}

/// One campaign conversion goal to update: the goal named by `category` ×
/// `origin`, and whether the campaign bids on it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CampaignConversionGoalInput {
    /// Conversion category, e.g. `SUBMIT_LEAD_FORM`.
    pub category: ConversionActionCategory,
    /// Conversion origin, e.g. `WEBSITE`.
    pub origin: ConversionOrigin,
    /// `true`: the campaign bids on the primary conversion actions in this
    /// category × origin. `false`: it does not.
    pub biddable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_and_origin_round_trips() {
        for category in ConversionActionCategory::ALL {
            let wire = serde_json::to_string(category).unwrap();
            assert_eq!(wire, format!("\"{}\"", category.as_api_str()));
            assert_eq!(
                &serde_json::from_str::<ConversionActionCategory>(&wire).unwrap(),
                category
            );
        }
        for origin in ConversionOrigin::ALL {
            let wire = serde_json::to_string(origin).unwrap();
            assert_eq!(wire, format!("\"{}\"", origin.as_api_str()));
            assert_eq!(
                &serde_json::from_str::<ConversionOrigin>(&wire).unwrap(),
                origin
            );
        }
        assert_eq!(ConversionActionCategory::ALL.len(), 23);
        assert_eq!(ConversionOrigin::ALL.len(), 7);
    }

    #[test]
    fn goal_input_rejects_unknown_fields_and_values() {
        let ok: CampaignConversionGoalInput = serde_json::from_str(
            r#"{"category": "SUBMIT_LEAD_FORM", "origin": "WEBSITE", "biddable": true}"#,
        )
        .unwrap();
        assert_eq!(ok.category, ConversionActionCategory::SubmitLeadForm);

        assert!(serde_json::from_str::<CampaignConversionGoalInput>(
            r#"{"category": "LEAD", "origin": "WEBSITE", "biddable": true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<CampaignConversionGoalInput>(
            r#"{"category": "PURCHASE", "origin": "WEBSITE", "biddable": true, "primary": true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<CampaignConversionGoalInput>(
            r#"{"category": "PURCHASE", "origin": "UNKNOWN", "biddable": true}"#
        )
        .is_err());
    }
}
