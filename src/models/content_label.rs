//! Content label types for account-level content exclusions.
//!
//! Maps to the Google Ads API v25 `ContentLabelTypeEnum.ContentLabelType`,
//! minus `UNSPECIFIED` and `UNKNOWN`, which cannot be written. A content label
//! can only be excluded (there is no positive content label criterion), at the
//! campaign or the customer level.
//!
//! Which labels still mean something on Display follows Google's "About content
//! suitability" help page (support.google.com/google-ads/answer/12764663):
//!
//! - Sensitive content (`TRAGEDY`, `SOCIAL_ISSUES`, `PROFANITY`,
//!   `SEXUALLY_SUGGESTIVE`, `JUVENILE`): Display Network and Google Video
//!   Partners.
//! - Digital content labels (`VIDEO_RATING_DV_G/PG/T/MA`,
//!   `VIDEO_NOT_YET_RATED`): Google Display Network; YouTube stopped using them
//!   in September 2024. Excluding all five stops a Display campaign serving.
//! - Content types (`EMBEDDED_VIDEO`, `LIVE_STREAMING_VIDEO`, and
//!   `BELOW_THE_FOLD`, which is Display only).
//! - Excluded content themes (`BRAND_SUITABILITY_*`): YouTube, Display Network
//!   and Google Video Partners, except `BRAND_SUITABILITY_HEALTH_SOURCE_UNDETERMINED`,
//!   which is YouTube only.
//! - `PARKED_DOMAIN`: Google removed parked domains as an ad surface, and the
//!   content setting for them, on 10 February 2026.
//! - `VIDEO`: not part of Google's current content suitability settings.

api_enum! {
    /// A content label that can be excluded (v25 `ContentLabelType`).
    pub enum ContentLabelType {
        /// Sexually suggestive content.
        SexuallySuggestive = "SEXUALLY_SUGGESTIVE",
        /// Below-the-fold placements (Display only).
        BelowTheFold = "BELOW_THE_FOLD",
        /// Parked domains. Deprecated: removed by Google on 10 February 2026.
        ParkedDomain = "PARKED_DOMAIN",
        /// Juvenile, gross and bizarre content.
        Juvenile = "JUVENILE",
        /// Profanity and rough language.
        Profanity = "PROFANITY",
        /// Tragedy and conflict.
        Tragedy = "TRAGEDY",
        /// Video. Not part of Google's current content suitability settings.
        Video = "VIDEO",
        /// Digital content label DL-G.
        VideoRatingDvG = "VIDEO_RATING_DV_G",
        /// Digital content label DL-PG.
        VideoRatingDvPg = "VIDEO_RATING_DV_PG",
        /// Digital content label DL-T.
        VideoRatingDvT = "VIDEO_RATING_DV_T",
        /// Digital content label DL-MA.
        VideoRatingDvMa = "VIDEO_RATING_DV_MA",
        /// Content not yet labelled.
        VideoNotYetRated = "VIDEO_NOT_YET_RATED",
        /// YouTube videos embedded on other sites.
        EmbeddedVideo = "EMBEDDED_VIDEO",
        /// Live streaming video.
        LiveStreamingVideo = "LIVE_STREAMING_VIDEO",
        /// Sensitive social issues.
        SocialIssues = "SOCIAL_ISSUES",
        /// Content suitable for families, including Made for Kids videos.
        BrandSuitabilityContentForFamilies = "BRAND_SUITABILITY_CONTENT_FOR_FAMILIES",
        /// Fighting video games.
        BrandSuitabilityGamesFighting = "BRAND_SUITABILITY_GAMES_FIGHTING",
        /// Mature video games.
        BrandSuitabilityGamesMature = "BRAND_SUITABILITY_GAMES_MATURE",
        /// Sensitive health content.
        BrandSuitabilityHealthSensitive = "BRAND_SUITABILITY_HEALTH_SENSITIVE",
        /// Health content from less commonly cited sources (YouTube only).
        BrandSuitabilityHealthSourceUndetermined = "BRAND_SUITABILITY_HEALTH_SOURCE_UNDETERMINED",
        /// Recent news.
        BrandSuitabilityNewsRecent = "BRAND_SUITABILITY_NEWS_RECENT",
        /// Sensitive news.
        BrandSuitabilityNewsSensitive = "BRAND_SUITABILITY_NEWS_SENSITIVE",
        /// News from sources not featured on Google News or YouTube News.
        BrandSuitabilityNewsSourceNotFeatured = "BRAND_SUITABILITY_NEWS_SOURCE_NOT_FEATURED",
        /// Political content.
        BrandSuitabilityPolitics = "BRAND_SUITABILITY_POLITICS",
        /// Religious content.
        BrandSuitabilityReligion = "BRAND_SUITABILITY_RELIGION",
    }
}

