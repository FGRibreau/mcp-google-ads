//! Policy-violation parsing and exemption handling for mutate operations.
//!
//! Google rejects some creates with `POLICY_ERROR`, carrying a
//! `policyViolationDetails` block that says which policy was hit, the exact
//! offending text, and whether the violation is *exemptible*. An exemptible
//! violation can be accepted by resubmitting the same operation with an
//! `exemptPolicyViolationKeys` entry — this is what the Google Ads web UI does
//! automatically, which is why keywords that fail through the API can be added
//! by hand in the UI.
//!
//! The most common case for medical advertisers is
//! `HEALTH_IN_PERSONALIZED_ADS`: virtually every gynecology/health keyword
//! trips it, and without an exemption the mutate fails with nothing but a
//! generic `400 Request contains an invalid argument`.
//!
//! Requesting an exemption is an assertion that the ad is eligible under the
//! policy's permitted use — Google still reviews it. It is therefore opt-in
//! (`confirm_and_apply(exempt_policy_violations=true)`), never automatic.

use serde_json::{json, Value};

/// Operation keys that accept `exemptPolicyViolationKeys`, verified against
/// the live v23 API:
///
/// - `adGroupCriterionOperation` — supported (keywords).
/// - `campaignCriterionOperation` — the field does not exist on this operation
///   ("Unknown name \"exemptPolicyViolationKeys\"").
/// - `adGroupAdOperation` — has `policyValidationParameter`, but responsive
///   search ads are rejected with
///   `UNSUPPORTED_AD_TYPE_FOR_EXEMPT_POLICY_VIOLATION_KEYS`, and RSAs are the
///   only ad type this server creates.
///
/// Keyword criteria are consequently the only operations we can exempt.
const EXEMPTABLE_OPERATION_KEYS: &[&str] = &["adGroupCriterionOperation"];

/// Maximum number of individual errors rendered into an error response before
/// truncating. A large batch can otherwise produce one entry per keyword.
const MAX_RENDERED_ERRORS: usize = 25;

/// A single policy violation reported against one mutate operation.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyViolation {
    /// Index into the plan's `mutate_operations`.
    pub operation_index: usize,
    /// e.g. `HEALTH_IN_PERSONALIZED_ADS`.
    pub policy_name: String,
    /// The exact text Google objected to; must be echoed back verbatim in the
    /// exemption key or the exemption is not matched.
    pub violating_text: String,
    /// Whether Google will accept an exemption request for this violation.
    pub is_exemptible: bool,
    /// Human-readable policy description, when provided.
    pub description: Option<String>,
}

impl PolicyViolation {
    /// The `PolicyViolationKey` to send back in `exemptPolicyViolationKeys`.
    fn key(&self) -> Value {
        json!({
            "policyName": self.policy_name,
            "violatingText": self.violating_text,
        })
    }
}

/// Result of attaching exemption keys to a set of operations.
#[derive(Debug, Clone)]
pub struct ExemptionPlan {
    /// Operations with exemption keys attached where supported.
    pub operations: Vec<Value>,
    /// Violations that were successfully attached to an operation.
    pub exempted: Vec<PolicyViolation>,
    /// Violations on operation types that cannot carry an exemption.
    pub unsupported: Vec<PolicyViolation>,
}

/// Walk the `errors` array of every `GoogleAdsFailure` detail block.
fn for_each_error(details: &[String], mut f: impl FnMut(&Value)) {
    for block in details {
        let Ok(parsed) = serde_json::from_str::<Value>(block) else {
            continue;
        };
        let Some(errors) = parsed.get("errors").and_then(|e| e.as_array()) else {
            continue;
        };
        for err in errors {
            f(err);
        }
    }
}

