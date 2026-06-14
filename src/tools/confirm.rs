use serde_json::json;

use crate::client::{GoogleAdsClient, MutateOperation};
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::audit;
use crate::safety::preview::{get_plan, remove_plan, ChangePlan, PlanDispatch};

/// Parameters carried through `confirm_and_apply` callers down to the apply
/// implementation. Centralised so the hard guards (`require_dry_run`,
/// `requires_double_confirm`) can be opt-out by the caller.
#[derive(Debug, Clone, Default)]
pub struct ConfirmApplyInput {
    pub plan_id: String,
    pub dry_run: bool,
    /// Override for `config.safety.require_dry_run`. When `true`, the dry-run
    /// guard is bypassed for THIS single apply (one-shot escape hatch — does
    /// not modify config). Default: `false`.
    pub bypass_require_dry_run: bool,
    /// Acknowledgement that the caller has read and intends to apply a plan
    /// flagged `requires_double_confirm`. Without this, destructive plans
    /// return `Err(DoubleConfirmRequired)` instead of executing.
    pub confirmed_twice: bool,
}

/// Confirm and apply a previously drafted change plan.
///
/// Dispatch routing:
/// - [`PlanDispatch::MutateOperations`] -> `POST /googleAds:mutate`
/// - [`PlanDispatch::ApplyRecommendation`] -> `POST /recommendations:apply`
/// - [`PlanDispatch::DismissRecommendation`] -> `POST /recommendations:dismiss`
///
/// Hard guards (return `Err` BEFORE any HTTP traffic):
/// - `config.safety.require_dry_run && !dry_run && !bypass_require_dry_run`
///   -> [`McpGoogleAdsError::DryRunRequired`]
/// - `plan.requires_double_confirm && !confirmed_twice && !dry_run`
///   -> [`McpGoogleAdsError::DoubleConfirmRequired`]
///
/// On success the plan is removed from the store and the mutation result
/// is logged to the audit file. On failure the plan is kept so the caller
/// can retry. Successful responses NEVER include a warning — the warning
/// emitted by v0.2.x was removed (it was cosmetic and lied about safety).
pub async fn confirm_and_apply(
    config: &Config,
    input: ConfirmApplyInput,
) -> Result<serde_json::Value> {
    let ConfirmApplyInput {
        plan_id,
        dry_run,
        bypass_require_dry_run,
        confirmed_twice,
    } = input;

    let plan = get_plan(&plan_id).ok_or_else(|| {
        McpGoogleAdsError::PlanNotFound(format!(
            "No pending plan found with ID '{}'. It may have already been applied or expired.",
            plan_id
        ))
    })?;

    // Dry run: return preview without executing.
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

    // Hard guard: dry-run requirement.
    if config.safety.require_dry_run && !bypass_require_dry_run {
        return Err(McpGoogleAdsError::DryRunRequired);
    }

    // Hard guard: double-confirmation for destructive operations.
    if plan.requires_double_confirm && !confirmed_twice {
        return Err(McpGoogleAdsError::DoubleConfirmRequired);
    }

    let client = GoogleAdsClient::new(config)?;
    apply_plan(&client, config, &plan, &plan_id).await
}

/// Dispatch the plan to the correct Google Ads RPC and shape the response.
async fn apply_plan(
    client: &GoogleAdsClient,
    config: &Config,
    plan: &ChangePlan,
    plan_id: &str,
) -> Result<serde_json::Value> {
    let log_file = config.safety.log_file.to_string_lossy().to_string();

    let dispatch_result = match &plan.dispatch {
        PlanDispatch::MutateOperations => apply_mutate_operations(client, plan).await,
        PlanDispatch::ApplyRecommendation { resource_names } => {
            apply_recommendation_dispatch(client, plan, resource_names).await
        }
        PlanDispatch::DismissRecommendation { resource_names } => {
            dismiss_recommendation_dispatch(client, plan, resource_names).await
        }
    };

    match dispatch_result {
        Ok(mut result) => {
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

            if let Some(obj) = result.as_object_mut() {
                obj.insert("plan_id".to_string(), json!(plan_id));
                obj.insert("operation".to_string(), json!(plan.operation));
                obj.insert("entity_type".to_string(), json!(plan.entity_type));
                obj.insert("entity_id".to_string(), json!(plan.entity_id));
                obj.insert("customer_id".to_string(), json!(plan.customer_id));
                if let Some(status) = plan.status_after_apply {
                    obj.insert("status_after_apply".to_string(), json!(status.as_api_str()));
                }
                if let Some(ref hint) = plan.next_action_hint {
                    obj.insert(
                        "next_action_hint".to_string(),
                        serde_json::to_value(hint).unwrap_or(serde_json::Value::Null),
                    );
                }
            }

            remove_plan(plan_id);
            Ok(result)
        }
        Err(e) => {
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
            // Keep the plan in the store so the user can retry.
            Err(e)
        }
    }
}