impl ContentLabelType {
    /// The five digital content labels. Excluding every one of them leaves a
    /// Display campaign nothing to serve on.
    pub const DIGITAL_CONTENT_LABELS: [ContentLabelType; 5] = [
        ContentLabelType::VideoRatingDvG,
        ContentLabelType::VideoRatingDvPg,
        ContentLabelType::VideoRatingDvT,
        ContentLabelType::VideoRatingDvMa,
        ContentLabelType::VideoNotYetRated,
    ];

    /// Google's caveat for this label on Display inventory, when it has one:
    /// deprecated, or not applicable to Display.
    pub fn display_caveat(&self) -> Option<&'static str> {
        match self {
            ContentLabelType::ParkedDomain => Some(
                "deprecated: Google removed parked domains as an ad surface, and the \
                 parked-domain content setting, on 10 February 2026",
            ),
            ContentLabelType::BrandSuitabilityHealthSourceUndetermined => {
                Some("YouTube only: does not apply to Display inventory")
            }
            ContentLabelType::Video => {
                Some("not part of Google's current content suitability settings")
            }
            _ => None,
        }
    }

    /// True for the digital content labels (DL-G, DL-PG, DL-T, DL-MA, not yet
    /// labelled).
    pub fn is_digital_content_label(&self) -> bool {
        Self::DIGITAL_CONTENT_LABELS.contains(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_api_name() {
        for label in ContentLabelType::ALL {
            let wire = serde_json::to_string(label).unwrap();
            assert_eq!(wire, format!("\"{}\"", label.as_api_str()));
            let back: ContentLabelType = serde_json::from_str(&wire).unwrap();
            assert_eq!(&back, label);
        }
        // 27 values in the v25 enum, minus UNSPECIFIED and UNKNOWN.
        assert_eq!(ContentLabelType::ALL.len(), 25);
    }

    #[test]
    fn unwritable_and_unknown_values_are_rejected() {
        for bad in [
            "\"UNSPECIFIED\"",
            "\"UNKNOWN\"",
            "\"GAMES\"",
            "\"below_the_fold\"",
        ] {
            assert!(
                serde_json::from_str::<ContentLabelType>(bad).is_err(),
                "{bad} must not parse"
            );
        }
    }

    #[test]
    fn caveats_cover_deprecated_and_non_display_labels() {
        assert!(ContentLabelType::ParkedDomain
            .display_caveat()
            .unwrap()
            .contains("deprecated"));
        assert!(ContentLabelType::BrandSuitabilityHealthSourceUndetermined
            .display_caveat()
            .unwrap()
            .contains("YouTube only"));
        assert!(ContentLabelType::Video.display_caveat().is_some());
        assert!(ContentLabelType::BelowTheFold.display_caveat().is_none());
        assert!(ContentLabelType::Tragedy.display_caveat().is_none());
    }

    #[test]
    fn digital_content_labels_are_the_five_ratings() {
        let dl: Vec<_> = ContentLabelType::ALL
            .iter()
            .filter(|l| l.is_digital_content_label())
            .collect();
        assert_eq!(dl.len(), 5);
        assert!(!ContentLabelType::Video.is_digital_content_label());
    }
}
