//! Input helpers shared by the criterion-writing tools.
//!
//! Every tool that fans a list of IDs out into one mutate operation each needs
//! the same two guards: an ID must be a bare positive integer (a resource name
//! or a display name in its place would otherwise reach Google as a malformed
//! resource and fail with a message that points nowhere), and a repeated ID
//! must not become a repeated operation (Google rejects the whole batch on the
//! duplicate resource name).

use std::collections::HashSet;

use crate::error::{McpGoogleAdsError, Result};

/// Upper bound on criteria written in a single plan.
///
/// Google's own mutate ceiling is far higher; this is a guard against a caller
/// fanning out over every ad group in a large account by accident. Exceeding it
/// is a hard error rather than a truncation — a silently partial exclusion is
/// worse than none, because it reads as done.
pub(crate) const MAX_CRITERIA_PER_PLAN: usize = 500;

/// Validate an ID is a bare positive integer, as Google Ads IDs always are.
pub(crate) fn validate_numeric_id(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must not be empty",
            label
        )));
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must be numeric, got '{}'",
            label, value
        )));
    }
    Ok(trimmed.to_string())
}

/// Deduplicate while preserving caller order, so the preview reads predictably.
pub(crate) fn dedupe_preserving_order<T: Clone + std::hash::Hash + Eq>(items: &[T]) -> Vec<T> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert((*item).clone()))
        .cloned()
        .collect()
}

/// Validate a required list of IDs: not empty, every entry a bare positive
/// integer, repeats dropped in caller order.
///
/// `field` names the parameter in the empty-list error (`ad_group_ids`),
/// `label` names one entry in the per-ID error (`ad_group_id`), and `hint` says
/// where the IDs come from.
pub(crate) fn validate_id_list(
    field: &str,
    label: &str,
    ids: &[String],
    hint: &str,
) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Err(McpGoogleAdsError::Validation(format!(
            "{} must not be empty — {}",
            field, hint
        )));
    }
    let validated = ids
        .iter()
        .map(|id| validate_numeric_id(label, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(dedupe_preserving_order(&validated))
}

/// Refuse a fan-out of one criterion per (ad group × item) that would exceed
/// [`MAX_CRITERIA_PER_PLAN`]. Returns the number of criteria otherwise.
pub(crate) fn check_fanout(ad_groups: usize, items: usize, item_noun: &str) -> Result<usize> {
    let total = ad_groups * items;
    if total > MAX_CRITERIA_PER_PLAN {
        return Err(McpGoogleAdsError::Validation(format!(
            "plan would write {} criteria ({} ad groups × {} {}), over the {} limit — \
             split it into smaller batches",
            total, ad_groups, items, item_noun, MAX_CRITERIA_PER_PLAN
        )));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_id_is_trimmed() {
        assert_eq!(validate_numeric_id("x", " 123 ").unwrap(), "123");
    }

    #[test]
    fn non_numeric_and_empty_ids_rejected() {
        assert!(validate_numeric_id("x", "").is_err());
        assert!(validate_numeric_id("x", "customers/1/userLists/2").is_err());
        assert!(validate_numeric_id("x", "-5").is_err());
    }

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let input = ["b", "a", "b", "c", "a"].map(String::from);
        assert_eq!(dedupe_preserving_order(&input), vec!["b", "a", "c"]);
    }

    #[test]
    fn id_list_validates_dedupes_and_names_the_field() {
        let ids = [" 7 ", "8", "7"].map(String::from);
        assert_eq!(
            validate_id_list("x_ids", "x_id", &ids, "hint").unwrap(),
            vec!["7", "8"]
        );
        let empty = validate_id_list("x_ids", "x_id", &[], "find them first").unwrap_err();
        assert!(empty
            .to_string()
            .contains("x_ids must not be empty — find them first"));
        let bad = validate_id_list("x_ids", "x_id", &["abc".to_string()], "h").unwrap_err();
        assert!(bad.to_string().contains("x_id must be numeric"));
    }

    #[test]
    fn fanout_over_the_limit_is_refused() {
        assert_eq!(check_fanout(2, 3, "segments").unwrap(), 6);
        let err = check_fanout(251, 2, "segments").unwrap_err().to_string();
        assert!(err.contains("502 criteria (251 ad groups × 2 segments)"));
    }
}
