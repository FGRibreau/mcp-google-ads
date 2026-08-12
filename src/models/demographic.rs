//! Demographic criterion enums (income range, age range, gender).
//!
//! These map to the Google Ads API `AdGroupCriterion` demographic criteria.
//! Each variant serialises to the API enum string (SCREAMING_SNAKE_CASE) and
//! also carries the **fixed** criterion ID Google assigns to it.
//!
//! Two things worth knowing before touching this module:
//!
//! 1. **Creating a criterion does not need the ID.** You send
//!    `{"incomeRange": {"type": "INCOME_RANGE_0_50"}}` and Google resolves the
//!    criterion ID itself. The `criterion_id()` table exists for the *removal*
//!    path, where the resource name `adGroupCriteria/{ad_group}~{id}` has to be
//!    built by hand, and for echoing the id back in previews.
//!
//! 2. **Demographic rows are not implicit.** A fresh ad group has *no*
//!    demographic criteria at all and serves to everyone. A negative criterion
//!    can therefore be created directly. But if a tier already exists as an
//!    explicit *positive* row, creating the negative collides on the shared
//!    resource name — the positive row must be removed first. That is what
//!    [`super::super::tools::demographics::remove_demographic_criterion`] is for.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Household income percentile band, as used by `AdGroupCriterion.income_range`.
///
/// Bands are expressed as percentile of household income **within the targeted
/// country**, top-down: `INCOME_RANGE_90_UP` is the top 10% of earners,
/// `INCOME_RANGE_0_50` the lower 50%. `INCOME_RANGE_UNDETERMINED` is the
/// "Unknown" bucket — users Google could not place in a band, which in small
/// or rural markets is frequently the largest single bucket.
///
/// Every variant carries an explicit `#[serde(rename)]`. Do not replace them
/// with `rename_all = "SCREAMING_SNAKE_CASE"`: serde does not insert a separator
/// before a digit, so `IncomeRange0_50` would derive to `INCOME_RANGE0_50` and
/// every income mutate would be rejected.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
pub enum IncomeRange {
    /// Lower 50% of household income.
    #[serde(rename = "INCOME_RANGE_0_50")]
    IncomeRange0_50,
    /// 41-50% band.
    #[serde(rename = "INCOME_RANGE_50_60")]
    IncomeRange50_60,
    /// 31-40% band.
    #[serde(rename = "INCOME_RANGE_60_70")]
    IncomeRange60_70,
    /// 21-30% band.
    #[serde(rename = "INCOME_RANGE_70_80")]
    IncomeRange70_80,
    /// 11-20% band.
    #[serde(rename = "INCOME_RANGE_80_90")]
    IncomeRange80_90,
    /// Top 10% of household income.
    #[serde(rename = "INCOME_RANGE_90_UP")]
    IncomeRange90Up,
    /// "Unknown" — income band could not be determined for the user.
    #[serde(rename = "INCOME_RANGE_UNDETERMINED")]
    IncomeRangeUndetermined,
}

impl IncomeRange {
    /// String matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            IncomeRange::IncomeRange0_50 => "INCOME_RANGE_0_50",
            IncomeRange::IncomeRange50_60 => "INCOME_RANGE_50_60",
            IncomeRange::IncomeRange60_70 => "INCOME_RANGE_60_70",
            IncomeRange::IncomeRange70_80 => "INCOME_RANGE_70_80",
            IncomeRange::IncomeRange80_90 => "INCOME_RANGE_80_90",
            IncomeRange::IncomeRange90Up => "INCOME_RANGE_90_UP",
            IncomeRange::IncomeRangeUndetermined => "INCOME_RANGE_UNDETERMINED",
        }
    }

    /// The fixed criterion ID Google assigns to this band.
    pub fn criterion_id(&self) -> u64 {
        match self {
            IncomeRange::IncomeRangeUndetermined => 510_000,
            IncomeRange::IncomeRange0_50 => 510_001,
            IncomeRange::IncomeRange50_60 => 510_002,
            IncomeRange::IncomeRange60_70 => 510_003,
            IncomeRange::IncomeRange70_80 => 510_004,
            IncomeRange::IncomeRange80_90 => 510_005,
            IncomeRange::IncomeRange90Up => 510_006,
        }
    }

    /// The bands the clinic playbook excludes by default: the lower 50% plus
    /// the "Unknown" bucket. Exposed so callers can name the intent rather than
    /// re-listing the bands, and so the pairing stays in one place.
    pub fn bottom_50_and_unknown() -> Vec<IncomeRange> {
        vec![
            IncomeRange::IncomeRange0_50,
            IncomeRange::IncomeRangeUndetermined,
        ]
    }
}

