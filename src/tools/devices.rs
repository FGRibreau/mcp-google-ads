//! Campaign device bid modifiers.
//!
//! A device adjustment is a `CampaignCriterion` with `device.type` and a
//! `bid_modifier`: a multiplier from 0.1 (−90%) to 10.0 (+900%), or 0 to opt
//! out of the device entirely (−100%). Device criteria are positive only and
//! campaign scoped; ad group device adjustments are a different resource
//! (`AdGroupBidModifier`) and not written here.
//!
//! A campaign may or may not already carry a criterion for a device, and
//! creating one that exists fails the whole mutate. So the MCP tool first
//! reads the campaign's device criteria ([`fetch_campaign_device_criteria`])
//! and the plan updates the ones that exist and creates the rest.
//!
//! Smart Bidding treats these adjustments its own way: with Target CPA a
//! device modifier adjusts the CPA target rather than the bid, and other
//! strategies may ignore positive adjustments. An opt-out (0) always applies.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::validate_numeric_id;

/// Smallest non-zero device bid modifier.
const MIN_BID_MODIFIER: f64 = 0.1;
/// Largest device bid modifier.
const MAX_BID_MODIFIER: f64 = 10.0;

/// The device modifiers to set. `None` leaves that device as it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceModifiers {
    pub desktop: Option<f64>,
    pub mobile: Option<f64>,
    pub tablet: Option<f64>,
    pub connected_tv: Option<f64>,
}

impl DeviceModifiers {
    /// `(API device type, requested modifier)` for every device given.
    fn requested(&self) -> Vec<(&'static str, f64)> {
        [
            ("DESKTOP", self.desktop),
            ("MOBILE", self.mobile),
            ("TABLET", self.tablet),
            ("CONNECTED_TV", self.connected_tv),
        ]
        .into_iter()
        .filter_map(|(device, value)| value.map(|v| (device, v)))
        .collect()
    }
}

/// A device criterion that already exists on the campaign.
#[derive(Debug, Clone, PartialEq)]
pub struct ExistingDeviceCriterion {
    /// API device type, e.g. `MOBILE`.
    pub device: String,
    /// `customers/{cid}/campaignCriteria/{campaign_id}~{criterion_id}`.
    pub resource_name: String,
    /// Current modifier; `None` when Google reports none.
    pub bid_modifier: Option<f64>,
}

/// Validate one modifier: 0 (opt out) or 0.1 to 10.0.
fn validate_modifier(device: &str, value: f64) -> Result<()> {
    let valid = value.is_finite()
        && (value == 0.0 || (MIN_BID_MODIFIER..=MAX_BID_MODIFIER).contains(&value));
    if !valid {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} bid modifier {} is out of range — use 0 to exclude the device (-100%) or a \
             multiplier from {} (-90%) to {} (+900%), e.g. 1.2 for +20%",
            device.to_ascii_lowercase(),
            value,
            MIN_BID_MODIFIER,
            MAX_BID_MODIFIER
        )));
    }
    Ok(())
}

/// How a modifier reads in the Google Ads UI, e.g. `+20%` or `-100% (excluded)`.
fn adjustment_label(value: f64) -> String {
    if value == 0.0 {
        return "-100% (device excluded)".to_string();
    }
    let pct = ((value - 1.0) * 100.0).round() as i64;
    if pct > 0 {
        format!("+{}%", pct)
    } else {
        format!("{}%", pct)
    }
}

