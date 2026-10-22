use std::collections::HashMap;
use std::sync::LazyLock;

/// Extract field names from the SELECT clause of a GAQL query.
///
/// Example: "SELECT campaign.id, campaign.name FROM campaign" -> ["campaign.id", "campaign.name"]
pub fn parse_select_fields(query: &str) -> Vec<String> {
    let upper = query.to_uppercase();
    let select_pos = match upper.find("SELECT") {
        Some(pos) => pos + 6,
        None => return Vec::new(),
    };

    let from_pos = upper.find("FROM").unwrap_or(query.len());
    let fields_str = &query[select_pos..from_pos];

    fields_str
        .split(',')
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect()
}

/// Convert one snake_case GAQL field segment to the camelCase key the Google Ads
/// API uses in its JSON payloads: `cost_micros` -> `costMicros`.
fn snake_to_camel(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut capitalize_next = false;

    for ch in segment.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(ch);
        }
    }

    out
}

/// Resolve a dotted field path in a JSON value.
/// For example, "campaign.name" in {"campaign": {"name": "My Campaign"}} -> "My Campaign"
///
/// GAQL SELECT clauses are written in snake_case (`metrics.cost_micros`) while the
/// API answers in camelCase (`{"metrics": {"costMicros": …}}`), so each segment is
/// looked up literally first and then in its camelCase form. Without the fallback
/// every field with a multi-word segment resolved to an empty string, and the table
/// and CSV renderers printed blank cells with no error.
fn resolve_field(row: &serde_json::Value, field: &str) -> String {
    let parts: Vec<&str> = field.split('.').collect();
    let mut current = row;

    for part in &parts {
        current = match current.get(part) {
            Some(v) => v,
            None => {
                let camel = snake_to_camel(part);
                match current.get(camel.as_str()) {
                    Some(v) => v,
                    None => return String::new(),
                }
            }
        };
    }

    match current {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Format query results as an aligned text table.
pub fn format_table(rows: &[serde_json::Value], fields: &[String]) -> String {
    if rows.is_empty() {
        return "No results found.".to_string();
    }

    // Calculate column widths
    let mut widths: Vec<usize> = fields.iter().map(|f| f.len()).collect();

    let cell_values: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            fields
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let val = resolve_field(row, field);
                    if val.len() > widths[i] {
                        widths[i] = val.len();
                    }
                    val
                })
                .collect()
        })
        .collect();

    let mut output = String::new();

    // Header
    let header: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{:<width$}", f, width = widths[i]))
        .collect();
    output.push_str(&header.join(" | "));
    output.push('\n');

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    output.push_str(&sep.join("-+-"));
    output.push('\n');

    // Rows
    for cells in &cell_values {
        let row: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect();
        output.push_str(&row.join(" | "));
        output.push('\n');
    }

    output
}

/// Format query results as CSV.
pub fn format_csv(rows: &[serde_json::Value], fields: &[String]) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&fields.join(","));
    output.push('\n');

    // Rows
    for row in rows {
        let values: Vec<String> = fields
            .iter()
            .map(|field| {
                let val = resolve_field(row, field);
                if val.contains(',') || val.contains('"') || val.contains('\n') {
                    format!("\"{}\"", val.replace('"', "\"\""))
                } else {
                    val
                }
            })
            .collect();
        output.push_str(&values.join(","));
        output.push('\n');
    }

    output
}

/// Convert cost_micros fields to human-readable currency values.
/// Google Ads returns costs in micros (1/1,000,000 of the currency unit).
pub fn enrich_cost_fields(rows: &mut [serde_json::Value]) {
    for row in rows.iter_mut() {
        enrich_cost_recursive(row);
    }
}

fn enrich_cost_recursive(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            let mut additions: Vec<(String, serde_json::Value)> = Vec::new();

            for key in &keys {
                if key.ends_with("_micros") || key.ends_with("Micros") {
                    if let Some(micros_val) = map.get(key) {
                        let micros = match micros_val {
                            serde_json::Value::Number(n) => n.as_f64(),
                            serde_json::Value::String(s) => s.parse::<f64>().ok(),
                            _ => None,
                        };

                        if let Some(m) = micros {
                            let human_key = key.replace("_micros", "").replace("Micros", "");
                            let human_value = m / 1_000_000.0;
                            additions.push((
                                format!("{}_readable", human_key),
                                serde_json::Value::String(format!("{:.2}", human_value)),
                            ));
                        }
                    }
                }
            }

            for (k, v) in additions {
                map.insert(k, v);
            }

            // Recurse into nested objects
            for key in keys {
                if let Some(v) = map.get_mut(&key) {
                    enrich_cost_recursive(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                enrich_cost_recursive(item);
            }
        }
        _ => {}
    }
}