/// The `mutate_operations` index an error points at (absent index means 0,
/// since protobuf JSON omits zero-valued fields).
fn operation_index(err: &Value) -> usize {
    err.get("location")
        .and_then(|l| l.get("fieldPathElements"))
        .and_then(|p| p.as_array())
        .and_then(|elems| {
            elems
                .iter()
                .find(|e| e.get("fieldName").and_then(|f| f.as_str()) == Some("mutate_operations"))
        })
        .and_then(|e| e.get("index"))
        .and_then(|i| i.as_u64())
        .unwrap_or(0) as usize
}

/// Extract every policy violation from the `details` of a `GoogleAdsFailure`.
pub fn parse_violations(details: &[String]) -> Vec<PolicyViolation> {
    let mut out = Vec::new();
    for_each_error(details, |err| {
        let Some(pvd) = err
            .get("details")
            .and_then(|d| d.get("policyViolationDetails"))
        else {
            return;
        };
        let key = pvd.get("key");
        let policy_name = key
            .and_then(|k| k.get("policyName"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let violating_text = key
            .and_then(|k| k.get("violatingText"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if policy_name.is_empty() {
            return;
        }
        out.push(PolicyViolation {
            operation_index: operation_index(err),
            policy_name,
            violating_text,
            is_exemptible: pvd
                .get("isExemptible")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            description: pvd
                .get("externalPolicyDescription")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });
    });
    out
}

/// The top-level operation key of a mutate operation (e.g.
/// `adGroupCriterionOperation`).
fn operation_key(op: &Value) -> Option<&str> {
    op.as_object()?.keys().next().map(|k| k.as_str())
}

/// Attach `exemptPolicyViolationKeys` to each operation that has an exemptible
/// violation and supports exemptions.
///
/// Operations without violations are passed through untouched, so the returned
/// vector can be resubmitted as-is.
pub fn apply_exemptions(operations: &[Value], violations: &[PolicyViolation]) -> ExemptionPlan {
    let mut operations = operations.to_vec();
    let mut exempted = Vec::new();
    let mut unsupported = Vec::new();

    for violation in violations {
        let Some(op) = operations.get_mut(violation.operation_index) else {
            unsupported.push(violation.clone());
            continue;
        };
        let key = operation_key(op).map(|k| k.to_string());
        let supported = key
            .as_deref()
            .is_some_and(|k| EXEMPTABLE_OPERATION_KEYS.contains(&k));
        if !supported {
            unsupported.push(violation.clone());
            continue;
        }

        // Safe: `supported` implies the key exists and the value is an object.
        let Some(body) = key
            .and_then(|k| op.get_mut(k))
            .and_then(|v| v.as_object_mut())
        else {
            unsupported.push(violation.clone());
            continue;
        };

        let entry = body
            .entry("exemptPolicyViolationKeys")
            .or_insert_with(|| json!([]));
        if let Some(arr) = entry.as_array_mut() {
            let k = violation.key();
            if !arr.contains(&k) {
                arr.push(k);
            }
            exempted.push(violation.clone());
        } else {
            unsupported.push(violation.clone());
        }
    }

    ExemptionPlan {
        operations,
        exempted,
        unsupported,
    }
}

/// Compact, agent-readable rendering of policy violations.
pub fn violations_json(violations: &[PolicyViolation], supported_only: bool) -> Value {
    let items: Vec<Value> = violations
        .iter()
        .map(|v| {
            let mut o = json!({
                "operation_index": v.operation_index,
                "policy_name": v.policy_name,
                "violating_text": v.violating_text,
                "is_exemptible": v.is_exemptible,
            });
            if let Some(ref d) = v.description {
                o["description"] = json!(d);
            }
            if supported_only {
                o["exemptable_via_api"] = json!(true);
            }
            o
        })
        .collect();
    json!(items)
}

/// Compact summary of a `GoogleAdsFailure`, used to surface the real cause of a
/// mutate rejection instead of the generic top-level message.
pub fn summarize_failure(details: &[String]) -> Value {
    let mut items: Vec<Value> = Vec::new();
    let mut total = 0usize;

    for_each_error(details, |err| {
        total += 1;
        if items.len() >= MAX_RENDERED_ERRORS {
            return;
        }
        let code = err
            .get("errorCode")
            .and_then(|c| c.as_object())
            .and_then(|o| o.iter().next())
            .map(|(k, v)| format!("{}: {}", k, v.as_str().unwrap_or_default()));
        let field = err
            .get("location")
            .and_then(|l| l.get("fieldPathElements"))
            .and_then(|p| p.as_array())
            .map(|elems| {
                elems
                    .iter()
                    .filter_map(|e| e.get("fieldName").and_then(|f| f.as_str()))
                    .collect::<Vec<_>>()
                    .join(".")
            });

        let mut item = json!({ "operation_index": operation_index(err) });
        if let Some(c) = code {
            item["code"] = json!(c);
        }
        if let Some(m) = err.get("message").and_then(|m| m.as_str()) {
            item["message"] = json!(m);
        }
        if let Some(f) = field {
            item["field"] = json!(f);
        }
        if let Some(t) = err
            .get("trigger")
            .and_then(|t| t.get("stringValue"))
            .and_then(|v| v.as_str())
        {
            item["trigger"] = json!(t);
        }
        if let Some(pvd) = err
            .get("details")
            .and_then(|d| d.get("policyViolationDetails"))
        {
            item["policy"] = json!({
                "name": pvd.get("key").and_then(|k| k.get("policyName")),
                "violating_text": pvd.get("key").and_then(|k| k.get("violatingText")),
                "is_exemptible": pvd.get("isExemptible").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
        items.push(item);
    });

    let mut out = json!({ "errors": items });
    if total > items.len() {
        out["truncated"] = json!(total - items.len());
        out["total_errors"] = json!(total);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real shape of a HEALTH_IN_PERSONALIZED_ADS rejection (v23).
    fn health_failure(index: Option<u64>, text: &str, exemptible: bool) -> String {
        let mut path = json!({"fieldName": "mutate_operations"});
        if let Some(i) = index {
            path["index"] = json!(i);
        }
        json!({
            "@type": "type.googleapis.com/google.ads.googleads.v23.errors.GoogleAdsFailure",
            "errors": [{
                "errorCode": {"policyViolationError": "POLICY_ERROR"},
                "message": "A policy was violated. See PolicyViolationDetails for more detail.",
                "trigger": {"stringValue": text},
                "location": {"fieldPathElements": [
                    path,
                    {"fieldName": "ad_group_criterion_operation"},
                    {"fieldName": "create"},
                    {"fieldName": "keyword"},
                    {"fieldName": "text"}
                ]},
                "details": {"policyViolationDetails": {
                    "externalPolicyDescription": "Your ad violates the Personalized advertising policy.",
                    "externalPolicyName": "Health in personalized advertising",
                    "key": {"policyName": "HEALTH_IN_PERSONALIZED_ADS", "violatingText": text},
                    "isExemptible": exemptible
                }}
            }]
        })
        .to_string()
    }

    fn kw_op(text: &str) -> Value {
        json!({"adGroupCriterionOperation": {"create": {
            "adGroup": "customers/1/adGroups/2",
            "keyword": {"text": text, "matchType": "PHRASE"}
        }}})
    }

    #[test]
    fn parses_health_violation() {
        let v = parse_violations(&[health_failure(Some(3), "ginecologista sobral", true)]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].operation_index, 3);
        assert_eq!(v[0].policy_name, "HEALTH_IN_PERSONALIZED_ADS");
        assert_eq!(v[0].violating_text, "ginecologista sobral");
        assert!(v[0].is_exemptible);
    }

    #[test]
    fn missing_index_defaults_to_zero() {
        // protobuf JSON omits zero-valued fields, so index 0 arrives absent
        let v = parse_violations(&[health_failure(None, "papanicolau", true)]);
        assert_eq!(v[0].operation_index, 0);
    }

    #[test]
    fn non_policy_errors_are_ignored() {
        let body = json!({
            "errors": [{
                "errorCode": {"fieldError": "REQUIRED"},
                "message": "The required field was not present.",
                "location": {"fieldPathElements": [{"fieldName": "mutate_operations"}]}
            }]
        })
        .to_string();
        assert!(parse_violations(&[body]).is_empty());
    }

    #[test]
    fn attaches_exemption_key_to_the_right_operation() {
        let ops = vec![kw_op("consulta"), kw_op("ginecologista sobral")];
        let violations = parse_violations(&[health_failure(Some(1), "ginecologista sobral", true)]);
        let plan = apply_exemptions(&ops, &violations);

        assert_eq!(plan.exempted.len(), 1);
        assert!(plan.unsupported.is_empty());
        // untouched operation keeps its original shape
        assert!(plan.operations[0]["adGroupCriterionOperation"]
            .get("exemptPolicyViolationKeys")
            .is_none());
        assert_eq!(
            plan.operations[1]["adGroupCriterionOperation"]["exemptPolicyViolationKeys"],
            json!([{
                "policyName": "HEALTH_IN_PERSONALIZED_ADS",
                "violatingText": "ginecologista sobral"
            }])
        );
    }

    #[test]
    fn ad_operations_cannot_be_exempted() {
        let ops = vec![json!({"adGroupAdOperation": {"create": {"adGroup": "x"}}})];
        let violations = parse_violations(&[health_failure(Some(0), "diu", true)]);
        let plan = apply_exemptions(&ops, &violations);

        assert!(plan.exempted.is_empty());
        assert_eq!(plan.unsupported.len(), 1);
        // operation is left exactly as it was
        assert_eq!(plan.operations[0], ops[0]);
    }

    #[test]
    fn out_of_range_index_is_unsupported_not_a_panic() {
        let ops = vec![kw_op("consulta")];
        let violations = parse_violations(&[health_failure(Some(9), "x", true)]);
        let plan = apply_exemptions(&ops, &violations);
        assert_eq!(plan.unsupported.len(), 1);
        assert!(plan.exempted.is_empty());
    }

    #[test]
    fn multiple_violations_on_one_operation_are_deduped() {
        let ops = vec![kw_op("ginecologista")];
        let f = health_failure(Some(0), "ginecologista", true);
        let violations = parse_violations(&[f.clone(), f]);
        assert_eq!(violations.len(), 2);
        let plan = apply_exemptions(&ops, &violations);
        assert_eq!(
            plan.operations[0]["adGroupCriterionOperation"]["exemptPolicyViolationKeys"]
                .as_array()
                .map(|a| a.len()),
            Some(1)
        );
    }

    #[test]
    fn summarize_surfaces_policy_name_and_trigger() {
        let s = summarize_failure(&[health_failure(Some(0), "histeroscopia", true)]);
        let e = &s["errors"][0];
        assert_eq!(e["code"], "policyViolationError: POLICY_ERROR");
        assert_eq!(e["trigger"], "histeroscopia");
        assert_eq!(e["policy"]["name"], "HEALTH_IN_PERSONALIZED_ADS");
        assert_eq!(e["policy"]["is_exemptible"], true);
        assert!(e["field"].as_str().unwrap().contains("keyword.text"));
    }

    #[test]
    fn summarize_truncates_large_batches() {
        let blocks: Vec<String> = (0..40)
            .map(|i| health_failure(Some(i), &format!("kw {i}"), true))
            .collect();
        let s = summarize_failure(&blocks);
        assert_eq!(
            s["errors"].as_array().map(|a| a.len()),
            Some(MAX_RENDERED_ERRORS)
        );
        assert_eq!(s["total_errors"], 40);
        assert_eq!(s["truncated"], 40 - MAX_RENDERED_ERRORS);
    }

    #[test]
    fn malformed_detail_blocks_are_skipped() {
        assert!(parse_violations(&["not json".to_string()]).is_empty());
        assert!(parse_violations(&["{}".to_string()]).is_empty());
    }
}
