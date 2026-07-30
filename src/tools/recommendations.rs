use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::Result;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan, PlanDispatch};

/// List active (non-dismissed) recommendations for a customer account.
///
/// This is a READ tool that returns recommendations directly.
/// Uses GAQL to query the recommendation resource.
pub async fn list_recommendations(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "SELECT \
        recommendation.type, \
        recommendation.impact, \
        recommendation.campaign, \
        recommendation.resource_name \
        FROM recommendation \
        WHERE recommendation.dismissed = FALSE \
        LIMIT 50";

    let rows = client.search(customer_id, query).await?;

    let result = json!({
        "recommendations": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Apply a recommendation.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
///
/// The plan carries [`PlanDispatch::ApplyRecommendation`] so the apply step
/// routes to the dedicated `recommendations:apply` RPC — NOT to
/// `googleAds:mutate`, which does not accept `applyRecommendationOperation`
/// as a valid `MutateOperation` key in v25.
pub fn apply_recommendation(
    config: &Config,
    customer_id: &str,
    recommendation_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("apply_recommendation", &config.safety)?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let resource_name = format!("customers/{}/recommendations/{}", cid, recommendation_id);

    let changes = json!({
        "recommendation_id": recommendation_id,
        "action": "APPLY",
        "resource_name": resource_name,
    });

    let plan = ChangePlan::new(
        "apply_recommendation".to_string(),
        "recommendation".to_string(),
        recommendation_id.to_string(),
        cid,
        changes,
        false,
        Vec::new(),
    )
    .with_dispatch(PlanDispatch::ApplyRecommendation {
        resource_names: vec![resource_name],
    });

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Dismiss a recommendation.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
///
/// The plan carries [`PlanDispatch::DismissRecommendation`] so the apply step
/// routes to the dedicated `recommendations:dismiss` RPC. v25 has no
/// `dismissRecommendationOperation` key on `MutateOperation` — routing it
/// through `googleAds:mutate` returns 400.
pub fn dismiss_recommendation(
    config: &Config,
    customer_id: &str,
    recommendation_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("dismiss_recommendation", &config.safety)?;

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);
    let resource_name = format!("customers/{}/recommendations/{}", cid, recommendation_id);

    let changes = json!({
        "recommendation_id": recommendation_id,
        "action": "DISMISS",
        "resource_name": resource_name,
    });

    let plan = ChangePlan::new(
        "dismiss_recommendation".to_string(),
        "recommendation".to_string(),
        recommendation_id.to_string(),
        cid,
        changes,
        false,
        Vec::new(),
    )
    .with_dispatch(PlanDispatch::DismissRecommendation {
        resource_names: vec![resource_name],
    });

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::safety::preview::get_plan;

    #[test]
    fn test_apply_recommendation_success() {
        let config = Config::default();
        let result = apply_recommendation(&config, "123-456-7890", "rec-123");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "apply_recommendation");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
    }

    #[test]
    fn test_apply_recommendation_uses_dedicated_dispatch() {
        let config = Config::default();
        let preview = apply_recommendation(&config, "123-456-7890", "rec-d1").unwrap();
        let plan_id = preview["plan_id"].as_str().unwrap();
        let plan = get_plan(plan_id).unwrap();
        match plan.dispatch {
            PlanDispatch::ApplyRecommendation { resource_names } => {
                assert_eq!(resource_names.len(), 1);
                assert!(resource_names[0].ends_with("/recommendations/rec-d1"));
            }
            other => panic!("expected ApplyRecommendation dispatch, got {:?}", other),
        }
        // The mutate_operations list MUST be empty so a bug doesn't accidentally
        // route a recommendation op through googleAds:mutate.
        assert!(plan.mutate_operations.is_empty());
    }

    #[test]
    fn test_apply_recommendation_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["apply_recommendation".to_string()];
        let result = apply_recommendation(&config, "123-456-7890", "rec-123");
        assert!(result.is_err());
    }

    #[test]
    fn test_dismiss_recommendation_success() {
        let config = Config::default();
        let result = dismiss_recommendation(&config, "123-456-7890", "rec-456");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "dismiss_recommendation");
    }

    #[test]
    fn test_dismiss_recommendation_uses_dedicated_dispatch() {
        let config = Config::default();
        let preview = dismiss_recommendation(&config, "123-456-7890", "rec-d2").unwrap();
        let plan_id = preview["plan_id"].as_str().unwrap();
        let plan = get_plan(plan_id).unwrap();
        match plan.dispatch {
            PlanDispatch::DismissRecommendation { resource_names } => {
                assert_eq!(resource_names.len(), 1);
                assert!(resource_names[0].ends_with("/recommendations/rec-d2"));
            }
            other => panic!("expected DismissRecommendation dispatch, got {:?}", other),
        }
        assert!(plan.mutate_operations.is_empty());
    }

    #[test]
    fn test_dismiss_recommendation_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["dismiss_recommendation".to_string()];
        let result = dismiss_recommendation(&config, "123-456-7890", "rec-456");
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_recommendation_entity_id() {
        let config = Config::default();
        let result = apply_recommendation(&config, "123-456-7890", "rec-789");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["entity_id"], "rec-789");
    }

    #[test]
    fn test_dismiss_recommendation_changes() {
        let config = Config::default();
        let result = dismiss_recommendation(&config, "123-456-7890", "rec-xyz");
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["changes"]["action"], "DISMISS");
    }
}