/// Build a GAQL date WHERE fragment.
///
/// Example: date_clause("2024-01-01", "2024-01-31")
///   -> "segments.date BETWEEN '2024-01-01' AND '2024-01-31'"
pub fn date_clause(start: &str, end: &str) -> String {
    format!("segments.date BETWEEN '{}' AND '{}'", start, end)
}

/// `campaign.start_date`/`campaign.end_date`, removed from the Google Ads API in
/// v23 in favor of `campaign.start_date_time`/`campaign.end_date_time` (a
/// datetime, not a bare date). Confirmed live against v25 (prod, 2026-10-23):
/// `run_gaql` with the old field names fails with a raw `UNRECOGNIZED_FIELD`,
/// and nothing points an agent reaching for the long-documented old name
/// toward the replacement. `run_gaql` exists so an agent can write arbitrary
/// GAQL without the server building it for them, so this rewrites the query
/// transparently rather than only hinting at the fix.
const DEPRECATED_DATE_FIELDS: &[(&str, &str)] = &[
    ("campaign.start_date", "campaign.start_date_time"),
    ("campaign.end_date", "campaign.end_date_time"),
];

/// Rewrite any `DEPRECATED_DATE_FIELDS` entry found among the query's exact
/// SELECT fields to its v23+ replacement, returning the rewritten query and
/// the (deprecated, replacement) pairs actually substituted (empty if none
/// applied). Matches on the field exactly as parsed by `parse_select_fields`
/// so `campaign.start_date_time` itself is never touched.
pub fn rewrite_deprecated_date_fields(query: &str) -> (String, Vec<(&'static str, &'static str)>) {
    let mut rewritten = query.to_string();
    let mut applied = Vec::new();

    for field in parse_select_fields(query) {
        for &(deprecated, replacement) in DEPRECATED_DATE_FIELDS {
            if field == deprecated {
                rewritten = rewritten.replacen(deprecated, replacement, 1);
                applied.push((deprecated, replacement));
            }
        }
    }

    (rewritten, applied)
}

/// Write a value at a dotted field path, creating intermediate objects as
/// needed. Inverse of `resolve_field`, used to alias a rewritten field's
/// result back under the field name the caller originally wrote.
fn set_field(row: &mut serde_json::Value, field: &str, value: String) {
    let parts: Vec<&str> = field.split('.').collect();
    let mut current = row;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.insert((*part).to_string(), serde_json::Value::String(value));
            }
            return;
        }

        if !current.is_object() {
            return;
        }
        current = current
            .as_object_mut()
            .expect("checked is_object above")
            .entry((*part).to_string())
            .or_insert_with(|| serde_json::json!({}));
    }
}

/// After `rewrite_deprecated_date_fields` substituted `actual_field` for
/// `deprecated_field` in the query sent to the API, inject the result back
/// into each row under `deprecated_field`'s own resolution path — truncated to
/// its date portion, since the deprecated field was a bare date, not a
/// timestamp. A row where `actual_field` is absent (e.g. a campaign with no
/// end date) is left untouched rather than aliased to an empty value.
pub fn alias_date_time_as_date(
    rows: &mut [serde_json::Value],
    deprecated_field: &str,
    actual_field: &str,
) {
    for row in rows.iter_mut() {
        let value = resolve_field(row, actual_field);
        if value.is_empty() {
            continue;
        }
        let date_only = value.split(' ').next().unwrap_or(&value).to_string();
        set_field(row, deprecated_field, date_only);
    }
}

