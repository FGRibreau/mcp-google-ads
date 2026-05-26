//! Actionable hint embedded in tool responses to tell an agent what to do next.
//!
//! When a tool creates an entity in `PAUSED` status by default, the response
//! includes a [`NextActionHint`] describing exactly how to enable it via the
//! MCP — no UI action required.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Hint pointing an agent at the next MCP tool to call to continue a workflow.
///
/// Surfaced in responses for operations whose default behaviour leaves an
/// entity in a non-final state (e.g. a freshly drafted ad in `PAUSED`).
/// The agent can read `tool` + `params` and immediately call the named MCP
/// tool — zero manual UI step required.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct NextActionHint {
    /// Name of the MCP tool to invoke (e.g. `"enable_entity"`).
    pub tool: String,
    /// JSON object with the exact parameters to pass to `tool`.
    pub params: serde_json::Value,
    /// Human-readable description of what calling `tool(params)` will do.
    pub description: String,
}

impl NextActionHint {
    /// Build a hint that tells the agent how to enable a newly created
    /// ad via the `enable_entity` MCP tool.
    ///
    /// Used when an ad is drafted in default `PAUSED` status — the agent
    /// can read the response, then call `enable_entity` with the resolved
    /// composite id to transition the ad to `ENABLED`.
    pub fn enable_ad(ad_group_id: &str, ad_id: &str) -> Self {
        let composite_id = format!("{}~{}", ad_group_id, ad_id);
        Self {
            tool: "enable_entity".to_string(),
            params: serde_json::json!({
                "entity_type": "ad",
                "entity_id": composite_id,
            }),
            description: format!(
                "Ad was created in PAUSED status. Call enable_entity with entity_type='ad' and entity_id='{}' to transition it to ENABLED via MCP — no UI action required.",
                composite_id
            ),
        }
    }

    /// Build a hint that tells the agent how to enable a newly created
    /// campaign via the `enable_entity` MCP tool.
    pub fn enable_campaign(campaign_id: &str) -> Self {
        Self {
            tool: "enable_entity".to_string(),
            params: serde_json::json!({
                "entity_type": "campaign",
                "entity_id": campaign_id,
            }),
            description: format!(
                "Campaign was created in PAUSED status. Call enable_entity with entity_type='campaign' and entity_id='{}' to start serving — no UI action required.",
                campaign_id
            ),
        }
    }

    /// Build a hint that tells the agent how to enable a newly created
    /// ad group via the `enable_entity` MCP tool.
    pub fn enable_ad_group(ad_group_id: &str) -> Self {
        Self {
            tool: "enable_entity".to_string(),
            params: serde_json::json!({
                "entity_type": "ad_group",
                "entity_id": ad_group_id,
            }),
            description: format!(
                "Ad group was created in PAUSED status. Call enable_entity with entity_type='ad_group' and entity_id='{}' to activate it — no UI action required.",
                ad_group_id
            ),
        }
    }

    /// Generic hint for a newly created entity awaiting an explicit enable.
    ///
    /// Used for entity types that don't have a more specific helper
    /// (e.g. asset groups in PMax campaigns where the agent should enable
    /// the parent campaign).
    pub fn enable_parent_campaign_after_create(campaign_temp_ref: &str) -> Self {
        Self {
            tool: "enable_entity".to_string(),
            params: serde_json::json!({
                "entity_type": "campaign",
                "entity_id": "<resolve campaign_id from confirm_and_apply response>",
            }),
            description: format!(
                "The campaign was drafted in PAUSED status (temp ref: {}). After confirm_and_apply, read the returned campaign id from responses[1] and call enable_entity to transition it to ENABLED.",
                campaign_temp_ref
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_ad_hint_shape() {
        let h = NextActionHint::enable_ad("111", "999");
        assert_eq!(h.tool, "enable_entity");
        assert_eq!(h.params["entity_type"], "ad");
        assert_eq!(h.params["entity_id"], "111~999");
        assert!(h.description.contains("enable_entity"));
        assert!(h.description.contains("111~999"));
    }

    #[test]
    fn enable_campaign_hint_shape() {
        let h = NextActionHint::enable_campaign("CAMP1");
        assert_eq!(h.params["entity_type"], "campaign");
        assert_eq!(h.params["entity_id"], "CAMP1");
    }

    #[test]
    fn enable_ad_group_hint_shape() {
        let h = NextActionHint::enable_ad_group("AG1");
        assert_eq!(h.params["entity_type"], "ad_group");
        assert_eq!(h.params["entity_id"], "AG1");
    }
}