impl std::fmt::Display for IncomeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

/// Age bracket, as used by `AdGroupCriterion.age_range`.
///
/// Explicit renames for the same reason as [`IncomeRange`] — serde would derive
/// `AGE_RANGE18_24` without them.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
pub enum AgeRange {
    #[serde(rename = "AGE_RANGE_18_24")]
    AgeRange18_24,
    #[serde(rename = "AGE_RANGE_25_34")]
    AgeRange25_34,
    #[serde(rename = "AGE_RANGE_35_44")]
    AgeRange35_44,
    #[serde(rename = "AGE_RANGE_45_54")]
    AgeRange45_54,
    #[serde(rename = "AGE_RANGE_55_64")]
    AgeRange55_64,
    #[serde(rename = "AGE_RANGE_65_UP")]
    AgeRange65Up,
    /// "Unknown" — age could not be determined for the user.
    #[serde(rename = "AGE_RANGE_UNDETERMINED")]
    AgeRangeUndetermined,
}

impl AgeRange {
    /// String matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            AgeRange::AgeRange18_24 => "AGE_RANGE_18_24",
            AgeRange::AgeRange25_34 => "AGE_RANGE_25_34",
            AgeRange::AgeRange35_44 => "AGE_RANGE_35_44",
            AgeRange::AgeRange45_54 => "AGE_RANGE_45_54",
            AgeRange::AgeRange55_64 => "AGE_RANGE_55_64",
            AgeRange::AgeRange65Up => "AGE_RANGE_65_UP",
            AgeRange::AgeRangeUndetermined => "AGE_RANGE_UNDETERMINED",
        }
    }

    /// The fixed criterion ID Google assigns to this bracket.
    pub fn criterion_id(&self) -> u64 {
        match self {
            AgeRange::AgeRange18_24 => 503_001,
            AgeRange::AgeRange25_34 => 503_002,
            AgeRange::AgeRange35_44 => 503_003,
            AgeRange::AgeRange45_54 => 503_004,
            AgeRange::AgeRange55_64 => 503_005,
            AgeRange::AgeRange65Up => 503_006,
            AgeRange::AgeRangeUndetermined => 503_999,
        }
    }
}

impl std::fmt::Display for AgeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

/// Gender, as used by `AdGroupCriterion.gender`.
///
/// Note the JSON surface is prefixed (`GENDER_MALE`) but the API wire value is
/// not (`MALE`) — see [`Gender::as_api_str`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, JsonSchema)]
pub enum Gender {
    #[serde(rename = "GENDER_MALE")]
    GenderMale,
    #[serde(rename = "GENDER_FEMALE")]
    GenderFemale,
    /// "Unknown" — gender could not be determined for the user.
    #[serde(rename = "GENDER_UNDETERMINED")]
    GenderUndetermined,
}

impl Gender {
    /// String matching the Google Ads REST API enum value.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Gender::GenderMale => "MALE",
            Gender::GenderFemale => "FEMALE",
            Gender::GenderUndetermined => "UNDETERMINED",
        }
    }

    /// The fixed criterion ID Google assigns to this gender.
    pub fn criterion_id(&self) -> u64 {
        match self {
            Gender::GenderMale => 10,
            Gender::GenderFemale => 11,
            Gender::GenderUndetermined => 20,
        }
    }
}

