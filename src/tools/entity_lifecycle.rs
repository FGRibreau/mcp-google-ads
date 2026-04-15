use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

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
/// Sets the entity's status to REMOVED. This is a destructive operation
/// and requires double confirmation.
pub fn remove_entity(
    config: &Config,
    customer_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_entity", &config.safety)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let operation = build_status_operation(&cid, entity_type, entity_id, "REMOVED")?;

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
}
