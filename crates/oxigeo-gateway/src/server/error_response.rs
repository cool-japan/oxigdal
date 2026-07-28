//! Bridges the crate's [`GatewayError`] into an axum HTTP response.
//!
//! [`GatewayError`] is defined locally and already knows its own HTTP status code, JSON body
//! shape, and (for retryable errors) a `Retry-After` value; the only foreign piece is
//! `axum::response::IntoResponse`. Implementing it here lets every handler and middleware in the
//! serving layer simply `return err.into_response()` and get a consistent, structured error body:
//!
//! ```json
//! { "error": { "code", "message", "status", "retryable", "retry_after" } }
//! ```
//!
//! A `Retry-After` header (seconds) is attached whenever [`GatewayError::retry_after`] is `Some`
//! (rate limiting, backend unavailability, an open circuit breaker).

use axum::Json;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

use crate::error::GatewayError;

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        // Compute everything from `&self` before it is consumed by the response body builder.
        let status = self.status_code();
        let retry_after = self.retry_after();
        let body = Json(self.to_json_response());

        let mut response = (status, body).into_response();

        if let Some(seconds) = retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }

        response
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn rate_limit_carries_retry_after_and_status() {
        let error = GatewayError::RateLimitExceeded {
            message: "too many requests".to_string(),
            retry_after: Some(42),
        };
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok());
        assert_eq!(retry, Some("42"));
    }

    #[tokio::test]
    async fn authentication_failure_is_401_without_retry_after() {
        let response = GatewayError::AuthenticationFailed("bad token".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::RETRY_AFTER).is_none());
    }

    #[tokio::test]
    async fn authorization_failure_is_403() {
        let response = GatewayError::AuthorizationFailed("nope".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unsupported_version_is_406() {
        let response = GatewayError::UnsupportedVersion {
            version: "v9".to_string(),
            supported: vec!["v1".to_string(), "v2".to_string()],
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn body_has_structured_error_shape() {
        let response = GatewayError::InvalidApiKey.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body collects");
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"]["code"], "INVALID_API_KEY");
        assert_eq!(json["error"]["status"], 401);
        assert_eq!(json["error"]["retryable"], false);
    }
}