async fn apply_mutate_operations(
    client: &GoogleAdsClient,
    plan: &ChangePlan,
) -> Result<serde_json::Value> {
    let operations: Vec<MutateOperation> = plan
        .mutate_operations
        .iter()
        .map(|op| MutateOperation {
            operation: op.clone(),
        })
        .collect();

    let response = client.mutate(&plan.customer_id, operations).await?;

    // Google returns HTTP 200 with `partialFailureError` in the body when one or
    // more operations fail (we request `partialFailure: true`). Treat its presence
    // as a real failure instead of reporting "APPLIED" — otherwise the failure is
    // invisible to the caller and the audit log records a false SUCCESS.
    if let Some(partial_error) = response.partial_failure_error {
        return Err(McpGoogleAdsError::PartialFailure(partial_error));
    }

    Ok(json!({
        "status": "APPLIED",
        "responses": response.mutate_operation_responses,
    }))
}

async fn apply_recommendation_dispatch(
    client: &GoogleAdsClient,
    plan: &ChangePlan,
    resource_names: &[String],
) -> Result<serde_json::Value> {
    let response = client
        .apply_recommendations(&plan.customer_id, resource_names.to_vec())
        .await?;

    if let Some(partial_error) = response.partial_failure_error {
        return Err(McpGoogleAdsError::PartialFailure(partial_error));
    }

    Ok(json!({
        "status": "APPLIED",
        "results": response.results,
    }))
}

async fn dismiss_recommendation_dispatch(
    client: &GoogleAdsClient,
    plan: &ChangePlan,
    resource_names: &[String],
) -> Result<serde_json::Value> {
    let response = client
        .dismiss_recommendations(&plan.customer_id, resource_names.to_vec())
        .await?;

    Ok(json!({
        "status": "DISMISSED",
        "results": response.results,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::preview::{get_plan, store_plan, ChangePlan};

    #[test]
    fn test_plan_not_found_via_get() {
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

    #[tokio::test]
    async fn test_require_dry_run_hard_guards_apply() {
        // Plan exists, require_dry_run=true (default), dry_run=false, no bypass.
        // Expected: Err(DryRunRequired) BEFORE any HTTP call.
        let plan = ChangePlan::new(
            "test_op".to_string(),
            "campaign".to_string(),
            "1".to_string(),
            "1234567890".to_string(),
            serde_json::json!({}),
            false,
            vec![serde_json::json!({"campaignOperation": {"update": {"resourceName": "x"}}})],
        );
        let plan_id = plan.plan_id.clone();
        store_plan(plan);

        let mut config = Config::default();
        config.safety.require_dry_run = true;

        let err = confirm_and_apply(
            &config,
            ConfirmApplyInput {
                plan_id: plan_id.clone(),
                dry_run: false,
                bypass_require_dry_run: false,
                confirmed_twice: false,
            },
        )
        .await
        .expect_err("expected DryRunRequired");

        assert!(matches!(err, McpGoogleAdsError::DryRunRequired));
        // Plan is preserved so the caller can retry.
        assert!(get_plan(&plan_id).is_some());
        remove_plan(&plan_id);
    }

    #[tokio::test]
    async fn test_dry_run_returns_preview_without_http() {
        let plan = ChangePlan::new(
            "test_op".to_string(),
            "campaign".to_string(),
            "1".to_string(),
            "1234567890".to_string(),
            serde_json::json!({}),
            false,
            vec![serde_json::json!({"campaignOperation": {"update": {"resourceName": "x"}}})],
        );
        let plan_id = plan.plan_id.clone();
        store_plan(plan);

        let config = Config::default();
        let preview = confirm_and_apply(
            &config,
            ConfirmApplyInput {
                plan_id: plan_id.clone(),
                dry_run: true,
                bypass_require_dry_run: false,
                confirmed_twice: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(preview["dry_run"], true);
        // Plan is preserved across dry runs.
        assert!(get_plan(&plan_id).is_some());
        remove_plan(&plan_id);
    }
}
