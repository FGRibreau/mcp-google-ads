use crate::config::SafetyConfig;
use crate::error::{McpGoogleAdsError, Result};

/// Check if proposed daily budget exceeds configured maximum
pub fn check_budget_cap(daily_budget: f64, config: &SafetyConfig) -> Result<()> {
    if daily_budget > config.max_daily_budget {
        return Err(McpGoogleAdsError::Safety(format!(
            "Daily budget {:.2} exceeds maximum {:.2}",
            daily_budget, config.max_daily_budget
        )));
    }
    Ok(())
}

/// Check if bid increase percentage exceeds configured maximum
pub fn check_bid_increase(
    current_bid: f64,
    proposed_bid: f64,
    config: &SafetyConfig,
) -> Result<()> {
    if current_bid <= 0.0 {
        return Ok(());
    }
    let increase_pct = ((proposed_bid - current_bid) / current_bid) * 100.0;
    if increase_pct > config.max_bid_increase_pct as f64 {
        return Err(McpGoogleAdsError::Safety(format!(
            "Bid increase {:.0}% exceeds maximum {}%",
            increase_pct, config.max_bid_increase_pct
        )));
    }
    Ok(())
}

/// Check if operation is in the blocked list
pub fn check_blocked_operation(operation: &str, config: &SafetyConfig) -> Result<()> {
    if config.blocked_operations.contains(&operation.to_string()) {
        return Err(McpGoogleAdsError::Safety(format!(
            "Operation '{}' is blocked by configuration",
            operation
        )));
    }
    Ok(())
}

/// Check if Broad match + Manual CPC combination is being used (dangerous)
pub fn check_broad_manual_cpc(match_type: &str, bidding_strategy: &str) -> Result<()> {
    if match_type == "BROAD" && bidding_strategy == "MANUAL_CPC" {
        return Err(McpGoogleAdsError::Safety(
            "BROAD match with MANUAL_CPC is blocked — this combination burns budget. Use Smart Bidding (MAXIMIZE_CONVERSIONS, TARGET_CPA) with BROAD match, or use EXACT/PHRASE with MANUAL_CPC.".to_string(),
        ));
    }
    Ok(())
}

/// Return true if this operation needs double confirmation (destructive ops, large budget changes)
pub fn requires_double_confirmation(
    operation: &str,
    current_budget: Option<f64>,
    proposed_budget: Option<f64>,
) -> bool {
    if operation.contains("delete") || operation.contains("remove") {
        return true;
    }
    if let (Some(current), Some(proposed)) = (current_budget, proposed_budget) {
        if current > 0.0 && ((proposed - current) / current) > 0.5 {
            return true;
        }
    }
    false
}

/// Validate headline character limit (max 30 chars for RSA)
pub fn validate_headline(headline: &str) -> Result<()> {
    let char_count = headline.chars().count();
    if char_count > 30 {
        return Err(McpGoogleAdsError::Validation(format!(
            "Headline '{}' exceeds 30 character limit ({} chars)",
            headline, char_count
        )));
    }
    Ok(())
}

/// Validate description character limit (max 90 chars for RSA)
pub fn validate_description(desc: &str) -> Result<()> {
    let char_count = desc.chars().count();
    if char_count > 90 {
        return Err(McpGoogleAdsError::Validation(format!(
            "Description exceeds 90 character limit ({} chars)",
            char_count
        )));
    }
    Ok(())
}

/// Validate sitelink text (max 25 chars)
pub fn validate_sitelink_text(text: &str) -> Result<()> {
    let char_count = text.chars().count();
    if char_count > 25 {
        return Err(McpGoogleAdsError::Validation(format!(
            "Sitelink text '{}' exceeds 25 character limit ({} chars)",
            text, char_count
        )));
    }
    Ok(())
}

