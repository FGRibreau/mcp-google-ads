//! Custom segments — the Google Ads UI name for the API's `CustomAudience`.
//!
//! A custom segment describes people by what they search for, read or install:
//! keywords, URLs and Android apps. Unlike every other resource this server
//! writes, custom audiences are not a `googleAds:mutate` operation; they go
//! through `CustomAudienceService` (`customers/{cid}/customAudiences:mutate`),
//! so the plan carries [`PlanDispatch::CustomAudiences`] and bare
//! `CustomAudienceOperation`s.
//!
//! Member rules come from the v25 `CustomAudienceMember` documentation:
//! a keyword is at most 10 words and 80 characters (40 for double-width
//! scripts), a URL is an HTTP URL with its protocol and at most 2048
//! characters, and an app is an Android package name. The API documents no
//! limit on the number of members, so none is imposed here.

use serde_json::json;

use crate::client::GoogleAdsClient;
use crate::config::Config;
use crate::error::{McpGoogleAdsError, Result};
use crate::models::CustomSegmentType;
use crate::safety::guards::check_blocked_operation;
use crate::safety::preview::{store_plan, ChangePlan, PlanDispatch};
use crate::tools::ids::dedupe_preserving_order;

/// Longest keyword member, in characters.
const MAX_KEYWORD_CHARS: usize = 80;
/// Most words in a keyword member.
const MAX_KEYWORD_WORDS: usize = 10;
/// Longest URL member, in characters.
const MAX_URL_CHARS: usize = 2048;

/// Validate a keyword member and return it trimmed.
fn keyword_member(raw: &str) -> Result<String> {
    let keyword = raw.trim();
    if keyword.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "custom segment keywords must not be empty".to_string(),
        ));
    }
    let chars = keyword.chars().count();
    if chars > MAX_KEYWORD_CHARS {
        return Err(McpGoogleAdsError::Validation(format!(
            "keyword '{}' is {} characters, over the {} limit",
            keyword, chars, MAX_KEYWORD_CHARS
        )));
    }
    let words = keyword.split_whitespace().count();
    if words > MAX_KEYWORD_WORDS {
        return Err(McpGoogleAdsError::Validation(format!(
            "keyword '{}' has {} words, over the {} word limit",
            keyword, words, MAX_KEYWORD_WORDS
        )));
    }
    Ok(keyword.to_string())
}

/// Validate a URL member: protocol included, at most 2048 characters.
fn url_member(raw: &str) -> Result<String> {
    let url = raw.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    match rest {
        Some(host) if !host.is_empty() && host.contains('.') => {}
        _ => {
            return Err(McpGoogleAdsError::Validation(format!(
                "custom segment URL '{}' must be an http(s) URL with its protocol, \
                 e.g. https://example.com/page",
                raw
            )))
        }
    }
    if url.chars().any(char::is_whitespace) {
        return Err(McpGoogleAdsError::Validation(format!(
            "custom segment URL '{}' must not contain whitespace",
            raw
        )));
    }
    let chars = url.chars().count();
    if chars > MAX_URL_CHARS {
        return Err(McpGoogleAdsError::Validation(format!(
            "custom segment URL is {} characters, over the {} limit",
            chars, MAX_URL_CHARS
        )));
    }
    Ok(url.to_string())
}

