use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::ids::{dedupe_preserving_order, validate_numeric_id};

const VALID_ENTITY_TYPES: &[&str] = &["campaign", "ad_group", "ad", "keyword"];

/// Build the resource name and operation key for a given entity type.
fn entity_resource_and_op(
    cid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<(String, String)> {
    let (resource_path, op_key) = match entity_type {
        "campaign" => (
            format!("customers/{}/campaigns/{}", cid, entity_id),
            "campaignOperation".to_string(),
        ),
        "ad_group" => (
            format!("customers/{}/adGroups/{}", cid, entity_id),
            "adGroupOperation".to_string(),
        ),
        "ad" => (
            format!("customers/{}/adGroupAds/{}", cid, entity_id),
            "adGroupAdOperation".to_string(),
        ),
        "keyword" => (
            format!("customers/{}/adGroupCriteria/{}", cid, entity_id),
            "adGroupCriterionOperation".to_string(),
        ),
        _ => {
            return Err(McpGoogleAdsError::Validation(format!(
                "Invalid entity type '{}'. Must be one of: {}",
                entity_type,
                VALID_ENTITY_TYPES.join(", ")
            )));
        }
    };
    Ok((resource_path, op_key))
}

/// Build a status-change operation for an entity.
fn build_status_operation(
    cid: &str,
    entity_type: &str,
    entity_id: &str,
    status: &str,
) -> Result<serde_json::Value> {
    let (resource_name, op_key) = entity_resource_and_op(cid, entity_type, entity_id)?;

    Ok(json!({
        op_key: {
            "update": {
                "resourceName": resource_name,
                "status": status
            },
            "updateMask": "status"
        }
    }))
}

/// Build a remove operation for an entity (uses the dedicated `remove` field).
fn build_remove_operation(
    cid: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<serde_json::Value> {
    let (resource_name, op_key) = entity_resource_and_op(cid, entity_type, entity_id)?;

    Ok(json!({
        op_key: {
            "remove": resource_name
        }
    }))
}

/// Pause an entity (campaign, ad group, ad, or keyword).
///
/// Sets the entity's status to PAUSED.
pub fn pause_entity(
    config: &Config,
    customer_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("pause_entity", &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let operation = build_status_operation(&cid, entity_type, entity_id, "PAUSED")?;

    let changes = json!({
        "entity_type": entity_type,
        "entity_id": entity_id,
        "new_status": "PAUSED"
    });

    let plan = ChangePlan::new(
        "pause_entity".to_string(),
        entity_type.to_string(),
        entity_id.to_string(),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Enable an entity (campaign, ad group, ad, or keyword).
///
/// Sets the entity's status to ENABLED.
pub fn enable_entity(
    config: &Config,
    customer_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("enable_entity", &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let operation = build_status_operation(&cid, entity_type, entity_id, "ENABLED")?;

    let changes = json!({
        "entity_type": entity_type,
        "entity_id": entity_id,
        "new_status": "ENABLED"
    });

    let plan = ChangePlan::new(
        "enable_entity".to_string(),
        entity_type.to_string(),
        entity_id.to_string(),
        cid,
        changes,
        false,
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Remove an entity (campaign, ad group, ad, or keyword).
///
/// Uses the dedicated `remove` operation in the mutate request. This is a
/// destructive operation and requires double confirmation.
pub fn remove_entity(
    config: &Config,
    customer_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_entity", &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let operation = build_remove_operation(&cid, entity_type, entity_id)?;

    let changes = json!({
        "entity_type": entity_type,
        "entity_id": entity_id,
        "new_status": "REMOVED",
        "warning": "This action is destructive and cannot be undone"
    });

    let plan = ChangePlan::new(
        "remove_entity".to_string(),
        entity_type.to_string(),
        entity_id.to_string(),
        cid,
        changes,
        true, // requires double confirmation
        vec![operation],
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// At most this many entities in one batch status change.
pub const MAX_ENTITIES_PER_PLAN: usize = 500;

/// The entities a batch status change touches, by type. Ads and keywords are
/// `adGroupId~id`, the tail of their resource names.
#[derive(Debug, Default)]
pub struct EntityIds<'a> {
    pub campaign_ids: &'a [String],
    pub ad_group_ids: &'a [String],
    pub ad_ids: &'a [String],
    pub keyword_ids: &'a [String],
}

/// Validate an `adGroupId~id` (ads and keywords): two bare positive integers.
fn validate_composite_id(field: &str, id: &str) -> Result<String> {
    let trimmed = id.trim();
    let mut parts = trimmed.split('~');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(ad_group), Some(item), None) => Ok(format!(
            "{}~{}",
            validate_numeric_id(field, ad_group)?,
            validate_numeric_id(field, item)?
        )),
        _ => Err(McpGoogleAdsError::Validation(format!(
            "{} entry '{}' must be 'adGroupId~id', the tail of its resource name \
             (e.g. '9000000101~9000001001')",
            field, id
        ))),
    }
}

/// Set the status of many campaigns, ad groups, ads and keywords in one
/// atomic mutate: every status changes, or none does.
fn set_entities_status(
    config: &Config,
    customer_id: &str,
    ids: &EntityIds,
    status: &str,
    operation_name: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation(operation_name, &config.safety)?;
    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let kinds: [(&str, &str, &[String], bool); 4] = [
        ("campaign", "campaign_ids", ids.campaign_ids, false),
        ("ad_group", "ad_group_ids", ids.ad_group_ids, false),
        ("ad", "ad_ids", ids.ad_ids, true),
        ("keyword", "keyword_ids", ids.keyword_ids, true),
    ];
    let mut operations = Vec::new();
    let mut changes = serde_json::Map::new();
    for (entity_type, field, raw, composite) in kinds {
        let validated = raw
            .iter()
            .map(|id| {
                if composite {
                    validate_composite_id(field, id)
                } else {
                    validate_numeric_id(field, id)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let unique = dedupe_preserving_order(&validated);
        if unique.is_empty() {
            continue;
        }
        for id in &unique {
            operations.push(build_status_operation(&cid, entity_type, id, status)?);
        }
        changes.insert(field.to_string(), json!(unique));
    }

    if operations.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "nothing to change — pass at least one of campaign_ids, ad_group_ids, ad_ids \
             or keyword_ids"
                .to_string(),
        ));
    }
    if operations.len() > MAX_ENTITIES_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} entities, over the {} per plan — split them into smaller batches",
            operations.len(),
            MAX_ENTITIES_PER_PLAN
        )));
    }
    changes.insert("new_status".to_string(), json!(status));
    changes.insert("total".to_string(), json!(operations.len()));
    changes.insert(
        "note".to_string(),
        json!("One atomic mutate: every status changes, or none does."),
    );

    let plan = ChangePlan::new(
        operation_name.to_string(),
        "entities".to_string(),
        "batch".to_string(),
        cid,
        serde_json::Value::Object(changes),
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Enable many campaigns, ad groups, ads and keywords at once.
pub fn enable_entities(
    config: &Config,
    customer_id: &str,
    ids: &EntityIds,
) -> Result<serde_json::Value> {
    set_entities_status(config, customer_id, ids, "ENABLED", "enable_entities")
}

/// Pause many campaigns, ad groups, ads and keywords at once.
pub fn pause_entities(
    config: &Config,
    customer_id: &str,
    ids: &EntityIds,
) -> Result<serde_json::Value> {
    set_entities_status(config, customer_id, ids, "PAUSED", "pause_entities")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_pause_entity_invalid_type() {
        let config = Config::default();
        let result = pause_entity(&config, "123-456-7890", "invalid", "123");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("Invalid entity type"));
    }

    #[test]
    fn test_pause_campaign() {
        let config = Config::default();
        let result = pause_entity(&config, "123-456-7890", "campaign", "12345");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "pause_entity");
        assert_eq!(preview["changes"]["new_status"], "PAUSED");
    }

    #[test]
    fn test_enable_ad_group() {
        let config = Config::default();
        let result = enable_entity(&config, "123-456-7890", "ad_group", "67890");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "enable_entity");
        assert_eq!(preview["changes"]["new_status"], "ENABLED");
    }

    #[test]
    fn test_remove_entity_requires_double_confirm() {
        let config = Config::default();
        let result = remove_entity(&config, "123-456-7890", "campaign", "12345");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "remove_entity");
        assert_eq!(preview["requires_double_confirm"], true);
        assert_eq!(preview["changes"]["new_status"], "REMOVED");
    }

    #[test]
    fn test_pause_entity_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["pause_entity".to_string()];
        let result = pause_entity(&config, "123-456-7890", "campaign", "12345");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_entity_resource_names() {
        let (res, op) = entity_resource_and_op("1234567890", "campaign", "555")
            .ok()
            .unwrap_or_default();
        assert_eq!(res, "customers/1234567890/campaigns/555");
        assert_eq!(op, "campaignOperation");

        let (res, op) = entity_resource_and_op("1234567890", "ad_group", "666")
            .ok()
            .unwrap_or_default();
        assert_eq!(res, "customers/1234567890/adGroups/666");
        assert_eq!(op, "adGroupOperation");

        let (res, op) = entity_resource_and_op("1234567890", "ad", "777")
            .ok()
            .unwrap_or_default();
        assert_eq!(res, "customers/1234567890/adGroupAds/777");
        assert_eq!(op, "adGroupAdOperation");

        let (res, op) = entity_resource_and_op("1234567890", "keyword", "888")
            .ok()
            .unwrap_or_default();
        assert_eq!(res, "customers/1234567890/adGroupCriteria/888");
        assert_eq!(op, "adGroupCriterionOperation");
    }

    #[test]
    fn test_enable_entity_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["enable_entity".to_string()];
        let result = enable_entity(&config, "123-456-7890", "campaign", "12345");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_remove_entity_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["remove_entity".to_string()];
        let result = remove_entity(&config, "123-456-7890", "campaign", "12345");
        assert!(result.is_err());
        let err = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains("blocked"));
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn test_enable_entities_is_one_atomic_plan_across_types() {
        let config = Config::default();
        let campaigns = strings(&["9000000001"]);
        let ad_groups = strings(&["9000000101", " 9000000102 "]);
        let ads = strings(&[
            "9000000101~9000001001",
            "9000000102~9000001002",
            "9000000101~9000001001",
        ]);
        let ids = EntityIds {
            campaign_ids: &campaigns,
            ad_group_ids: &ad_groups,
            ad_ids: &ads,
            ..Default::default()
        };
        let preview = enable_entities(&config, "123-456-7890", &ids).expect("plan builds");
        assert_eq!(preview["operation"], "enable_entities");
        assert_eq!(preview["requires_double_confirm"], false);
        assert_eq!(preview["changes"]["new_status"], "ENABLED");
        assert_eq!(preview["changes"]["total"], 5, "the repeated ad is dropped");
        assert_eq!(
            preview["changes"]["ad_group_ids"],
            json!(["9000000101", "9000000102"])
        );
        assert!(preview["changes"].get("keyword_ids").is_none());

        let plan_id = preview["plan_id"].as_str().expect("plan_id");
        let plan = crate::safety::preview::get_plan(plan_id).expect("stored");
        crate::safety::preview::remove_plan(plan_id);
        assert_eq!(plan.mutate_operations.len(), 5);
        assert_eq!(
            plan.mutate_operations[0],
            json!({"campaignOperation": {
                "update": {"resourceName": "customers/1234567890/campaigns/9000000001", "status": "ENABLED"},
                "updateMask": "status"
            }})
        );
        assert_eq!(
            plan.mutate_operations[3]["adGroupAdOperation"]["update"]["resourceName"],
            "customers/1234567890/adGroupAds/9000000101~9000001001"
        );
    }

    #[test]
    fn test_pause_entities_sets_paused() {
        let config = Config::default();
        let keywords = strings(&["9000000104~123"]);
        let ids = EntityIds {
            keyword_ids: &keywords,
            ..Default::default()
        };
        let preview = pause_entities(&config, "123-456-7890", &ids).expect("plan builds");
        assert_eq!(preview["operation"], "pause_entities");
        assert_eq!(preview["changes"]["new_status"], "PAUSED");
        let plan_id = preview["plan_id"].as_str().expect("plan_id");
        let plan = crate::safety::preview::get_plan(plan_id).expect("stored");
        crate::safety::preview::remove_plan(plan_id);
        assert_eq!(
            plan.mutate_operations[0]["adGroupCriterionOperation"]["update"]["status"],
            "PAUSED"
        );
    }

    #[test]
    fn test_entities_validation() {
        let config = Config::default();
        let err = |ids: EntityIds| {
            enable_entities(&config, "123-456-7890", &ids)
                .unwrap_err()
                .to_string()
        };
        assert!(err(EntityIds::default()).contains("nothing to change"));

        let bare_ad = strings(&["9000001001"]);
        assert!(err(EntityIds {
            ad_ids: &bare_ad,
            ..Default::default()
        })
        .contains("adGroupId~id"));
        let bad_campaign = strings(&["9000000001x"]);
        assert!(enable_entities(
            &config,
            "123-456-7890",
            &EntityIds {
                campaign_ids: &bad_campaign,
                ..Default::default()
            }
        )
        .is_err());
        let three_parts = strings(&["1~2~3"]);
        assert!(err(EntityIds {
            keyword_ids: &three_parts,
            ..Default::default()
        })
        .contains("adGroupId~id"));

        let many: Vec<String> = (1..=MAX_ENTITIES_PER_PLAN + 1)
            .map(|i| i.to_string())
            .collect();
        assert!(err(EntityIds {
            ad_group_ids: &many,
            ..Default::default()
        })
        .contains("per plan"));

        let mut blocked = Config::default();
        blocked.safety.blocked_operations = vec!["pause_entities".to_string()];
        let campaigns = strings(&["1"]);
        assert!(pause_entities(
            &blocked,
            "123-456-7890",
            &EntityIds {
                campaign_ids: &campaigns,
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("blocked"));
    }
}