impl std::fmt::Display for Gender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn income_serializes_to_api_enum_strings() {
        assert_eq!(
            serde_json::to_string(&IncomeRange::IncomeRange0_50).unwrap(),
            "\"INCOME_RANGE_0_50\""
        );
        assert_eq!(
            serde_json::to_string(&IncomeRange::IncomeRange90Up).unwrap(),
            "\"INCOME_RANGE_90_UP\""
        );
        assert_eq!(
            serde_json::to_string(&IncomeRange::IncomeRangeUndetermined).unwrap(),
            "\"INCOME_RANGE_UNDETERMINED\""
        );
    }

    #[test]
    fn income_serde_roundtrips_through_api_str() {
        // as_api_str() and the serde representation must not drift apart —
        // the mutate payload uses one and the preview echoes the other.
        for band in [
            IncomeRange::IncomeRange0_50,
            IncomeRange::IncomeRange50_60,
            IncomeRange::IncomeRange60_70,
            IncomeRange::IncomeRange70_80,
            IncomeRange::IncomeRange80_90,
            IncomeRange::IncomeRange90Up,
            IncomeRange::IncomeRangeUndetermined,
        ] {
            let json = serde_json::to_string(&band).unwrap();
            assert_eq!(json, format!("\"{}\"", band.as_api_str()));
            let back: IncomeRange = serde_json::from_str(&json).unwrap();
            assert_eq!(back, band);
        }
    }

    /// The two IDs verified against a live account (Dra. Iara, 2026-08-11).
    #[test]
    fn income_criterion_ids_match_live_account() {
        assert_eq!(IncomeRange::IncomeRangeUndetermined.criterion_id(), 510_000);
        assert_eq!(IncomeRange::IncomeRange0_50.criterion_id(), 510_001);
    }

    #[test]
    fn income_criterion_ids_are_unique_and_ordered() {
        let ids: Vec<u64> = [
            IncomeRange::IncomeRangeUndetermined,
            IncomeRange::IncomeRange0_50,
            IncomeRange::IncomeRange50_60,
            IncomeRange::IncomeRange60_70,
            IncomeRange::IncomeRange70_80,
            IncomeRange::IncomeRange80_90,
            IncomeRange::IncomeRange90Up,
        ]
        .iter()
        .map(|b| b.criterion_id())
        .collect();

        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "criterion ids must be unique");
        assert_eq!(sorted, ids, "ids run consecutively from UNDETERMINED");
    }

    #[test]
    fn bottom_50_and_unknown_is_the_documented_pair() {
        let bands = IncomeRange::bottom_50_and_unknown();
        assert_eq!(bands.len(), 2);
        assert!(bands.contains(&IncomeRange::IncomeRange0_50));
        assert!(bands.contains(&IncomeRange::IncomeRangeUndetermined));
    }

    /// Pin the exact JSON spelling of every variant against a literal table.
    ///
    /// This is the test that caught serde's `rename_all` dropping the separator
    /// before a digit (`INCOME_RANGE0_50`). Because the JSON form is what the
    /// caller types and `as_api_str` is what goes on the wire, a drift between
    /// them is silent until Google rejects the mutate.
    #[test]
    fn every_variant_has_the_exact_expected_json_spelling() {
        let income: Vec<(IncomeRange, &str)> = vec![
            (IncomeRange::IncomeRange0_50, "INCOME_RANGE_0_50"),
            (IncomeRange::IncomeRange50_60, "INCOME_RANGE_50_60"),
            (IncomeRange::IncomeRange60_70, "INCOME_RANGE_60_70"),
            (IncomeRange::IncomeRange70_80, "INCOME_RANGE_70_80"),
            (IncomeRange::IncomeRange80_90, "INCOME_RANGE_80_90"),
            (IncomeRange::IncomeRange90Up, "INCOME_RANGE_90_UP"),
            (
                IncomeRange::IncomeRangeUndetermined,
                "INCOME_RANGE_UNDETERMINED",
            ),
        ];
        for (variant, expected) in income {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                format!("\"{}\"", expected)
            );
            assert_eq!(variant.as_api_str(), expected);
            assert_eq!(
                serde_json::from_str::<IncomeRange>(&format!("\"{}\"", expected)).unwrap(),
                variant
            );
        }

        let age: Vec<(AgeRange, &str)> = vec![
            (AgeRange::AgeRange18_24, "AGE_RANGE_18_24"),
            (AgeRange::AgeRange25_34, "AGE_RANGE_25_34"),
            (AgeRange::AgeRange35_44, "AGE_RANGE_35_44"),
            (AgeRange::AgeRange45_54, "AGE_RANGE_45_54"),
            (AgeRange::AgeRange55_64, "AGE_RANGE_55_64"),
            (AgeRange::AgeRange65Up, "AGE_RANGE_65_UP"),
            (AgeRange::AgeRangeUndetermined, "AGE_RANGE_UNDETERMINED"),
        ];
        for (variant, expected) in age {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                format!("\"{}\"", expected)
            );
            assert_eq!(variant.as_api_str(), expected);
            assert_eq!(
                serde_json::from_str::<AgeRange>(&format!("\"{}\"", expected)).unwrap(),
                variant
            );
        }

        // Gender is deliberately asymmetric: prefixed in JSON, bare on the wire.
        let gender: Vec<(Gender, &str, &str)> = vec![
            (Gender::GenderMale, "GENDER_MALE", "MALE"),
            (Gender::GenderFemale, "GENDER_FEMALE", "FEMALE"),
            (
                Gender::GenderUndetermined,
                "GENDER_UNDETERMINED",
                "UNDETERMINED",
            ),
        ];
        for (variant, json_form, wire_form) in gender {
            assert_eq!(
                serde_json::to_string(&variant).unwrap(),
                format!("\"{}\"", json_form)
            );
            assert_eq!(variant.as_api_str(), wire_form);
            assert_eq!(
                serde_json::from_str::<Gender>(&format!("\"{}\"", json_form)).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn age_ids_are_unique() {
        let ids: Vec<u64> = [
            AgeRange::AgeRange18_24,
            AgeRange::AgeRange25_34,
            AgeRange::AgeRange35_44,
            AgeRange::AgeRange45_54,
            AgeRange::AgeRange55_64,
            AgeRange::AgeRange65Up,
            AgeRange::AgeRangeUndetermined,
        ]
        .iter()
        .map(|a| a.criterion_id())
        .collect();
        let mut dedup = ids.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(dedup.len(), ids.len());
    }

    /// Gender is the one enum whose API string is NOT the serde variant name:
    /// the wire value is `MALE`, not `GENDER_MALE`. Guard against someone
    /// "tidying" `as_api_str` to match the serde form and silently breaking
    /// every gender mutate.
    #[test]
    fn gender_api_string_is_unprefixed() {
        assert_eq!(Gender::GenderMale.as_api_str(), "MALE");
        assert_eq!(Gender::GenderFemale.as_api_str(), "FEMALE");
        assert_eq!(Gender::GenderUndetermined.as_api_str(), "UNDETERMINED");

        // ...while the JSON surface keeps the prefixed, self-describing form.
        assert_eq!(
            serde_json::to_string(&Gender::GenderFemale).unwrap(),
            "\"GENDER_FEMALE\""
        );
    }

    #[test]
    fn gender_criterion_ids() {
        assert_eq!(Gender::GenderMale.criterion_id(), 10);
        assert_eq!(Gender::GenderFemale.criterion_id(), 11);
        assert_eq!(Gender::GenderUndetermined.criterion_id(), 20);
    }

    #[test]
    fn rejects_unknown_variants() {
        assert!(serde_json::from_str::<IncomeRange>("\"INCOME_RANGE_TOP_1\"").is_err());
        assert!(serde_json::from_str::<AgeRange>("\"AGE_RANGE_13_17\"").is_err());
        assert!(serde_json::from_str::<Gender>("\"MALE\"").is_err());
    }
}
