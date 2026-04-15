use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan};

const VALID_DAYS: &[&str] = &[
    "MONDAY",
    "TUESDAY",
    "WEDNESDAY",
    "THURSDAY",
    "FRIDAY",
    "SATURDAY",
    "SUNDAY",
];

/// A schedule entry for ad scheduling.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleEntry {
    pub day_of_week: String,
    pub start_hour: u32,
    pub start_minute: u32,
    pub end_hour: u32,
    pub end_minute: u32,
}

/// Set campaign ad schedules.
///
/// Each schedule entry specifies a day of week and time window when ads should run.
/// Valid minutes are 0, 15, 30, 45. Valid hours are 0-24.
///
/// Returns a ChangePlan preview that must be confirmed via `confirm_and_apply`.
pub fn set_campaign_schedule(
    config: &Config,
    customer_id: &str,
    campaign_id: &str,
    schedules: Vec<ScheduleEntry>,
) -> Result<serde_json::Value> {
    check_blocked_operation("set_campaign_schedule", &config.safety)?;

    if schedules.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "At least one schedule entry is required".to_string(),
        ));
    }

    // Validate schedules
    for sched in &schedules {
        if !VALID_DAYS.contains(&sched.day_of_week.as_str()) {
            return Err(McpGoogleAdsError::Validation(format!(
                "Invalid day '{}'. Must be one of: {}",
                sched.day_of_week,
                VALID_DAYS.join(", ")
            )));
        }
        if sched.start_hour > 24 || sched.end_hour > 24 {
            return Err(McpGoogleAdsError::Validation(format!(
                "Hours must be 0-24, got start={} end={}",
                sched.start_hour, sched.end_hour
            )));
        }
        let valid_minutes = [0, 15, 30, 45];
        if !valid_minutes.contains(&sched.start_minute)
            || !valid_minutes.contains(&sched.end_minute)
        {
            return Err(McpGoogleAdsError::Validation(format!(
                "Minutes must be 0, 15, 30, or 45, got start_minute={} end_minute={}",
                sched.start_minute, sched.end_minute
            )));
        }
    }

    let cid = crate::client::GoogleAdsClient::normalize_customer_id(customer_id);
    let campaign_resource = format!("customers/{}/campaigns/{}", cid, campaign_id);

    let operations: Vec<serde_json::Value> = schedules
        .iter()
        .map(|sched| {
            json!({
                "campaignCriterionOperation": {
                    "create": {
                        "campaign": campaign_resource,
                        "adSchedule": {
                            "dayOfWeek": sched.day_of_week,
                            "startHour": sched.start_hour,
                            "startMinute": minute_enum(sched.start_minute),
                            "endHour": sched.end_hour,
                            "endMinute": minute_enum(sched.end_minute)
                        }
                    }
                }
            })
        })
        .collect();

    let schedule_summary: Vec<serde_json::Value> = schedules
        .iter()
        .map(|s| {
            json!({
                "day": s.day_of_week,
                "start": format!("{:02}:{:02}", s.start_hour, s.start_minute),
                "end": format!("{:02}:{:02}", s.end_hour, s.end_minute)
            })
        })
        .collect();

    let changes = json!({
        "campaign_id": campaign_id,
        "schedules": schedule_summary
    });

    let plan = ChangePlan::new(
        "set_campaign_schedule".to_string(),
        "campaign_criterion".to_string(),
        campaign_id.to_string(),
        cid,
        changes,
        false,
        operations,
    );

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

/// Convert minute value to Google Ads MinuteOfHour enum string.
fn minute_enum(minute: u32) -> &'static str {
    match minute {
        0 => "ZERO",
        15 => "FIFTEEN",
        30 => "THIRTY",
        45 => "FORTY_FIVE",
        _ => "ZERO",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn monday_schedule() -> ScheduleEntry {
        ScheduleEntry {
            day_of_week: "MONDAY".to_string(),
            start_hour: 9,
            start_minute: 0,
            end_hour: 17,
            end_minute: 0,
        }
    }

    #[test]
    fn test_set_campaign_schedule_success() {
        let config = Config::default();
        let result = set_campaign_schedule(&config, "123-456-7890", "555", vec![monday_schedule()]);
        assert!(result.is_ok());
        let preview = result.ok().unwrap_or_default();
        assert_eq!(preview["operation"], "set_campaign_schedule");
    }

    #[test]
    fn test_set_campaign_schedule_multiple() {
        let config = Config::default();
        let result = set_campaign_schedule(
            &config,
            "123-456-7890",
            "555",
            vec![
                monday_schedule(),
                ScheduleEntry {
                    day_of_week: "FRIDAY".to_string(),
                    start_hour: 8,
                    start_minute: 30,
                    end_hour: 18,
                    end_minute: 0,
                },
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_campaign_schedule_empty() {
        let config = Config::default();
        let result = set_campaign_schedule(&config, "123-456-7890", "555", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_campaign_schedule_invalid_day() {
        let config = Config::default();
        let result = set_campaign_schedule(
            &config,
            "123-456-7890",
            "555",
            vec![ScheduleEntry {
                day_of_week: "FUNDAY".to_string(),
                start_hour: 9,
                start_minute: 0,
                end_hour: 17,
                end_minute: 0,
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_set_campaign_schedule_invalid_hour() {
        let config = Config::default();
        let result = set_campaign_schedule(
            &config,
            "123-456-7890",
            "555",
            vec![ScheduleEntry {
                day_of_week: "MONDAY".to_string(),
                start_hour: 25,
                start_minute: 0,
                end_hour: 17,
                end_minute: 0,
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_set_campaign_schedule_invalid_minute() {
        let config = Config::default();
        let result = set_campaign_schedule(
            &config,
            "123-456-7890",
            "555",
            vec![ScheduleEntry {
                day_of_week: "MONDAY".to_string(),
                start_hour: 9,
                start_minute: 10,
                end_hour: 17,
                end_minute: 0,
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_minute_enum() {
        assert_eq!(minute_enum(0), "ZERO");
        assert_eq!(minute_enum(15), "FIFTEEN");
        assert_eq!(minute_enum(30), "THIRTY");
        assert_eq!(minute_enum(45), "FORTY_FIVE");
    }

    #[test]
    fn test_set_campaign_schedule_blocked() {
        let mut config = Config::default();
        config.safety.blocked_operations = vec!["set_campaign_schedule".to_string()];
        let result = set_campaign_schedule(&config, "123-456-7890", "555", vec![monday_schedule()]);
        assert!(result.is_err());
    }
}
