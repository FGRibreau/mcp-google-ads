//! Write tools for negative keyword lists (Google Ads "shared sets").
//!
//! Three resources are involved, and every tool here is a thin wrapper over
//! one or more of them:
//!
//! - `sharedSetOperation` — the list itself (`SharedSet`, type `NEGATIVE_KEYWORDS`)
//! - `sharedCriterionOperation` — a keyword inside the list (`SharedCriterion`)
//! - `campaignSharedSetOperation` — the list↔campaign link (`CampaignSharedSet`)
//!
//! `create_negative_keyword_list` emits all three in a single atomic mutate
//! using the temporary resource ID `-1` for the not-yet-created set, the same
//! technique `draft_campaign` uses for its budget→campaign→ad group chain.
//! Atomicity matters here: a list created without its keywords, or keywords
//! created without their campaign links, is a silent gap in coverage.

use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};
use crate::tools::shared_sets::validate_numeric_id;

const VALID_MATCH_TYPES: &[&str] = &["EXACT", "PHRASE", "BROAD"];

/// Google Ads caps a negative keyword list at 5,000 members. Rejecting here
/// gives a clear error instead of a partial-looking API failure.
const MAX_SHARED_SET_KEYWORDS: usize = 5_000;

fn validate_match_type(match_type: &str) -> Result<()> {
    if !VALID_MATCH_TYPES.contains(&match_type) {
        return Err(McpGoogleAdsError::Validation(format!(
            "Invalid match type '{}'. Must be one of: {}",
            match_type,
            VALID_MATCH_TYPES.join(", ")
        )));
    }
    Ok(())
}

/// Drop case-insensitive duplicates, preserving first-seen order.
///
/// Google rejects a duplicate member outright, and because mutates here are
/// atomic (`partialFailure: false`) one repeated word would fail the entire
/// batch. The count of what was dropped is reported in the preview so this is
/// never a silent edit of the caller's input.
fn dedupe_keywords(keywords: Vec<String>) -> (Vec<String>, usize) {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::with_capacity(keywords.len());
    let mut dropped = 0;

    for kw in keywords {
        let trimmed = kw.trim().to_string();
        if trimmed.is_empty() {
            dropped += 1;
            continue;
        }
        if seen.insert(trimmed.to_lowercase()) {
            unique.push(trimmed);
        } else {
            dropped += 1;
        }
    }

    (unique, dropped)
}

fn validate_keyword_batch(keywords: &[String]) -> Result<()> {
    if keywords.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one keyword is required".to_string(),
        ));
    }
    if keywords.len() > MAX_SHARED_SET_KEYWORDS {
        return Err(McpGoogleAdsError::Validation(format!(
            "A negative keyword list holds at most {} keywords, got {}",
            MAX_SHARED_SET_KEYWORDS,
            keywords.len()
        )));
    }
    Ok(())
}

fn shared_criterion_op(
    shared_set_resource: &str,
    text: &str,
    match_type: &str,
) -> serde_json::Value {
    json!({
        "sharedCriterionOperation": {
            "create": {
                "sharedSet": shared_set_resource,
                "keyword": {
                    "text": text,
                    "matchType": match_type
                }
            }
        }
    })
}

