use serde_json::json;

use crate::client::{GoogleAdsClient, MutateOperation};
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::audit;
use crate::safety::policy_exemption;
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
    /// Opt in to requesting policy exemptions. When the mutate is rejected with
    /// *exemptible* policy violations, resubmit once with
    /// `exemptPolicyViolationKeys` attached (what the Google Ads UI does
    /// automatically). Off by default: an exemption request asserts the ad is
    /// eligible under the policy's permitted use, which is the caller's call to
    /// make, not ours. Default: `false`.
    pub exempt_policy_violations: bool,
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
        exempt_policy_violations,
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
    apply_plan(&client, config, &plan, &plan_id, exempt_policy_violations).await
}

/// Dispatch the plan to the correct Google Ads RPC and shape the response.
async fn apply_plan(
    client: &GoogleAdsClient,
    config: &Config,
    plan: &ChangePlan,
    plan_id: &str,
    exempt_policy_violations: bool,
) -> Result<serde_json::Value> {
    let log_file = config.safety.log_file.to_string_lossy().to_string();

    let dispatch_result = match &plan.dispatch {
        PlanDispatch::MutateOperations => {
            apply_mutate_operations(client, plan, exempt_policy_violations).await
        }
        PlanDispatch::ApplyRecommendation {
            resource_names,
            apply_parameters,
        } => apply_recommendation_dispatch(client, plan, resource_names, apply_parameters).await,
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

fn to_mutate_operations(ops: &[serde_json::Value]) -> Vec<MutateOperation> {
    ops.iter()
        .map(|op| MutateOperation {
            operation: op.clone(),
        })
        .collect()
}

/// Shape a successful mutate response, failing loudly on partial failure.
///
/// Mutates are sent with `partialFailure: false` (atomic — a failing operation
/// aborts the whole request as an HTTP error), so `partial_failure_error`
/// should never be set. Kept as a safety net: if it ever appears, report
/// failure instead of "APPLIED" so the audit log can't record a false SUCCESS.
fn finish_mutate(
    response: crate::client::MutateResponse,
    exempted: Option<&[policy_exemption::PolicyViolation]>,
) -> Result<serde_json::Value> {
    if let Some(partial_error) = response.partial_failure_error {
        return Err(McpGoogleAdsError::PartialFailure(partial_error));
    }

    let mut out = json!({
        "status": "APPLIED",
        "responses": response.mutate_operation_responses,
    });

    // Record exemptions on the response: the caller asked for them, but the
    // fact that Google accepted an ad only under an exemption is material.
    if let Some(exempted) = exempted.filter(|e| !e.is_empty()) {
        out["policy_exemptions_requested"] = policy_exemption::violations_json(exempted, false);
        out["policy_exemption_note"] = json!(
            "These operations were accepted with a policy exemption request. \
             Google still reviews them; check approval status before relying on delivery."
        );
    }

    Ok(out)
}

async fn apply_mutate_operations(
    client: &GoogleAdsClient,
    plan: &ChangePlan,
    exempt_policy_violations: bool,
) -> Result<serde_json::Value> {
    let first_error = match client
        .mutate(
            &plan.customer_id,
            to_mutate_operations(&plan.mutate_operations),
        )
        .await
    {
        Ok(response) => return finish_mutate(response, None),
        Err(e) => e,
    };

    // Only a structured Google Ads failure can carry policy violations.
    let McpGoogleAdsError::GoogleAds { ref details, .. } = first_error else {
        return Err(first_error);
    };

    let violations = policy_exemption::parse_violations(details);
    let exemptible: Vec<_> = violations
        .iter()
        .filter(|v| v.is_exemptible)
        .cloned()
        .collect();

    if exemptible.is_empty() {
        // Nothing exemptible — surface the original error, which now renders
        // the underlying GoogleAdsFailure detail rather than just "400".
        return Err(first_error);
    }

    if !exempt_policy_violations {
        return Err(McpGoogleAdsError::PolicyExemption {
            message: format!(
                "Blocked by Google ad policy: {} exemptible policy violation(s). \
                 These are the same violations the Google Ads UI auto-exempts. \
                 To request an exemption, call confirm_and_apply again with the same \
                 plan_id and exempt_policy_violations=true. The plan is preserved.",
                exemptible.len()
            ),
            violations: policy_exemption::violations_json(&exemptible, false),
        });
    }

    let exemption = policy_exemption::apply_exemptions(&plan.mutate_operations, &exemptible);
    if exemption.exempted.is_empty() {
        return Err(McpGoogleAdsError::PolicyExemption {
            message: format!(
                "Blocked by Google ad policy: {} violation(s) that cannot be exempted \
                 through the API. Only ad group criteria (keywords) accept \
                 exemptPolicyViolationKeys; responsive search ads do not. \
                 Revise the offending text instead.",
                exemption.unsupported.len()
            ),
            violations: policy_exemption::violations_json(&exemption.unsupported, false),
        });
    }

    let response = client
        .mutate(
            &plan.customer_id,
            to_mutate_operations(&exemption.operations),
        )
        .await?;
    finish_mutate(response, Some(&exemption.exempted))
}

async fn apply_recommendation_dispatch(
    client: &GoogleAdsClient,
    plan: &ChangePlan,
    resource_names: &[String],
    apply_parameters: &Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let response = client
        .apply_recommendations(
            &plan.customer_id,
            resource_names.to_vec(),
            apply_parameters.clone(),
        )
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
                ..Default::default()
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
                ..Default::default()
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