/// Read the campaign's current device criteria.
///
/// `campaign_id` must already be validated as numeric: it is interpolated
/// into the GAQL query.
pub async fn fetch_campaign_device_criteria(
    client: &GoogleAdsClient,
    customer_id: &str,
    campaign_id: &str,
) -> Result<Vec<ExistingDeviceCriterion>> {
    let campaign = validate_numeric_id("campaign_id", campaign_id)?;
    let query = format!(
        "SELECT campaign_criterion.resource_name, campaign_criterion.device.type, \
         campaign_criterion.bid_modifier FROM campaign_criterion \
         WHERE campaign.id = {} AND campaign_criterion.type = 'DEVICE' \
         AND campaign_criterion.status != 'REMOVED'",
        campaign
    );
    let rows = client.search(customer_id, &query).await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let criterion = &row["campaignCriterion"];
            Some(ExistingDeviceCriterion {
                device: criterion.pointer("/device/type")?.as_str()?.to_string(),
                resource_name: criterion.get("resourceName")?.as_str()?.to_string(),
                bid_modifier: criterion.get("bidModifier").and_then(|v| v.as_f64()),
            })
        })
        .collect())
}

/// Validate a device modifier request before any HTTP traffic. Returns the
/// validated campaign ID.
pub fn prepare_device_modifiers(
    config: &Config,
    campaign_id: &str,
    modifiers: &DeviceModifiers,
) -> Result<String> {
    check_blocked_operation("set_campaign_device_modifiers", &config.safety)?;
    let campaign = validate_numeric_id("campaign_id", campaign_id)?;

    let requested = modifiers.requested();
    if requested.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "no device given — set at least one of desktop, mobile, tablet or connected_tv"
                .to_string(),
        ));
    }
    for (device, value) in &requested {
        validate_modifier(device, *value)?;
    }
    Ok(campaign)
}