/// Common GAQL error hints mapping error substrings to human-readable suggestions.
static ERROR_HINTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        "UNRECOGNIZED_FIELD",
        "The field name is not recognized. Check the Google Ads API field reference.",
    );
    m.insert(
        "INVALID_FIELD_IN_SELECT",
        "This field cannot be used in SELECT with the given FROM resource.",
    );
    m.insert(
        "INVALID_FIELD_IN_WHERE",
        "This field cannot be used in WHERE clause with the given FROM resource.",
    );
    m.insert(
        "INVALID_FIELD_IN_ORDER_BY",
        "This field cannot be used in ORDER BY with the given FROM resource.",
    );
    m.insert(
        "PROHIBITED_RESOURCE_TYPE_IN_FROM_CLAUSE",
        "This resource type cannot be used in FROM clause directly.",
    );
    m.insert(
        "PROHIBITED_METRIC_IN_SELECT_OR_WHERE_CLAUSE",
        "This metric cannot be used with the selected date range or segmentation.",
    );
    m.insert(
        "PROHIBITED_SEGMENT_IN_SELECT_OR_WHERE_CLAUSE",
        "This segment conflicts with other selected fields.",
    );
    m.insert(
        "MUTUALLY_EXCLUSIVE_FIELDS",
        "Two or more selected fields cannot be used together.",
    );
    m.insert(
        "DATE_RANGE_TOO_WIDE",
        "The date range is too wide for the requested metrics. Try narrowing the date range.",
    );
    m.insert(
        "AUTHORIZATION_ERROR",
        "Check your developer token, customer ID, and login customer ID configuration.",
    );
    // Google rejects a bad structured snippet header with a bare
    // `stringFormatError: INVALID_FORMAT` and never names the valid values.
    // Keyed on the field path, which only appears for this asset type.
    m.insert(
        "structured_snippet_asset",
        "Structured snippet headers are language-specific and must match Google's \
         predefined list for the ACCOUNT'S language, accents included — e.g. \
         'Serviços' on a pt-BR account, 'Service catalog' on an en account. Note the \
         sets are not literal translations: 'Neighborhoods' is valid but 'Bairros' is not.",
    );
    m
});

