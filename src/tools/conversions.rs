use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

/// Maximum length accepted for a conversion action name.
const MAX_CONVERSION_ACTION_NAME_LEN: usize = 100;

/// List all conversion actions configured in the account.
pub async fn get_conversion_actions(client: &GoogleAdsClient, customer_id: &str) -> Result<String> {
    let query = "\
        SELECT \
            conversion_action.id, \
            conversion_action.name, \
            conversion_action.type, \
            conversion_action.status, \
            conversion_action.category, \
            conversion_action.value_settings.default_value, \
            conversion_action.counting_type \
        FROM conversion_action \
        WHERE conversion_action.status != 'REMOVED' \
        ORDER BY conversion_action.name \
        LIMIT 200";

    let rows = client.search(customer_id, query).await?;

    let result = serde_json::json!({
        "conversion_actions": rows,
        "total_count": rows.len(),
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Draft a new conversion action for server-side click uploads.
///
/// The action is created with `type = UPLOAD_CLICKS`, the source Google Ads uses
/// for gclid-based offline conversion imports: an uploaded click identifier
/// (gclid) is matched back to the originating ad click. No value settings are
/// attached, so valueless uploads (gclid + conversion time, no revenue) are
/// accepted — which is exactly how a free-signup funnel reports its "Signup" and
/// "Activation" events.
///
/// Returns a [`ChangePlan`] preview that must be confirmed via
/// `confirm_and_apply`. After apply, the numeric ID needed for an
/// `*_CONVERSION_ACTION_ID` environment variable is the trailing segment of the
/// returned `conversionAction` resource name
/// (`customers/{cid}/conversionActions/{id}`).
pub fn create_conversion_action(
    config: &Config,
    customer_id: &str,
    name: &str,
    category: &str,
    counting_type: &str,
    click_through_lookback_window_days: i64,
) -> Result<serde_json::Value> {
    check_blocked_operation("create_conversion_action", &config.safety)?;

    let name = name.trim();
    if name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "Conversion action name must not be empty".to_string(),
        ));
    }
    if name.chars().count() > MAX_CONVERSION_ACTION_NAME_LEN {
        return Err(McpGoogleAdsError::Validation(format!(
            "Conversion action name must be at most {} characters",
            MAX_CONVERSION_ACTION_NAME_LEN
        )));
    }

    let counting_type = counting_type.trim().to_uppercase();
    if counting_type != "ONE_PER_CLICK" && counting_type != "MANY_PER_CLICK" {
        return Err(McpGoogleAdsError::Validation(format!(
            "counting_type must be ONE_PER_CLICK or MANY_PER_CLICK, got '{}'",
            counting_type
        )));
    }

    // Google Ads accepts a click-through lookback window of 1 to 90 days.
    if !(1..=90).contains(&click_through_lookback_window_days) {
        return Err(McpGoogleAdsError::Validation(format!(
            "click_through_lookback_window_days must be between 1 and 90, got {}",
            click_through_lookback_window_days
        )));
    }

    let category = category.trim().to_uppercase();
    if category.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "category must not be empty".to_string(),
        ));
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);

    let create = json!({
        "name": name,
        "type": "UPLOAD_CLICKS",
        "category": category,
        "status": "ENABLED",
        "countingType": counting_type,
        "clickThroughLookbackWindowDays": click_through_lookback_window_days,
    });

    let operations = vec![json!({
        "conversionActionOperation": {
            "create": create
        }
    })];

    let changes = json!({
        "name": name,
        "type": "UPLOAD_CLICKS",
        "category": category,
        "counting_type": counting_type,
        "click_through_lookback_window_days": click_through_lookback_window_days,
        "note": "After apply, the numeric conversion action ID is the trailing segment of the returned conversionAction resourceName (customers/{cid}/conversionActions/{id}).",
    });

    let plan = ChangePlan::new(
        "create_conversion_action".to_string(),
        "conversion_action".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::safety::preview::{get_plan, remove_plan};

    /// Extract the single `conversionActionOperation.create` object from a stored plan.
    fn created_action(preview: &serde_json::Value) -> serde_json::Value {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        let op = plan
            .mutate_operations
            .first()
            .expect("one mutate operation")
            .clone();
        remove_plan(plan_id);
        op["conversionActionOperation"]["create"].clone()
    }

    #[test]
    fn test_create_conversion_action_success() {
        let config = Config::default();
        let preview = create_conversion_action(
            &config,
            "123-456-7890",
            "Activation",
            "SIGNUP",
            "ONE_PER_CLICK",
            30,
        )
        .expect("ok");

        assert_eq!(preview["operation"], "create_conversion_action");
        assert_eq!(preview["status"], "PENDING_CONFIRMATION");
        assert!(preview["plan_id"].as_str().is_some());

        let create = created_action(&preview);
        // The whole point of this tool: it only ever creates click-upload actions.
        assert_eq!(create["type"], "UPLOAD_CLICKS");
        assert_eq!(create["name"], "Activation");
        assert_eq!(create["category"], "SIGNUP");
        assert_eq!(create["status"], "ENABLED");
        assert_eq!(create["countingType"], "ONE_PER_CLICK");
        assert_eq!(create["clickThroughLookbackWindowDays"], 30);
        // Valueless: no value settings so uploads without revenue are accepted.
        assert!(create.get("valueSettings").is_none());
    }

    #[test]
    fn test_category_and_counting_type_are_normalized() {
        let config = Config::default();
        let preview = create_conversion_action(
            &config,
            "1234567890",
            "  Activation  ",
            "signup",
            "one_per_click",
            30,
        )
        .expect("ok");
        let create = created_action(&preview);
        assert_eq!(create["name"], "Activation");
        assert_eq!(create["category"], "SIGNUP");
        assert_eq!(create["countingType"], "ONE_PER_CLICK");
    }

    #[test]
    fn test_window_boundaries_accepted() {
        let config = Config::default();
        for window in [1, 90] {
            let preview = create_conversion_action(
                &config,
                "1234567890",
                "Activation",
                "SIGNUP",
                "ONE_PER_CLICK",
                window,
            )
            .expect("boundary window accepted");
            let create = created_action(&preview);
            assert_eq!(create["clickThroughLookbackWindowDays"], window);
        }
    }

    #[test]
    fn test_window_out_of_range_rejected() {
        let config = Config::default();
        for window in [0, 91, -1, 365] {
            let err = create_conversion_action(
                &config,
                "1234567890",
                "Activation",
                "SIGNUP",
                "ONE_PER_CLICK",
                window,
            )
            .expect_err("out-of-range window rejected");
            assert!(err.to_string().contains("between 1 and 90"));
        }
    }

    #[test]
    fn test_empty_name_rejected() {
        let config = Config::default();
        let err =
            create_conversion_action(&config, "1234567890", "   ", "SIGNUP", "ONE_PER_CLICK", 30)
                .expect_err("empty name rejected");
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_name_too_long_rejected() {
        let config = Config::default();
        let long_name = "a".repeat(MAX_CONVERSION_ACTION_NAME_LEN + 1);
        let err = create_conversion_action(
            &config,
            "1234567890",
            &long_name,
            "SIGNUP",
            "ONE_PER_CLICK",
            30,
        )
        .expect_err("over-long name rejected");
        assert!(err.to_string().contains("at most"));
    }

    #[test]
    fn test_invalid_counting_type_rejected() {
        let config = Config::default();
        let err = create_conversion_action(
            &config,
            "1234567890",
            "Activation",
            "SIGNUP",
            "TWICE_PER_CLICK",
            30,
        )
        .expect_err("invalid counting type rejected");
        assert!(err.to_string().contains("ONE_PER_CLICK"));
    }

    #[test]
    fn test_empty_category_rejected() {
        let config = Config::default();
        let err = create_conversion_action(
            &config,
            "1234567890",
            "Activation",
            "  ",
            "ONE_PER_CLICK",
            30,
        )
        .expect_err("empty category rejected");
        assert!(err.to_string().contains("category"));
    }

    #[test]
    fn test_blocked_operation_rejected() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["create_conversion_action".to_string()];
        let err = create_conversion_action(
            &config,
            "1234567890",
            "Activation",
            "SIGNUP",
            "ONE_PER_CLICK",
            30,
        )
        .expect_err("blocked operation rejected");
        assert!(err.to_string().contains("blocked"));
    }
}