/// Draft the device modifiers for a campaign, updating the device criteria
/// that exist and creating the rest.
///
/// Refuses a change that leaves desktop, mobile, tablet and connected TV all
/// at 0: the campaign would have no device left to serve on — pause it
/// instead.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
pub fn set_campaign_device_modifiers(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    modifiers: &DeviceModifiers,
    existing: &[ExistingDeviceCriterion],
) -> Result<serde_json::Value> {
    let campaign = prepare_device_modifiers(config, campaign_id, modifiers)?;
    let requested = modifiers.requested();

    let effective = |device: &str| -> Option<f64> {
        requested
            .iter()
            .find(|(d, _)| *d == device)
            .map(|(_, v)| *v)
            .or_else(|| {
                existing
                    .iter()
                    .find(|e| e.device == device)
                    .and_then(|e| e.bid_modifier)
            })
    };
    if ["DESKTOP", "MOBILE", "TABLET", "CONNECTED_TV"]
        .iter()
        .all(|d| effective(d) == Some(0.0))
    {
        return Err(McpGoogleAdsError::Validation(
            "this would exclude desktop, mobile, tablet and connected TV alike — the campaign \
             could not serve anywhere. Pause the campaign instead."
                .to_string(),
        ));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign);

    let mut operations = Vec::with_capacity(requested.len());
    let mut summary = Vec::with_capacity(requested.len());
    for (device, value) in &requested {
        let current = existing.iter().find(|e| e.device == *device);
        match current {
            Some(row) => {
                operations.push(json!({
                    "campaignCriterionOperation": {
                        "update": {
                            "resourceName": row.resource_name,
                            "bidModifier": value
                        },
                        "updateMask": "bidModifier"
                    }
                }));
            }
            None => {
                operations.push(json!({
                    "campaignCriterionOperation": {
                        "create": {
                            "campaign": campaign_resource,
                            "device": { "type": device },
                            "bidModifier": value
                        }
                    }
                }));
            }
        }
        summary.push(json!({
            "device": device,
            "bid_modifier": value,
            "adjustment": adjustment_label(*value),
            "action": if current.is_some() { "update" } else { "create" },
            "previous_bid_modifier": current.and_then(|c| c.bid_modifier),
        }));
    }

    let changes = json!({
        "campaign_id": campaign,
        "devices": summary,
        "note": "0 excludes the device (-100%). With Smart Bidding, positive and negative \
                 adjustments may be ignored or, with Target CPA, adjust the CPA target \
                 instead of the bid; an exclusion always applies.",
    });

    let plan = ChangePlan::new(
        "set_campaign_device_modifiers".to_string(),
        "campaign_criterion".to_string(),
        campaign,
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::preview::{get_plan, remove_plan};

    fn ops(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        let plan_id = preview["plan_id"].as_str().expect("plan_id");
        let plan = get_plan(plan_id).expect("stored");
        remove_plan(plan_id);
        plan.mutate_operations
    }

    fn existing(device: &str, criterion: &str, modifier: Option<f64>) -> ExistingDeviceCriterion {
        ExistingDeviceCriterion {
            device: device.to_string(),
            resource_name: format!("customers/1234567890/campaignCriteria/555~{}", criterion),
            bid_modifier: modifier,
        }
    }

    #[test]
    fn creates_missing_and_updates_existing_device_criteria() {
        let config = Config::default();
        let preview = set_campaign_device_modifiers(
            &config,
            "123-456-7890",
            "555",
            &DeviceModifiers {
                mobile: Some(0.0),
                desktop: Some(1.2),
                ..Default::default()
            },
            &[existing("MOBILE", "30001", Some(1.0))],
        )
        .expect("ok");

        assert_eq!(preview["operation"], "set_campaign_device_modifiers");
        let ops = ops(&preview);
        assert_eq!(ops.len(), 2);
        // Desktop: no row yet -> create.
        let create = &ops[0]["campaignCriterionOperation"]["create"];
        assert_eq!(create["campaign"], "customers/1234567890/campaigns/555");
        assert_eq!(create["device"]["type"], "DESKTOP");
        assert_eq!(create["bidModifier"], 1.2);
        // Mobile: existing row -> update with mask.
        let update = &ops[1]["campaignCriterionOperation"];
        assert_eq!(
            update["update"]["resourceName"],
            "customers/1234567890/campaignCriteria/555~30001"
        );
        assert_eq!(update["update"]["bidModifier"], 0.0);
        assert_eq!(update["updateMask"], "bidModifier");
        assert_eq!(
            preview["changes"]["devices"][1]["adjustment"],
            "-100% (device excluded)"
        );
        assert_eq!(preview["changes"]["devices"][0]["adjustment"], "+20%");
    }

    #[test]
    fn rejects_out_of_range_empty_and_all_excluded() {
        let config = Config::default();
        for bad in [0.05, 10.5, -1.0, f64::NAN] {
            let err = set_campaign_device_modifiers(
                &config,
                "1",
                "555",
                &DeviceModifiers {
                    tablet: Some(bad),
                    ..Default::default()
                },
                &[],
            );
            assert!(err.is_err(), "{bad} must be refused");
        }

        let none =
            set_campaign_device_modifiers(&config, "1", "555", &DeviceModifiers::default(), &[]);
        assert!(none.unwrap_err().to_string().contains("no device"));

        // Three zeros requested plus an existing zero = nothing left.
        let all_off = set_campaign_device_modifiers(
            &config,
            "1",
            "555",
            &DeviceModifiers {
                desktop: Some(0.0),
                mobile: Some(0.0),
                connected_tv: Some(0.0),
                tablet: None,
            },
            &[existing("TABLET", "30002", Some(0.0))],
        );
        assert!(all_off.unwrap_err().to_string().contains("Pause"));

        let bad_campaign = set_campaign_device_modifiers(
            &config,
            "1",
            "Display",
            &DeviceModifiers {
                mobile: Some(0.0),
                ..Default::default()
            },
            &[],
        );
        assert!(bad_campaign.unwrap_err().to_string().contains("numeric"));
    }

    #[test]
    fn adjustment_labels() {
        assert_eq!(adjustment_label(1.0), "0%");
        assert_eq!(adjustment_label(0.8), "-20%");
        assert_eq!(adjustment_label(10.0), "+900%");
    }
}
