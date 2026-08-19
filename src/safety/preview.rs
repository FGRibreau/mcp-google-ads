use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{AdStatus, NextActionHint};

/// Discriminator that tells `confirm_and_apply` how to dispatch this plan.
///
/// - `MutateOperations` (default) -> POST `/v25/customers/{cid}/googleAds:mutate`
/// - `ApplyRecommendation` -> POST `/v25/customers/{cid}/recommendations:apply`
/// - `DismissRecommendation` -> POST `/v25/customers/{cid}/recommendations:dismiss`
///
/// The recommendation variants exist because `applyRecommendationOperation` and
/// `dismissRecommendationOperation` are NOT valid `MutateOperation` keys in
/// Google Ads v25 — they live on dedicated RPCs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanDispatch {
    #[default]
    MutateOperations,
    ApplyRecommendation {
        /// Full resource name(s): `customers/{cid}/recommendations/{rec_id}`.
        resource_names: Vec<String>,
        /// The `apply_parameters` oneof, for the recommendation types that
        /// cannot be applied from a bare resource name. Merged onto every
        /// operation in the batch at dispatch time.
        #[serde(default)]
        apply_parameters: Option<serde_json::Value>,
    },
    DismissRecommendation {
        /// Full resource name(s): `customers/{cid}/recommendations/{rec_id}`.
        resource_names: Vec<String>,
    },
}

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
    /// The actual mutate operations to execute (serialized for the API).
    ///
    /// Empty for `dispatch != MutateOperations` (recommendation plans route
    /// through dedicated RPCs and do not use this field).
    pub mutate_operations: Vec<serde_json::Value>,
    /// Dispatch route for [`crate::tools::confirm::confirm_and_apply`].
    #[serde(default)]
    pub dispatch: PlanDispatch,
    /// The lifecycle status the entity will hold immediately after a
    /// successful apply. Surfaced in the response so agents know whether
    /// they still need to call `enable_entity`.
    #[serde(default)]
    pub status_after_apply: Option<AdStatus>,
    /// Optional MCP next-step hint propagated into the apply response.
    /// Tells the agent how to continue the workflow with zero UI action.
    #[serde(default)]
    pub next_action_hint: Option<NextActionHint>,
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
            dispatch: PlanDispatch::MutateOperations,
            status_after_apply: None,
            next_action_hint: None,
        }
    }

    /// Attach a `status_after_apply` value to the plan (builder-style).
    pub fn with_status_after_apply(mut self, status: AdStatus) -> Self {
        self.status_after_apply = Some(status);
        self
    }

    /// Attach a [`NextActionHint`] to the plan (builder-style).
    pub fn with_next_action_hint(mut self, hint: NextActionHint) -> Self {
        self.next_action_hint = Some(hint);
        self
    }

    /// Override the default `MutateOperations` dispatch with a dedicated
    /// recommendation RPC route.
    pub fn with_dispatch(mut self, dispatch: PlanDispatch) -> Self {
        self.dispatch = dispatch;
        self
    }

    pub fn to_preview(&self) -> serde_json::Value {
        let mut preview = serde_json::json!({
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
        });

        if let Some(obj) = preview.as_object_mut() {
            if let Some(status) = self.status_after_apply {
                obj.insert(
                    "status_after_apply".to_string(),
                    serde_json::Value::String(status.as_api_str().to_string()),
                );
            }
            if let Some(ref hint) = self.next_action_hint {
                obj.insert(
                    "next_action_hint".to_string(),
                    serde_json::to_value(hint).unwrap_or(serde_json::Value::Null),
                );
            }
        }

        preview
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
        assert_eq!(plan.dispatch, PlanDispatch::MutateOperations);
        assert!(plan.status_after_apply.is_none());
        assert!(plan.next_action_hint.is_none());
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

    #[test]
    fn test_to_preview_with_status_and_hint() {
        let plan = make_plan()
            .with_status_after_apply(AdStatus::Paused)
            .with_next_action_hint(NextActionHint::enable_ad("AG1", "AD1"));
        let preview = plan.to_preview();
        assert_eq!(preview["status_after_apply"], "PAUSED");
        assert_eq!(preview["next_action_hint"]["tool"], "enable_entity");
        assert_eq!(
            preview["next_action_hint"]["params"]["entity_id"],
            "AG1~AD1"
        );
    }

    #[test]
    fn test_dispatch_default_and_override() {
        let plan = make_plan();
        assert_eq!(plan.dispatch, PlanDispatch::MutateOperations);

        let plan = make_plan().with_dispatch(PlanDispatch::DismissRecommendation {
            resource_names: vec!["customers/123/recommendations/rec-1".to_string()],
        });
        match plan.dispatch {
            PlanDispatch::DismissRecommendation { resource_names } => {
                assert_eq!(resource_names.len(), 1);
            }
            _ => panic!("expected DismissRecommendation"),
        }
    }
}