/// Create a negative keyword list, optionally seeded with keywords and
/// attached to campaigns — all in one atomic mutate.
pub fn create_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    name: &str,
    keywords: Vec<String>,
    match_type: &str,
    campaign_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("create_negative_keyword_list", &config.safety)?;

    if name.trim().is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "List name is required".to_string(),
        ));
    }
    validate_match_type(match_type)?;
    for campaign_id in campaign_ids {
        validate_numeric_id("campaign_id", campaign_id)?;
    }

    let (keywords, duplicates_dropped) = dedupe_keywords(keywords);
    if keywords.len() > MAX_SHARED_SET_KEYWORDS {
        return Err(McpGoogleAdsError::Validation(format!(
            "A negative keyword list holds at most {} keywords, got {}",
            MAX_SHARED_SET_KEYWORDS,
            keywords.len()
        )));
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    // Temporary resource ID -1: the criteria and campaign links below reference
    // the set within the same request, before it has a real ID.
    let shared_set_resource = format!("customers/{}/sharedSets/-1", cid);

    let mut operations = vec![json!({
        "sharedSetOperation": {
            "create": {
                "name": name.trim(),
                "type": "NEGATIVE_KEYWORDS",
                "resourceName": shared_set_resource
            }
        }
    })];

    for kw in &keywords {
        operations.push(shared_criterion_op(&shared_set_resource, kw, match_type));
    }

    for campaign_id in campaign_ids {
        operations.push(json!({
            "campaignSharedSetOperation": {
                "create": {
                    "campaign": format!("customers/{}/campaigns/{}", cid, campaign_id),
                    "sharedSet": shared_set_resource
                }
            }
        }));
    }

    let changes = json!({
        "name": name.trim(),
        "type": "NEGATIVE_KEYWORDS",
        "keywords": keywords,
        "keyword_count": keywords.len(),
        "match_type": match_type,
        "attach_to_campaign_ids": campaign_ids,
        "duplicates_dropped": duplicates_dropped,
    });

    let plan = ChangePlan::new(
        "create_negative_keyword_list".to_string(),
        "shared_set".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Add keywords to an existing negative keyword list.
pub fn add_to_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    shared_set_id: &str,
    keywords: Vec<String>,
    match_type: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("add_to_negative_keyword_list", &config.safety)?;

    validate_numeric_id("shared_set_id", shared_set_id)?;
    validate_match_type(match_type)?;

    let (keywords, duplicates_dropped) = dedupe_keywords(keywords);
    validate_keyword_batch(&keywords)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let shared_set_resource = format!("customers/{}/sharedSets/{}", cid, shared_set_id);

    let operations: Vec<serde_json::Value> = keywords
        .iter()
        .map(|kw| shared_criterion_op(&shared_set_resource, kw, match_type))
        .collect();

    let changes = json!({
        "shared_set_id": shared_set_id,
        "keywords": keywords,
        "keyword_count": keywords.len(),
        "match_type": match_type,
        "duplicates_dropped": duplicates_dropped,
        "note": "Adding a keyword the list already holds fails the whole batch — check get_negative_keyword_list first.",
    });

    let plan = ChangePlan::new(
        "add_to_negative_keyword_list".to_string(),
        "shared_criterion".to_string(),
        shared_set_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Remove keywords from a negative keyword list by criterion ID.
///
/// Destructive: requires double confirmation.
pub fn remove_from_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    shared_set_id: &str,
    criterion_ids: Vec<String>,
) -> Result<serde_json::Value> {
    check_blocked_operation("remove_from_negative_keyword_list", &config.safety)?;

    validate_numeric_id("shared_set_id", shared_set_id)?;
    if criterion_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one criterion ID is required".to_string(),
        ));
    }
    for criterion_id in &criterion_ids {
        validate_numeric_id("criterion_id", criterion_id)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operations: Vec<serde_json::Value> = criterion_ids
        .iter()
        .map(|criterion_id| {
            json!({
                "sharedCriterionOperation": {
                    "remove": format!(
                        "customers/{}/sharedCriteria/{}~{}",
                        cid, shared_set_id, criterion_id
                    )
                }
            })
        })
        .collect();

    let changes = json!({
        "shared_set_id": shared_set_id,
        "criterion_ids": criterion_ids,
        "warning": "This action is destructive and cannot be undone. Every campaign using this list loses these exclusions.",
    });

    let plan = ChangePlan::new(
        "remove_from_negative_keyword_list".to_string(),
        "shared_criterion".to_string(),
        shared_set_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Attach a negative keyword list to one or more campaigns.
pub fn attach_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    shared_set_id: &str,
    campaign_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("attach_negative_keyword_list", &config.safety)?;

    validate_numeric_id("shared_set_id", shared_set_id)?;
    if campaign_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one campaign ID is required".to_string(),
        ));
    }
    for campaign_id in campaign_ids {
        validate_numeric_id("campaign_id", campaign_id)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let shared_set_resource = format!("customers/{}/sharedSets/{}", cid, shared_set_id);

    let operations: Vec<serde_json::Value> = campaign_ids
        .iter()
        .map(|campaign_id| {
            json!({
                "campaignSharedSetOperation": {
                    "create": {
                        "campaign": format!("customers/{}/campaigns/{}", cid, campaign_id),
                        "sharedSet": shared_set_resource
                    }
                }
            })
        })
        .collect();

    let changes = json!({
        "shared_set_id": shared_set_id,
        "campaign_ids": campaign_ids,
    });

    let plan = ChangePlan::new(
        "attach_negative_keyword_list".to_string(),
        "campaign_shared_set".to_string(),
        shared_set_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Detach a negative keyword list from one or more campaigns.
///
/// Destructive: those campaigns immediately lose every exclusion the list
/// carried, so this requires double confirmation.
pub fn detach_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    shared_set_id: &str,
    campaign_ids: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("detach_negative_keyword_list", &config.safety)?;

    validate_numeric_id("shared_set_id", shared_set_id)?;
    if campaign_ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one campaign ID is required".to_string(),
        ));
    }
    for campaign_id in campaign_ids {
        validate_numeric_id("campaign_id", campaign_id)?;
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    // The CampaignSharedSet resource ID is the composite {campaign_id}~{shared_set_id}.
    let operations: Vec<serde_json::Value> = campaign_ids
        .iter()
        .map(|campaign_id| {
            json!({
                "campaignSharedSetOperation": {
                    "remove": format!(
                        "customers/{}/campaignSharedSets/{}~{}",
                        cid, campaign_id, shared_set_id
                    )
                }
            })
        })
        .collect();

    let changes = json!({
        "shared_set_id": shared_set_id,
        "campaign_ids": campaign_ids,
        "warning": "These campaigns lose every exclusion this list carries. The list itself is not deleted.",
    });

    let plan = ChangePlan::new(
        "detach_negative_keyword_list".to_string(),
        "campaign_shared_set".to_string(),
        shared_set_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Delete a negative keyword list entirely.
///
/// Destructive: requires double confirmation. Every campaign it is attached to
/// loses those exclusions.
pub fn delete_negative_keyword_list(
    config: &Config,
    customer_id: &str,
    shared_set_id: &str,
) -> Result<serde_json::Value> {
    check_blocked_operation("delete_negative_keyword_list", &config.safety)?;

    validate_numeric_id("shared_set_id", shared_set_id)?;

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);

    let operations = vec![json!({
        "sharedSetOperation": {
            "remove": format!("customers/{}/sharedSets/{}", cid, shared_set_id)
        }
    })];

    let changes = json!({
        "shared_set_id": shared_set_id,
        "warning": "This action is destructive and cannot be undone. Every campaign attached to this list loses all of its exclusions.",
    });

    let plan = ChangePlan::new(
        "delete_negative_keyword_list".to_string(),
        "shared_set".to_string(),
        shared_set_id.to_string(),
        cid,
        changes,
        true,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::preview::get_plan;

    fn ops_of(preview: &serde_json::Value) -> Vec<serde_json::Value> {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        get_plan(plan_id).expect("plan stored").mutate_operations
    }

    #[test]
    fn test_dedupe_keywords_case_insensitive_and_trims() {
        let (unique, dropped) = dedupe_keywords(vec![
            "gratis".to_string(),
            "GRATIS".to_string(),
            "  sus  ".to_string(),
            "sus".to_string(),
            "   ".to_string(),
            "ubs".to_string(),
        ]);
        assert_eq!(unique, vec!["gratis", "sus", "ubs"]);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn test_create_list_chains_temp_resource_id() {
        let config = Config::default();
        let preview = create_negative_keyword_list(
            &config,
            "123-456-7890",
            "NEG - Genericas",
            vec!["gratis".to_string(), "sus".to_string()],
            "PHRASE",
            &["111".to_string(), "222".to_string()],
        )
        .expect("ok");

        let ops = ops_of(&preview);
        // 1 set + 2 keywords + 2 campaign links
        assert_eq!(ops.len(), 5);

        let temp = "customers/1234567890/sharedSets/-1";
        assert_eq!(
            ops[0].pointer("/sharedSetOperation/create/resourceName"),
            Some(&json!(temp))
        );
        assert_eq!(
            ops[0].pointer("/sharedSetOperation/create/type"),
            Some(&json!("NEGATIVE_KEYWORDS"))
        );

        // Every criterion and link must point at the temp resource, otherwise
        // the chain breaks and Google creates orphans.
        for op in &ops[1..3] {
            assert_eq!(
                op.pointer("/sharedCriterionOperation/create/sharedSet"),
                Some(&json!(temp))
            );
            assert_eq!(
                op.pointer("/sharedCriterionOperation/create/keyword/matchType"),
                Some(&json!("PHRASE"))
            );
        }
        for op in &ops[3..5] {
            assert_eq!(
                op.pointer("/campaignSharedSetOperation/create/sharedSet"),
                Some(&json!(temp))
            );
        }
        assert_eq!(
            ops[3].pointer("/campaignSharedSetOperation/create/campaign"),
            Some(&json!("customers/1234567890/campaigns/111"))
        );
    }

    #[test]
    fn test_create_list_allows_empty_keywords() {
        let config = Config::default();
        let preview = create_negative_keyword_list(
            &config,
            "1234567890",
            "Empty list",
            vec![],
            "PHRASE",
            &[],
        )
        .expect("an empty list is valid — keywords can be added later");
        assert_eq!(ops_of(&preview).len(), 1);
    }

    #[test]
    fn test_create_list_requires_name() {
        let config = Config::default();
        let err = create_negative_keyword_list(&config, "1234567890", "   ", vec![], "PHRASE", &[])
            .expect_err("blank name rejected");
        assert!(err.to_string().contains("name is required"));
    }

    #[test]
    fn test_create_list_rejects_bad_match_type() {
        let config = Config::default();
        assert!(create_negative_keyword_list(
            &config,
            "1234567890",
            "L",
            vec!["x".to_string()],
            "FUZZY",
            &[]
        )
        .is_err());
    }

    #[test]
    fn test_create_list_rejects_non_numeric_campaign_id() {
        let config = Config::default();
        let err = create_negative_keyword_list(
            &config,
            "1234567890",
            "L",
            vec![],
            "PHRASE",
            &["customers/1/campaigns/2".to_string()],
        )
        .expect_err("resource name rejected as campaign_id");
        assert!(err.to_string().contains("numeric ID"));
    }

    #[test]
    fn test_create_list_reports_dropped_duplicates() {
        let config = Config::default();
        let preview = create_negative_keyword_list(
            &config,
            "1234567890",
            "L",
            vec!["sus".to_string(), "SUS".to_string()],
            "PHRASE",
            &[],
        )
        .expect("ok");
        assert_eq!(preview["changes"]["duplicates_dropped"], 1);
        assert_eq!(preview["changes"]["keyword_count"], 1);
    }

    #[test]
    fn test_add_to_list_targets_real_set_resource() {
        let config = Config::default();
        let preview = add_to_negative_keyword_list(
            &config,
            "1234567890",
            "999",
            vec!["gratis".to_string()],
            "PHRASE",
        )
        .expect("ok");

        let ops = ops_of(&preview);
        assert_eq!(ops.len(), 1);
        assert_eq!(
            ops[0].pointer("/sharedCriterionOperation/create/sharedSet"),
            Some(&json!("customers/1234567890/sharedSets/999"))
        );
        assert_eq!(preview["requires_double_confirm"], false);
    }

    #[test]
    fn test_add_to_list_rejects_empty_after_dedupe() {
        let config = Config::default();
        let err = add_to_negative_keyword_list(
            &config,
            "1234567890",
            "999",
            vec!["   ".to_string()],
            "PHRASE",
        )
        .expect_err("whitespace-only input leaves nothing to add");
        assert!(err.to_string().contains("At least one keyword"));
    }

    #[test]
    fn test_add_to_list_rejects_over_limit() {
        let config = Config::default();
        let keywords: Vec<String> = (0..MAX_SHARED_SET_KEYWORDS + 1)
            .map(|i| format!("kw{}", i))
            .collect();
        let err = add_to_negative_keyword_list(&config, "1234567890", "999", keywords, "PHRASE")
            .expect_err("over-limit batch rejected");
        assert!(err.to_string().contains("at most 5000"));
    }

    #[test]
    fn test_remove_from_list_is_double_confirm() {
        let config = Config::default();
        let preview = remove_from_negative_keyword_list(
            &config,
            "1234567890",
            "999",
            vec!["777".to_string()],
        )
        .expect("ok");

        assert_eq!(preview["requires_double_confirm"], true);
        let ops = ops_of(&preview);
        assert_eq!(
            ops[0].pointer("/sharedCriterionOperation/remove"),
            Some(&json!("customers/1234567890/sharedCriteria/999~777"))
        );
    }

    #[test]
    fn test_attach_builds_campaign_links() {
        let config = Config::default();
        let preview =
            attach_negative_keyword_list(&config, "1234567890", "999", &["111".to_string()])
                .expect("ok");

        let ops = ops_of(&preview);
        assert_eq!(
            ops[0].pointer("/campaignSharedSetOperation/create/campaign"),
            Some(&json!("customers/1234567890/campaigns/111"))
        );
        assert_eq!(
            ops[0].pointer("/campaignSharedSetOperation/create/sharedSet"),
            Some(&json!("customers/1234567890/sharedSets/999"))
        );
    }

    #[test]
    fn test_attach_requires_campaign() {
        let config = Config::default();
        assert!(attach_negative_keyword_list(&config, "1234567890", "999", &[]).is_err());
    }

    #[test]
    fn test_detach_uses_campaign_first_composite_id() {
        let config = Config::default();
        let preview =
            detach_negative_keyword_list(&config, "1234567890", "999", &["111".to_string()])
                .expect("ok");

        assert_eq!(preview["requires_double_confirm"], true);
        let ops = ops_of(&preview);
        // Composite order is {campaign_id}~{shared_set_id} — reversing it
        // addresses a different (usually nonexistent) resource.
        assert_eq!(
            ops[0].pointer("/campaignSharedSetOperation/remove"),
            Some(&json!("customers/1234567890/campaignSharedSets/111~999"))
        );
    }

    #[test]
    fn test_delete_list_is_double_confirm() {
        let config = Config::default();
        let preview = delete_negative_keyword_list(&config, "1234567890", "999").expect("ok");

        assert_eq!(preview["requires_double_confirm"], true);
        let ops = ops_of(&preview);
        assert_eq!(
            ops[0].pointer("/sharedSetOperation/remove"),
            Some(&json!("customers/1234567890/sharedSets/999"))
        );
    }

    #[test]
    fn test_blocked_operation_is_honoured() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_negative_keyword_list".to_string()];
        let err = create_negative_keyword_list(&config, "1234567890", "L", vec![], "PHRASE", &[])
            .expect_err("blocked");
        assert!(err.to_string().contains("blocked"));
    }
}
