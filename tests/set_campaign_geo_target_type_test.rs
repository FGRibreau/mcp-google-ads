//! Black-box + property-based coverage for `set_campaign_geo_target_type`.
//!
//! The tool sets `campaign.geo_target_type_setting` via a
//! `campaignOperation.update` with a matching field mask. These tests exercise
//! the public crate API only (no mocks) and inspect the stored plan.

use mcp_google_ads::config::Config;
use mcp_google_ads::models::GeoTargetType;
use mcp_google_ads::safety::preview::get_plan;
use mcp_google_ads::tools::campaigns_write::set_campaign_geo_target_type;
use proptest::prelude::*;

/// Fetch the sole `campaignOperation` from the stored plan behind a preview.
fn campaign_update_op(preview: &serde_json::Value) -> serde_json::Value {
    let plan_id = preview["plan_id"].as_str().expect("plan_id present");
    let plan = get_plan(plan_id).expect("plan stored");
    plan.mutate_operations
        .iter()
        .find(|op| op.get("campaignOperation").is_some())
        .expect("campaignOperation present")
        .clone()
}

#[test]
fn positive_presence_builds_expected_operation_and_mask() {
    let config = Config::default();
    let preview = set_campaign_geo_target_type(
        &config,
        "123-456-7890",
        "12345",
        Some(GeoTargetType::Presence),
        None,
    )
    .expect("plan builds");

    assert_eq!(preview["operation"], "set_campaign_geo_target_type");
    assert_eq!(preview["requires_double_confirm"], false);

    let op = campaign_update_op(&preview);
    assert_eq!(
        op["campaignOperation"]["update"]["resourceName"],
        "customers/1234567890/campaigns/12345"
    );
    assert_eq!(
        op["campaignOperation"]["update"]["geoTargetTypeSetting"]["positiveGeoTargetType"],
        "PRESENCE"
    );
    assert_eq!(
        op["campaignOperation"]["updateMask"],
        "geoTargetTypeSetting.positiveGeoTargetType"
    );
}

#[test]
fn both_none_is_rejected() {
    let config = Config::default();
    let err = set_campaign_geo_target_type(&config, "1234567890", "999", None, None)
        .expect_err("both None must be rejected");
    assert!(err.to_string().contains("No geo target type specified"));
}

/// Strategy over the two supported geo target type values.
fn geo_target_type() -> impl Strategy<Value = GeoTargetType> {
    prop_oneof![
        Just(GeoTargetType::Presence),
        Just(GeoTargetType::PresenceOrInterest),
    ]
}

proptest! {
    /// Invariant: whichever of {positive, negative} are `Some` are exactly the
    /// keys present in `geoTargetTypeSetting`, and each mask entry is present
    /// iff its field was set — with matching enum values. Also: at least one of
    /// the two must be Some for the plan to build at all.
    #[test]
    fn set_geo_target_type_plan_reflects_inputs(
        pos in proptest::option::of(geo_target_type()),
        neg in proptest::option::of(geo_target_type()),
        campaign_id in "[0-9]{1,15}",
    ) {
        let config = Config::default();
        let result =
            set_campaign_geo_target_type(&config, "1234567890", &campaign_id, pos, neg);

        if pos.is_none() && neg.is_none() {
            prop_assert!(result.is_err());
            return Ok(());
        }

        let preview = result.expect("plan builds when at least one field is set");
        let op = campaign_update_op(&preview);
        let setting = &op["campaignOperation"]["update"]["geoTargetTypeSetting"];
        let mask = op["campaignOperation"]["updateMask"]
            .as_str()
            .unwrap_or_default();

        match pos {
            Some(p) => {
                prop_assert_eq!(setting["positiveGeoTargetType"].as_str(), Some(p.as_api_str()));
                prop_assert!(mask.contains("geoTargetTypeSetting.positiveGeoTargetType"));
            }
            None => {
                prop_assert!(setting.get("positiveGeoTargetType").is_none());
                prop_assert!(!mask.contains("positiveGeoTargetType"));
            }
        }

        match neg {
            Some(n) => {
                prop_assert_eq!(setting["negativeGeoTargetType"].as_str(), Some(n.as_api_str()));
                prop_assert!(mask.contains("geoTargetTypeSetting.negativeGeoTargetType"));
            }
            None => {
                prop_assert!(setting.get("negativeGeoTargetType").is_none());
                prop_assert!(!mask.contains("negativeGeoTargetType"));
            }
        }

        // The resource name always encodes the (normalized) customer + campaign.
        let expected_resource = format!("customers/1234567890/campaigns/{}", campaign_id);
        prop_assert_eq!(
            op["campaignOperation"]["update"]["resourceName"].as_str(),
            Some(expected_resource.as_str())
        );
    }

    /// Round-trip invariant on the enum: serialize -> deserialize is identity,
    /// and the serialized form equals `as_api_str` (modulo JSON quoting).
    #[test]
    fn geo_target_type_round_trips(t in geo_target_type()) {
        let json = serde_json::to_string(&t).unwrap();
        let expected = format!("\"{}\"", t.as_api_str());
        prop_assert_eq!(json.as_str(), expected.as_str());
        let back: GeoTargetType = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, t);
    }
}
