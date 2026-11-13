//! Custom segment (API: `CustomAudience`) type.
//!
//! The v25 enum also has `INTEREST` and `PURCHASE_INTENT`, but the resource
//! documentation says neither "is allowed for newly created custom audience but
//! kept for existing audiences". They are left out so a create can only carry a
//! type Google accepts.

api_enum! {
    /// How Google interprets a new custom segment's members.
    pub enum CustomSegmentType {
        /// Google picks the best interpretation at serving time (the default).
        Auto = "AUTO",
        /// People who searched for the keywords on Google Search.
        Search = "SEARCH",
    }
}

impl CustomSegmentType {
    /// The type a new segment gets when the caller names none.
    pub const DEFAULT: CustomSegmentType = CustomSegmentType::Auto;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_auto_and_round_trips() {
        assert_eq!(CustomSegmentType::DEFAULT, CustomSegmentType::Auto);
        for t in CustomSegmentType::ALL {
            let wire = serde_json::to_string(t).unwrap();
            assert_eq!(wire, format!("\"{}\"", t.as_api_str()));
        }
    }

    #[test]
    fn types_google_refuses_for_new_segments_do_not_parse() {
        for bad in ["\"INTEREST\"", "\"PURCHASE_INTENT\"", "\"auto\""] {
            assert!(serde_json::from_str::<CustomSegmentType>(bad).is_err());
        }
    }
}
