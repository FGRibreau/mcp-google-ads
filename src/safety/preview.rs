use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePlan {
    pub plan_id: String,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: String,
    pub customer_id: String,
    pub changes: serde_json::Value,
    pub created_at: String,
    pub requires_double_confirm: bool,
    /// The actual mutate operations to execute (serialized for the API)
    pub mutate_operations: Vec<serde_json::Value>,
}

impl ChangePlan {
    pub fn new(
        operation: String,
        entity_type: String,
        entity_id: String,
        customer_id: String,
        changes: serde_json::Value,
        requires_double_confirm: bool,
        mutate_operations: Vec<serde_json::Value>,
    ) -> Self {
        Self {
            plan_id: Uuid::new_v4().to_string()[..8].to_string(),
            operation,
            entity_type,
            entity_id,
            customer_id,
            changes,
            created_at: Utc::now().to_rfc3339(),
            requires_double_confirm,
            mutate_operations,
        }
    }

    pub fn to_preview(&self) -> serde_json::Value {
        serde_json::json!({
            "plan_id": self.plan_id,
            "operation": self.operation,
            "entity_type": self.entity_type,
            "entity_id": self.entity_id,
            "customer_id": self.customer_id,
            "changes": self.changes,
            "requires_double_confirm": self.requires_double_confirm,
            "status": "PENDING_CONFIRMATION",
            "instructions": format!(
                "Review the changes above. To apply, call confirm_and_apply with plan_id='{}' and dry_run=false.",
                self.plan_id
            ),
        })
    }
}

/// Thread-safe store for pending plans
static PENDING_PLANS: std::sync::LazyLock<Mutex<HashMap<String, ChangePlan>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn store_plan(plan: ChangePlan) {
    PENDING_PLANS
        .lock()
        .expect("plan store lock poisoned")
        .insert(plan.plan_id.clone(), plan);
}

pub fn get_plan(plan_id: &str) -> Option<ChangePlan> {
    PENDING_PLANS
        .lock()
        .expect("plan store lock poisoned")
        .get(plan_id)
        .cloned()
}

pub fn remove_plan(plan_id: &str) {
    PENDING_PLANS
        .lock()
        .expect("plan store lock poisoned")
        .remove(plan_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan() -> ChangePlan {
        ChangePlan::new(
            "test_operation".to_string(),
            "campaign".to_string(),
            "entity-123".to_string(),
            "1234567890".to_string(),
            serde_json::json!({"key": "value"}),
            false,
            vec![serde_json::json!({"campaignOperation": {"create": {}}})],
        )
    }

    #[test]
    fn test_change_plan_creation() {
        let plan = make_plan();
        assert!(!plan.plan_id.is_empty());
        assert!(!plan.created_at.is_empty());
        assert_eq!(plan.operation, "test_operation");
        assert_eq!(plan.entity_type, "campaign");
        assert_eq!(plan.entity_id, "entity-123");
        assert_eq!(plan.customer_id, "1234567890");
        assert!(!plan.requires_double_confirm);
        assert_eq!(plan.mutate_operations.len(), 1);
    }

    #[test]
    fn test_store_and_retrieve_plan() {
        let plan = make_plan();
        let plan_id = plan.plan_id.clone();
        store_plan(plan);

        let retrieved = get_plan(&plan_id);
        assert!(retrieved.is_some());
        let retrieved = retrieved.map(|p| p.operation).unwrap_or_default();
        assert_eq!(retrieved, "test_operation");

        // Cleanup
        remove_plan(&plan_id);
    }

    #[test]
    fn test_remove_plan() {
        let plan = make_plan();
        let plan_id = plan.plan_id.clone();
        store_plan(plan);

        remove_plan(&plan_id);
        assert!(get_plan(&plan_id).is_none());
    }

    #[test]
    fn test_get_nonexistent_plan() {
        assert!(get_plan("does-not-exist-xyz").is_none());
    }

    #[test]
    fn test_to_preview_format() {
        let plan = make_plan();
        let preview = plan.to_preview();

        assert!(preview.get("plan_id").is_some());
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        let instructions = preview["instructions"].as_str().unwrap_or_default();
        assert!(instructions.contains(&plan.plan_id));
        assert!(instructions.contains("confirm_and_apply"));
    }
}
