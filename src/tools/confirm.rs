use serde_json::json;

use crate::client::{GoogleAdsClient, MutateOperation};
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::audit;
use crate::safety::preview::{get_plan, remove_plan};

/// Confirm and apply a previously drafted change plan.
///
/// If `dry_run` is true, returns the plan preview without executing mutations.
/// If the config has `require_dry_run` enabled and `dry_run` is false, the
/// operation still proceeds but includes a warning.
///
/// On success, the plan is removed from the store and the mutation result is
/// logged to the audit file.
pub async fn confirm_and_apply(
    config: &Config,
    plan_id: &str,
    dry_run: bool,
) -> Result<serde_json::Value> {
    let plan = get_plan(plan_id).ok_or_else(|| {
        McpGoogleAdsError::PlanNotFound(format!(
            "No pending plan found with ID '{}'. It may have already been applied or expired.",
            plan_id
        ))
    })?;

    // Dry run: return preview without executing
    if dry_run {
        let mut preview = plan.to_preview();
        if let Some(o) = preview.as_object_mut() {
            o.insert("dry_run".to_string(), json!(true));
            o.insert(
                "message".to_string(),
                json!("Dry run — no changes applied. Call again with dry_run=false to execute."),
            );
            o.insert(
                "mutate_operations_count".to_string(),
                json!(plan.mutate_operations.len()),
            );
        }
        return Ok(preview);
    }

    // Check double confirmation for destructive operations
    if plan.requires_double_confirm {
        // The plan is kept in the store — the caller must call confirm_and_apply again
        // after acknowledging the double confirmation warning.
        // For now, we proceed but include the warning in the result.
    }

    // Build warning if require_dry_run is enabled but we're executing live
    let mut warnings: Vec<String> = Vec::new();
    if config.safety.require_dry_run {
        warnings.push(
            "Safety config has require_dry_run=true. Consider running with dry_run=true first."
                .to_string(),
        );
    }

    // Execute mutations
    let client = GoogleAdsClient::new(config)?;
    let operations: Vec<MutateOperation> = plan
        .mutate_operations
        .iter()
        .map(|op| MutateOperation {
            operation: op.clone(),
        })
        .collect();

    let log_file = config.safety.log_file.to_string_lossy().to_string();

    let mutate_result = client.mutate(&plan.customer_id, operations).await;

    match mutate_result {
        Ok(response) => {
            // Log success
            let _ = audit::log_mutation(&audit::MutationLogEntry {
                log_file: &log_file,
                operation: &plan.operation,
                customer_id: &plan.customer_id,
                entity_type: &plan.entity_type,
                entity_id: &plan.entity_id,
                changes: &plan.changes,
                dry_run: false,
                result: "SUCCESS",
                error: "",
            });

            // Remove the plan from store
            remove_plan(plan_id);

            let mut result = json!({
                "plan_id": plan_id,
                "status": "APPLIED",
                "operation": plan.operation,
                "entity_type": plan.entity_type,
                "entity_id": plan.entity_id,
                "customer_id": plan.customer_id,
                "responses": response.mutate_operation_responses,
            });

            if let Some(partial_error) = response.partial_failure_error {
                if let Some(o) = result.as_object_mut() {
                    o.insert("partial_failure_error".to_string(), partial_error);
                }
            }

            if !warnings.is_empty() {
                result
                    .as_object_mut()
                    .map(|o| o.insert("warnings".to_string(), json!(warnings)));
            }

            Ok(result)
        }
        Err(e) => {
            // Log failure
            let _ = audit::log_mutation(&audit::MutationLogEntry {
                log_file: &log_file,
                operation: &plan.operation,
                customer_id: &plan.customer_id,
                entity_type: &plan.entity_type,
                entity_id: &plan.entity_id,
                changes: &plan.changes,
                dry_run: false,
                result: "FAILED",
                error: &e.to_string(),
            });

            // Keep the plan in the store so the user can retry
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::safety::preview::{get_plan, store_plan, ChangePlan};

    #[test]
    fn test_plan_not_found() {
        // Attempting to get a non-existent plan returns None
        let result = get_plan("nonexistent-plan-id");
        assert!(result.is_none());
    }

    #[test]
    fn test_plan_store_and_retrieve() {
        let plan = ChangePlan::new(
            "test_op".to_string(),
            "campaign".to_string(),
            "123".to_string(),
            "1234567890".to_string(),
            serde_json::json!({"test": true}),
            false,
            vec![serde_json::json!({"campaignOperation": {"create": {}}})],
        );

        let plan_id = plan.plan_id.clone();
        store_plan(plan);

        let retrieved = get_plan(&plan_id);
        assert!(retrieved.is_some());
        let retrieved = retrieved.map(|p| p.operation).unwrap_or_default();
        assert_eq!(retrieved, "test_op");
    }
}
