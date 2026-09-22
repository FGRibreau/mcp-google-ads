//! `X-API-Key` authentication guard for the `Http` transport.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Rejects any request that doesn't present the configured `X-API-Key`
/// header. When `expected` is `None` (no `GOOGLE_ADS_MCP_API_KEY` set), every
/// request passes through unauthenticated.
pub async fn check_api_key(expected: Option<String>, req: Request, next: Next) -> Response {
    let Some(expected) = expected else {
        return next.run(req).await;
    };

    let provided = req.headers().get("x-api-key").and_then(|v| v.to_str().ok());

    match provided {
        Some(provided) if constant_time_eq(provided, &expected) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            "missing or invalid X-API-Key header",
        )
            .into_response(),
    }
}

/// Compares two strings in time independent of where they first differ, so a
/// timing side-channel can't be used to guess the configured API key one byte
/// at a time.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn equal_strings_match() {
        assert!(constant_time_eq("secret", "secret"));
    }

    #[test]
    fn different_strings_do_not_match() {
        assert!(!constant_time_eq("secret", "wrong"));
    }

    #[test]
    fn different_lengths_do_not_match() {
        assert!(!constant_time_eq("secret", "secre"));
    }
}
