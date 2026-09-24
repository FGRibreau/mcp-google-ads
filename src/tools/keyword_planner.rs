use crate::client::GoogleAdsClient;
use crate::error::{McpGoogleAdsError, Result};
use crate::tools::geo::resolve_country_geo_target;

/// English — the language Keyword Planner requests fall back to when the
/// caller names none, preserving the behaviour from before `language` was a
/// parameter.
const DEFAULT_LANGUAGE_CONSTANT: &str = "languageConstants/1000";

/// Resolve a language into a `languageConstants/{id}` resource name.
///
/// Accepts a language code ("en", "en_GB", "de") or a language name
/// ("English", "german"). Short inputs resolve via `language_constant.code`,
/// longer ones are matched against `language_constant.name`
/// case-insensitively, since GAQL string comparison is case-sensitive.
async fn resolve_language_constant(
    client: &GoogleAdsClient,
    customer_id: &str,
    language: &str,
) -> Result<String> {
    let input = language.trim();
    if input.is_empty() {
        return Err(McpGoogleAdsError::Validation(
            "language must not be empty".to_string(),
        ));
    }

    // Google's language codes are 2-5 characters ("en", "en_GB", "zh_CN");
    // anything longer is a name.
    let looks_like_code = input.len() <= 5
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let predicate = if looks_like_code {
        format!(
            "language_constant.code = '{}'",
            input.replace('-', "_").replace('\'', "\\'")
        )
    } else {
        format!(
            "language_constant.name LIKE '%{}%'",
            input.replace('\'', "\\'")
        )
    };

    let query = format!(
        "SELECT \
            language_constant.id, \
            language_constant.code, \
            language_constant.name \
        FROM language_constant \
        WHERE {}",
        predicate
    );

    let rows = client.search(customer_id, &query).await?;

    let candidates: Vec<&serde_json::Value> = rows
        .iter()
        .filter_map(|row| row.get("languageConstant"))
        .collect();

    let exact: Vec<&&serde_json::Value> = candidates
        .iter()
        .filter(|c| {
            c.get("name")
                .and_then(|n| n.as_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(input))
        })
        .collect();

    let chosen = match (exact.len(), candidates.len()) {
        (1, _) => *exact[0],
        (0, 1) => candidates[0],
        (_, 0) => {
            return Err(McpGoogleAdsError::Validation(format!(
                "Could not resolve language '{}' — pass a language code \
                 (e.g. 'en', 'de') or a language name (e.g. 'English').",
                language
            )));
        }
        _ => {
            let names: Vec<String> = candidates
                .iter()
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                .map(|n| n.to_string())
                .collect();
            return Err(McpGoogleAdsError::Validation(format!(
                "Language '{}' is ambiguous — it matches {}. \
                 Pass a language code or the exact language name.",
                language,
                names.join(", ")
            )));
        }
    };

    let id = chosen
        .get("id")
        .and_then(|id| {
            id.as_str()
                .map(|s| s.to_string())
                .or_else(|| id.as_u64().map(|n| n.to_string()))
        })
        .ok_or_else(|| {
            McpGoogleAdsError::Validation(format!(
                "Language '{}' resolved to a language constant with no id",
                language
            ))
        })?;

    Ok(format!("languageConstants/{}", id))
}

/// Discover keyword ideas from seed keywords using the Keyword Planner API.
///
/// `country` scopes the volume and competition figures to one market and
/// `language` to one language, both resolved against the live API. Google's
/// `KeywordPlanIdeaService.GenerateKeywordIdeas` has always accepted
/// `geo_target_constants` and `language`; without them the numbers returned
/// are worldwide, English-only totals, which is rarely what the caller means.
///
/// Omitting `country` leaves the request unscoped and omitting `language`
/// keeps the previous English default; neither costs a lookup round-trip.
pub async fn discover_keywords(
    client: &GoogleAdsClient,
    customer_id: &str,
    seed_keywords: Vec<String>,
    country: Option<&str>,
    language: Option<&str>,
) -> Result<String> {
    let geo_target_constants = match country {
        Some(c) => vec![resolve_country_geo_target(client, customer_id, c).await?],
        None => Vec::new(),
    };

    let language_constant = match language {
        Some(l) => resolve_language_constant(client, customer_id, l).await?,
        None => DEFAULT_LANGUAGE_CONSTANT.to_string(),
    };

    let results = client
        .generate_keyword_ideas(
            customer_id,
            seed_keywords,
            Some(50),
            &geo_target_constants,
            &language_constant,
        )
        .await?;

    let result = serde_json::json!({
        "keyword_ideas": results,
        "total_count": results.len(),
        "geo_target_constants": geo_target_constants,
        "language_constant": language_constant,
    });

    serde_json::to_string_pretty(&result).map_err(Into::into)
}

/// Get historical keyword performance metrics for forecasting.
pub async fn get_keyword_forecasts(
    client: &GoogleAdsClient,
    customer_id: &str,
    keyword_texts: Vec<String>,
) -> Result<String> {
    if keyword_texts.is_empty() {
        let result = serde_json::json!({
            "keyword_forecasts": [],
            "total_count": 0,
            "message": "No keywords provided."
        });
        return serde_json::to_string_pretty(&result).map_err(Into::into);
    }

    let escaped: Vec<String> = keyword_texts
        .iter()
        .map(|kw| format!("'{}'", kw.replace('\'', "\\'")))
        .collect();
    let in_clause = escaped.join(", ");

    let query = format!(
        "SELECT \
            ad_group_criterion.keyword.text, \
            metrics.average_cpc, \
            metrics.impressions, \
            metrics.clicks, \
            metrics.cost_micros, \
            metrics.average_cpm \
        FROM keyword_view \
        WHERE ad_group_criterion.keyword.text IN ({}) \
        AND segments.date DURING LAST_30_DAYS",
        in_clause
    );

    let rows = client.search(customer_id, &query).await?;

    let result = if rows.is_empty() {
        serde_json::json!({
            "keyword_forecasts": [],
            "total_count": 0,
            "message": "No matching keywords found in the account. These keywords may not exist in any active ad group."
        })
    } else {
        serde_json::json!({
            "keyword_forecasts": rows,
            "total_count": rows.len(),
        })
    };

    serde_json::to_string_pretty(&result).map_err(Into::into)
}