/// Validate an app member: an Android package name such as
/// `com.example.runner` — dot-separated segments, each starting with a
/// letter and made of letters, digits and underscores.
fn app_member(raw: &str) -> Result<String> {
    let app = raw.trim();
    let segments: Vec<&str> = app.split('.').collect();
    let valid = segments.len() >= 2
        && segments.iter().all(|segment| {
            let mut chars = segment.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !valid {
        let hint = if app.starts_with("2-") || app.starts_with("1-") {
            " — pass the bare package name, without the '2-' mobile app ID prefix \
             (iOS apps cannot be custom segment members)"
        } else {
            ""
        };
        return Err(McpGoogleAdsError::Validation(format!(
            "app '{}' is not an Android package name such as com.example.app{}",
            raw, hint
        )));
    }
    Ok(app.to_string())
}

/// Draft a new custom segment (API: `CustomAudience`).
///
/// `segment_type` defaults to `AUTO` (Google picks the interpretation);
/// `SEARCH` means "people who searched for these terms on Google". The v25
/// enum's `INTEREST` and `PURCHASE_INTENT` are not accepted for new segments.
///
/// Members: `keywords`, `urls` (with protocol) and `apps` (Android package
/// names). At least one is required. Repeats are dropped — keywords
/// case-insensitively, keeping the first spelling — because Google rejects a
/// repeated `[type, value]` pair with `MEMBER_TYPE_AND_PARAMETER_ALREADY_EXISTED`.
/// The name must be unique among the account's custom segments
/// (`NAME_ALREADY_USED` otherwise).
///
/// After apply, the response's `custom_segment_ids` holds the new ID for
/// `add_ad_group_custom_segments`.
///
/// Returns a [`ChangePlan`] preview to be confirmed via `confirm_and_apply`.
#[allow(clippy::too_many_arguments)]
pub fn create_custom_segment(
    config: &Config,
    customer_id: &str,
    name: &str,
    description: Option<&str>,
    segment_type: Option<CustomSegmentType>,
    keywords: &[String],
    urls: &[String],
    apps: &[String],
) -> Result<serde_json::Value> {
    check_blocked_operation("create_custom_segment", &config.safety)?;

    let name = name.trim();
    if name.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "name must not be empty — custom segments need a name unique in the account"
                .to_string(),
        ));
    }
    if keywords.is_empty() && urls.is_empty() && apps.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "a custom segment needs at least one member — pass keywords, urls and/or apps"
                .to_string(),
        ));
    }

    let keywords = {
        let validated = keywords
            .iter()
            .map(|k| keyword_member(k))
            .collect::<Result<Vec<_>>>()?;
        let mut seen = std::collections::HashSet::new();
        validated
            .into_iter()
            .filter(|k| seen.insert(k.to_lowercase()))
            .collect::<Vec<_>>()
    };
    let urls = dedupe_preserving_order(
        &urls
            .iter()
            .map(|u| url_member(u))
            .collect::<Result<Vec<_>>>()?,
    );
    let apps = dedupe_preserving_order(
        &apps
            .iter()
            .map(|a| app_member(a))
            .collect::<Result<Vec<_>>>()?,
    );

    let segment_type = segment_type.unwrap_or(CustomSegmentType::DEFAULT);
    let description = description.map(str::trim).filter(|d| !d.is_empty());

    let mut members = Vec::with_capacity(keywords.len() + urls.len() + apps.len());
    members.extend(
        keywords
            .iter()
            .map(|k| json!({ "memberType": "KEYWORD", "keyword": k })),
    );
    members.extend(
        urls.iter()
            .map(|u| json!({ "memberType": "URL", "url": u })),
    );
    members.extend(
        apps.iter()
            .map(|a| json!({ "memberType": "APP", "app": a })),
    );

    let mut create = json!({
        "name": name,
        "type": segment_type.as_api_str(),
        "members": members,
    });
    if let Some(d) = description {
        create["description"] = json!(d);
    }

    let cid = GoogleAdsClient::normalize_customer_id(customer_id);

    let changes = json!({
        "name": name,
        "description": description,
        "segment_type": segment_type.as_api_str(),
        "keywords": keywords,
        "urls": urls,
        "apps": apps,
        "total_members": keywords.len() + urls.len() + apps.len(),
        "endpoint": "customAudiences:mutate",
        "note": "After apply, custom_segment_ids in the response holds the new segment ID — \
                 pass it to add_ad_group_custom_segments. A segment only narrows reach once an \
                 ad group targets it.",
    });

    let plan = ChangePlan::new(
        "create_custom_segment".to_string(),
        "custom_audience".to_string(),
        "new".to_string(),
        cid,
        changes,
        false,
        vec![json!({ "create": create })],
    )
    .with_dispatch(PlanDispatch::CustomAudiences);

    let preview = plan.to_preview();
    store_plan(plan);
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::preview::{get_plan, remove_plan};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn stored(preview: &serde_json::Value) -> ChangePlan {
        let plan_id = preview["plan_id"].as_str().expect("plan_id present");
        let plan = get_plan(plan_id).expect("plan stored");
        remove_plan(plan_id);
        plan
    }

    #[test]
    fn keyword_limits() {
        assert!(keyword_member(&"a".repeat(80)).is_ok());
        assert!(keyword_member(&"a".repeat(81)).is_err());
        assert!(keyword_member("one two three four five six seven eight nine ten").is_ok());
        assert!(keyword_member("one two three four five six seven eight nine ten eleven").is_err());
        assert!(keyword_member("   ").is_err());
    }

    #[test]
    fn url_needs_protocol() {
        assert_eq!(
            url_member(" https://www.example.org/en ").unwrap(),
            "https://www.example.org/en"
        );
        assert!(url_member("www.example.org").is_err());
        assert!(url_member("ftp://example.com").is_err());
        assert!(url_member("https://").is_err());
        assert!(url_member(&format!("https://example.com/{}", "a".repeat(2048))).is_err());
    }

    #[test]
    fn app_must_be_a_package_name() {
        assert_eq!(
            app_member("com.example.runner").unwrap(),
            "com.example.runner"
        );
        assert!(app_member("homeowner").is_err());
        assert!(app_member("com..x").is_err());
        assert!(app_member("com.1x").is_err());
        let prefixed = app_member("2-com.example.runner").unwrap_err();
        assert!(prefixed.to_string().contains("bare package name"));
    }

    #[test]
    fn builds_one_create_with_all_member_kinds() {
        let config = Config::default();
        let preview = create_custom_segment(
            &config,
            "123-456-7890",
            " Running shoe searchers ",
            Some("People searching for running shoes"),
            Some(CustomSegmentType::Search),
            &strings(&["running shoes", "Running shoes", "marathon training"]),
            &strings(&["https://www.example.com/running-shoes"]),
            &strings(&["com.example.runner"]),
        )
        .expect("ok");

        assert_eq!(preview["operation"], "create_custom_segment");
        assert_eq!(preview["entity_type"], "custom_audience");
        assert_eq!(preview["changes"]["total_members"], 4);
        assert_eq!(preview["changes"]["segment_type"], "SEARCH");

        let plan = stored(&preview);
        assert_eq!(plan.dispatch, PlanDispatch::CustomAudiences);
        assert_eq!(plan.mutate_operations.len(), 1);
        let create = &plan.mutate_operations[0]["create"];
        assert_eq!(create["name"], "Running shoe searchers");
        assert_eq!(create["type"], "SEARCH");
        assert_eq!(create["description"], "People searching for running shoes");
        let members = create["members"].as_array().expect("members");
        assert_eq!(members.len(), 4, "case-variant keyword collapsed");
        assert_eq!(
            members[0],
            json!({"memberType": "KEYWORD", "keyword": "running shoes"})
        );
        assert_eq!(
            members[2],
            json!({"memberType": "URL", "url": "https://www.example.com/running-shoes"})
        );
        assert_eq!(
            members[3],
            json!({"memberType": "APP", "app": "com.example.runner"})
        );
        // Status is output only; it is never sent.
        assert!(create.get("status").is_none());
        crate::client::GoogleAdsClient::validate_custom_audience_operations(
            &plan.mutate_operations,
        )
        .expect("bare CustomAudienceOperation");
    }

    #[test]
    fn defaults_to_auto_and_omits_empty_description() {
        let config = Config::default();
        let preview = create_custom_segment(
            &config,
            "1234567890",
            "Segment",
            Some("  "),
            None,
            &strings(&["running shoes"]),
            &[],
            &[],
        )
        .expect("ok");
        let plan = stored(&preview);
        let create = &plan.mutate_operations[0]["create"];
        assert_eq!(create["type"], "AUTO");
        assert!(create.get("description").is_none());
    }

    #[test]
    fn refuses_missing_name_members_and_blocked_operation() {
        let config = Config::default();
        let no_name =
            create_custom_segment(&config, "1", " ", None, None, &strings(&["x"]), &[], &[]);
        assert!(no_name.unwrap_err().to_string().contains("name"));

        let no_members = create_custom_segment(&config, "1", "S", None, None, &[], &[], &[]);
        assert!(no_members
            .unwrap_err()
            .to_string()
            .contains("at least one member"));

        let mut blocked = Config::default();
        blocked.safety.blocked_operations = vec!["create_custom_segment".to_string()];
        let err = create_custom_segment(&blocked, "1", "S", None, None, &strings(&["x"]), &[], &[]);
        assert!(err.unwrap_err().to_string().contains("blocked"));
    }
}