/// Look up an error hint for a given GAQL error message.
pub fn get_error_hint(error_message: &str) -> Option<&'static str> {
    ERROR_HINTS
        .iter()
        .find(|(key, _)| error_message.contains(*key))
        .map(|(_, hint)| *hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select_fields() {
        let query =
            "SELECT campaign.id, campaign.name FROM campaign WHERE campaign.status = 'ENABLED'";
        let fields = parse_select_fields(query);
        assert_eq!(fields, vec!["campaign.id", "campaign.name"]);
    }

    #[test]
    fn test_parse_select_fields_no_from() {
        let fields = parse_select_fields("SELECT campaign.id, campaign.name");
        assert_eq!(fields, vec!["campaign.id", "campaign.name"]);
    }

    #[test]
    fn test_parse_select_fields_empty() {
        let fields = parse_select_fields("FROM campaign");
        assert!(fields.is_empty());
    }

    #[test]
    fn test_format_table() {
        let rows = vec![
            serde_json::json!({"campaign": {"id": "123", "name": "Test"}}),
            serde_json::json!({"campaign": {"id": "456", "name": "Another"}}),
        ];
        let fields = vec!["campaign.id".to_string(), "campaign.name".to_string()];
        let table = format_table(&rows, &fields);
        assert!(table.contains("campaign.id"));
        assert!(table.contains("123"));
        assert!(table.contains("Another"));
    }

    #[test]
    fn test_format_table_empty() {
        let table = format_table(&[], &["field".to_string()]);
        assert_eq!(table, "No results found.");
    }

    #[test]
    fn test_format_csv() {
        let rows = vec![serde_json::json!({"campaign": {"id": "123", "name": "Test"}})];
        let fields = vec!["campaign.id".to_string(), "campaign.name".to_string()];
        let csv = format_csv(&rows, &fields);
        assert!(csv.starts_with("campaign.id,campaign.name\n"));
        assert!(csv.contains("123,Test"));
    }

    #[test]
    fn test_format_csv_escaping() {
        let rows = vec![serde_json::json!({"name": "Hello, World"})];
        let fields = vec!["name".to_string()];
        let csv = format_csv(&rows, &fields);
        assert!(csv.contains("\"Hello, World\""));
    }

    #[test]
    fn test_enrich_cost_fields() {
        let mut rows = vec![serde_json::json!({"metrics": {"cost_micros": "1500000"}})];
        enrich_cost_fields(&mut rows);
        let cost = rows[0]["metrics"]["cost_readable"].as_str();
        assert_eq!(cost, Some("1.50"));
    }

    #[test]
    fn test_enrich_cost_fields_numeric() {
        let mut rows = vec![serde_json::json!({"metrics": {"cost_micros": 2500000}})];
        enrich_cost_fields(&mut rows);
        let cost = rows[0]["metrics"]["cost_readable"].as_str();
        assert_eq!(cost, Some("2.50"));
    }

    #[test]
    fn test_date_clause() {
        let clause = date_clause("2024-01-01", "2024-01-31");
        assert_eq!(
            clause,
            "segments.date BETWEEN '2024-01-01' AND '2024-01-31'"
        );
    }

    #[test]
    fn test_get_error_hint() {
        let hint = get_error_hint("UNRECOGNIZED_FIELD in query");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("field name"));
    }

    #[test]
    fn test_get_error_hint_none() {
        let hint = get_error_hint("some random error");
        assert!(hint.is_none());
    }

    #[test]
    fn test_parse_select_fields_lowercase() {
        let query = "select campaign.id from campaign";
        let fields = parse_select_fields(query);
        assert_eq!(fields, vec!["campaign.id"]);
    }

    #[test]
    fn test_format_csv_with_newlines() {
        let rows = vec![serde_json::json!({"name": "Hello\nWorld"})];
        let fields = vec!["name".to_string()];
        let csv = format_csv(&rows, &fields);
        assert!(csv.contains("\"Hello\nWorld\""));
    }

    #[test]
    fn test_format_csv_with_quotes() {
        let rows = vec![serde_json::json!({"name": "Say \"hello\""})];
        let fields = vec!["name".to_string()];
        let csv = format_csv(&rows, &fields);
        // Double quotes should be escaped as ""
        assert!(csv.contains("\"Say \"\"hello\"\"\""));
    }

    /// A row shaped the way the Google Ads API actually returns it: GAQL SELECT
    /// fields are snake_case, the JSON payload is camelCase. Every field whose
    /// path has a multi-word segment must still resolve.
    #[test]
    fn test_resolve_field_accepts_camel_case_payload() {
        let row = serde_json::json!({
            "adGroupCriterion": {
                "keyword": {"text": "webhook api", "matchType": "PHRASE"},
                "qualityInfo": {"postClickQualityScore": "BELOW_AVERAGE"}
            },
            "metrics": {"costMicros": "177389137", "allConversions": 3}
        });

        assert_eq!(
            resolve_field(&row, "ad_group_criterion.keyword.text"),
            "webhook api"
        );
        assert_eq!(
            resolve_field(&row, "ad_group_criterion.keyword.match_type"),
            "PHRASE"
        );
        assert_eq!(
            resolve_field(
                &row,
                "ad_group_criterion.quality_info.post_click_quality_score"
            ),
            "BELOW_AVERAGE"
        );
        assert_eq!(resolve_field(&row, "metrics.cost_micros"), "177389137");
        assert_eq!(resolve_field(&row, "metrics.all_conversions"), "3");
    }

    /// snake_case payloads must keep resolving — the camelCase lookup is a
    /// fallback, not a replacement.
    #[test]
    fn test_resolve_field_still_accepts_snake_case_payload() {
        let row = serde_json::json!({"metrics": {"cost_micros": "42", "all_conversions": 7}});
        assert_eq!(resolve_field(&row, "metrics.cost_micros"), "42");
        assert_eq!(resolve_field(&row, "metrics.all_conversions"), "7");
    }

    #[test]
    fn test_resolve_field_absent_stays_empty() {
        let row = serde_json::json!({"metrics": {"costMicros": "42"}});
        assert_eq!(resolve_field(&row, "metrics.nope_not_here"), "");
        assert_eq!(resolve_field(&row, "campaign.name"), "");
    }

    /// The failure this guards against is silent: a change_event rendered as a
    /// table used to print blank rows, which reads as "no changes on the
    /// account" — the opposite of the truth — and that report is what decides
    /// whether an already-applied change gets applied twice.
    #[test]
    fn test_format_table_change_event_is_never_blank() {
        let rows = vec![serde_json::json!({
            "changeEvent": {
                "changeDateTime": "2026-08-17 05:23:04.551871",
                "changeResourceType": "CAMPAIGN_CRITERION",
                "resourceChangeOperation": "CREATE",
                "userEmail": "someone@example.com"
            }
        })];
        let fields = vec![
            "change_event.change_date_time".to_string(),
            "change_event.change_resource_type".to_string(),
            "change_event.resource_change_operation".to_string(),
            "change_event.user_email".to_string(),
        ];

        let table = format_table(&rows, &fields);
        assert!(table.contains("2026-08-17"), "date missing:\n{table}");
        assert!(
            table.contains("CAMPAIGN_CRITERION"),
            "resource type missing:\n{table}"
        );
        assert!(table.contains("CREATE"), "operation missing:\n{table}");
        assert!(
            table.contains("someone@example.com"),
            "user email missing:\n{table}"
        );
    }

    #[test]
    fn test_format_csv_camel_case_payload() {
        let rows = vec![serde_json::json!({
            "searchTermView": {"searchTerm": "webhook postman"},
            "metrics": {"costMicros": "2790000"}
        })];
        let fields = vec![
            "search_term_view.search_term".to_string(),
            "metrics.cost_micros".to_string(),
        ];

        let csv = format_csv(&rows, &fields);
        assert!(
            csv.contains("webhook postman"),
            "search term missing:\n{csv}"
        );
        assert!(csv.contains("2790000"), "cost missing:\n{csv}");
    }

    #[test]
    fn test_snake_to_camel() {
        assert_eq!(snake_to_camel("cost_micros"), "costMicros");
        assert_eq!(snake_to_camel("ad_group_criterion"), "adGroupCriterion");
        assert_eq!(snake_to_camel("name"), "name");
        assert_eq!(snake_to_camel(""), "");
        // A trailing underscore must not panic nor swallow characters.
        assert_eq!(snake_to_camel("trailing_"), "trailing");
        assert_eq!(snake_to_camel("_leading"), "Leading");
    }

    #[test]
    fn test_rewrite_deprecated_date_fields_rewrites_both() {
        let (rewritten, applied) = rewrite_deprecated_date_fields(
            "SELECT campaign.id, campaign.start_date, campaign.end_date FROM campaign",
        );
        assert_eq!(
            rewritten,
            "SELECT campaign.id, campaign.start_date_time, campaign.end_date_time FROM campaign"
        );
        assert_eq!(
            applied,
            vec![
                ("campaign.start_date", "campaign.start_date_time"),
                ("campaign.end_date", "campaign.end_date_time"),
            ]
        );
    }

    #[test]
    fn test_rewrite_deprecated_date_fields_leaves_already_correct_query_untouched() {
        let query =
            "SELECT campaign.id, campaign.start_date_time, campaign.end_date_time FROM campaign";
        let (rewritten, applied) = rewrite_deprecated_date_fields(query);
        // The critical case: a naive substring replace would corrupt
        // `start_date_time` into `start_date_time_time`.
        assert_eq!(rewritten, query);
        assert!(applied.is_empty());
    }

    #[test]
    fn test_rewrite_deprecated_date_fields_no_match() {
        let query = "SELECT campaign.id, campaign.name FROM campaign";
        let (rewritten, applied) = rewrite_deprecated_date_fields(query);
        assert_eq!(rewritten, query);
        assert!(applied.is_empty());
    }

    #[test]
    fn test_alias_date_time_as_date_truncates_to_date_portion() {
        let mut rows = vec![serde_json::json!({
            "campaign": {"id": "1", "startDateTime": "2026-10-09 12:57:49"}
        })];
        alias_date_time_as_date(&mut rows, "campaign.start_date", "campaign.start_date_time");
        assert_eq!(resolve_field(&rows[0], "campaign.start_date"), "2026-10-09");
    }

    #[test]
    fn test_alias_date_time_as_date_skips_absent_field() {
        // A campaign with no end date, as Google Ads actually returns it: the
        // field is absent from the row entirely, not null.
        let mut rows = vec![serde_json::json!({
            "campaign": {"id": "1", "startDateTime": "2026-10-09 12:57:49"}
        })];
        alias_date_time_as_date(&mut rows, "campaign.end_date", "campaign.end_date_time");
        assert_eq!(resolve_field(&rows[0], "campaign.end_date"), "");
    }
}