/// Validate sitelink description (max 35 chars)
pub fn validate_sitelink_description(desc: &str) -> Result<()> {
    let char_count = desc.chars().count();
    if char_count > 35 {
        return Err(McpGoogleAdsError::Validation(format!(
            "Sitelink description exceeds 35 character limit ({} chars)",
            char_count
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SafetyConfig;

    fn config_with_budget(max: f64) -> SafetyConfig {
        SafetyConfig {
            max_daily_budget: max,
            ..SafetyConfig::default()
        }
    }

    fn config_with_bid_pct(pct: u32) -> SafetyConfig {
        SafetyConfig {
            max_bid_increase_pct: pct,
            ..SafetyConfig::default()
        }
    }

    #[test]
    fn test_budget_cap_allows_within() {
        let config = config_with_budget(50.0);
        assert!(check_budget_cap(49.99, &config).is_ok());
    }

    #[test]
    fn test_budget_cap_rejects_over() {
        let config = config_with_budget(50.0);
        assert!(check_budget_cap(51.0, &config).is_err());
    }

    #[test]
    fn test_budget_cap_exact_limit() {
        let config = config_with_budget(50.0);
        assert!(check_budget_cap(50.0, &config).is_ok());
    }

    #[test]
    fn test_bid_increase_allows_within() {
        let config = config_with_bid_pct(100);
        assert!(check_bid_increase(1.0, 2.0, &config).is_ok());
    }

    #[test]
    fn test_bid_increase_rejects_over() {
        let config = config_with_bid_pct(100);
        assert!(check_bid_increase(1.0, 2.5, &config).is_err());
    }

    #[test]
    fn test_bid_increase_zero_current() {
        let config = config_with_bid_pct(100);
        assert!(check_bid_increase(0.0, 999.0, &config).is_ok());
    }

    #[test]
    fn test_blocked_operation_allows() {
        let config = SafetyConfig {
            blocked_operations: vec!["delete_campaign".to_string()],
            ..SafetyConfig::default()
        };
        assert!(check_blocked_operation("create_campaign", &config).is_ok());
    }

    #[test]
    fn test_blocked_operation_rejects() {
        let config = SafetyConfig {
            blocked_operations: vec!["delete_campaign".to_string()],
            ..SafetyConfig::default()
        };
        assert!(check_blocked_operation("delete_campaign", &config).is_err());
    }

    #[test]
    fn test_broad_manual_cpc_blocked() {
        assert!(check_broad_manual_cpc("BROAD", "MANUAL_CPC").is_err());
    }

    #[test]
    fn test_broad_smart_bidding_allowed() {
        assert!(check_broad_manual_cpc("BROAD", "MAXIMIZE_CONVERSIONS").is_ok());
    }

    #[test]
    fn test_exact_manual_cpc_allowed() {
        assert!(check_broad_manual_cpc("EXACT", "MANUAL_CPC").is_ok());
    }

    #[test]
    fn test_double_confirm_delete() {
        assert!(requires_double_confirmation("delete_campaign", None, None));
    }

    #[test]
    fn test_double_confirm_remove() {
        assert!(requires_double_confirmation("remove_entity", None, None));
    }

    #[test]
    fn test_double_confirm_pause() {
        assert!(!requires_double_confirmation("pause_entity", None, None));
    }

    #[test]
    fn test_double_confirm_large_budget_increase() {
        assert!(requires_double_confirmation(
            "update_budget",
            Some(10.0),
            Some(20.0)
        ));
    }

    #[test]
    fn test_double_confirm_small_budget_increase() {
        assert!(!requires_double_confirmation(
            "update_budget",
            Some(10.0),
            Some(14.0)
        ));
    }

    #[test]
    fn test_validate_headline_ok() {
        assert!(validate_headline(&"A".repeat(30)).is_ok());
    }

    #[test]
    fn test_validate_headline_too_long() {
        assert!(validate_headline(&"A".repeat(31)).is_err());
    }

    #[test]
    fn test_validate_description_ok() {
        assert!(validate_description(&"A".repeat(90)).is_ok());
    }

    #[test]
    fn test_validate_description_too_long() {
        assert!(validate_description(&"A".repeat(91)).is_err());
    }

    #[test]
    fn test_validate_sitelink_text_ok() {
        assert!(validate_sitelink_text(&"A".repeat(25)).is_ok());
    }

    #[test]
    fn test_validate_sitelink_text_too_long() {
        assert!(validate_sitelink_text(&"A".repeat(26)).is_err());
    }

    #[test]
    fn test_validate_sitelink_desc_ok() {
        assert!(validate_sitelink_description(&"A".repeat(35)).is_ok());
    }

    #[test]
    fn test_validate_sitelink_desc_too_long() {
        assert!(validate_sitelink_description(&"A".repeat(36)).is_err());
    }

    #[test]
    fn test_budget_cap_zero() {
        let config = config_with_budget(50.0);
        assert!(check_budget_cap(0.0, &config).is_ok());
    }

    #[test]
    fn test_budget_cap_negative() {
        let config = config_with_budget(50.0);
        assert!(check_budget_cap(-1.0, &config).is_ok());
    }

    #[test]
    fn test_bid_increase_decrease() {
        let config = config_with_bid_pct(100);
        assert!(check_bid_increase(2.0, 1.0, &config).is_ok());
    }

    #[test]
    fn test_bid_increase_negative_bid() {
        let config = config_with_bid_pct(100);
        assert!(check_bid_increase(-1.0, 1.0, &config).is_ok());
    }

    #[test]
    fn test_double_confirm_zero_current() {
        // current is 0.0, so the >0.0 check fails, no double confirm needed
        assert!(!requires_double_confirmation(
            "update_budget",
            Some(0.0),
            Some(100.0)
        ));
    }

    #[test]
    fn test_blocked_empty_list() {
        let config = SafetyConfig {
            blocked_operations: vec![],
            ..SafetyConfig::default()
        };
        assert!(check_blocked_operation("any_operation", &config).is_ok());
    }

    #[test]
    fn test_validate_headline_unicode() {
        // 6 CJK characters = 6 chars but 18 bytes (3 bytes each)
        let headline = "日本語テスト";
        assert_eq!(headline.chars().count(), 6);
        assert!(headline.len() > 6); // bytes > chars
        assert!(validate_headline(headline).is_ok());
    }

    #[test]
    fn test_validate_headline_unicode_at_limit() {
        // 30 CJK characters should be OK
        let headline: String = "日".repeat(30);
        assert!(validate_headline(&headline).is_ok());
        // 31 should fail
        let headline_over: String = "日".repeat(31);
        assert!(validate_headline(&headline_over).is_err());
    }

    #[test]
    fn test_validate_description_unicode() {
        // Emojis: each is typically 4 bytes but 1 char
        let desc: String = "\u{1F600}".repeat(90); // 90 emoji chars
        assert_eq!(desc.chars().count(), 90);
        assert!(validate_description(&desc).is_ok());
        let desc_over: String = "\u{1F600}".repeat(91);
        assert!(validate_description(&desc_over).is_err());
    }

    #[test]
    fn test_validate_sitelink_text_unicode() {
        // Accented characters: e.g. "é" is 2 bytes but 1 char
        let text: String = "é".repeat(25);
        assert_eq!(text.chars().count(), 25);
        assert!(validate_sitelink_text(&text).is_ok());
        let text_over: String = "é".repeat(26);
        assert!(validate_sitelink_text(&text_over).is_err());
    }

    #[test]
    fn test_validate_sitelink_desc_unicode() {
        // Mixed unicode: CJK + accented
        let desc = "日本語テストéàüöñ"; // 11 chars
        assert_eq!(desc.chars().count(), 11);
        assert!(validate_sitelink_description(desc).is_ok());
        // 35 CJK chars exactly at limit
        let desc_at_limit: String = "漢".repeat(35);
        assert!(validate_sitelink_description(&desc_at_limit).is_ok());
        let desc_over: String = "漢".repeat(36);
        assert!(validate_sitelink_description(&desc_over).is_err());
    }
}
