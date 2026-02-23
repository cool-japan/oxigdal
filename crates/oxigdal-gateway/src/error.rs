//! Error types for the OxiGDAL API Gateway.
//!
//! This module provides comprehensive error handling for gateway operations including
//! rate limiting, authentication, GraphQL, WebSocket, and middleware errors.

/// Result type for gateway operations.
pub type Result<T> = std::result::Result<T, GatewayError>;

/// Comprehensive error types for API gateway operations.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// Rate limit exceeded error.
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded {
        /// Error message
        message: String,
        /// Retry after duration in seconds
        retry_after: Option<u64>,
    },

    /// Authentication error.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Authorization error.
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),

    /// Invalid API key error.
    #[error("Invalid API key")]
    InvalidApiKey,

    /// Expired token error.
    #[error("Token expired")]
    TokenExpired,

    /// Invalid token error.
    #[error("Invalid token: {0}")]
    InvalidToken(String),

    /// JWT error.
    #[error("JWT error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),

    /// OAuth2 error.
    #[error("OAuth2 error: {0}")]
    OAuth2Error(String),

    /// GraphQL error.
    #[error("GraphQL error: {0}")]
    GraphQLError(String),

    /// WebSocket error.
    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    /// Invalid request error.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Unsupported API version error.
    #[error("Unsupported API version: {version}")]
    UnsupportedVersion {
        /// Requested version
        version: String,
        /// Supported versions
        supported: Vec<String>,
    },

    /// Transformation error.
    #[error("Transformation error: {0}")]
    TransformationError(String),

    /// Schema validation error.
    #[error("Schema validation error: {0}")]
    SchemaValidationError(String),

    /// Load balancer error.
    #[error("Load balancer error: {0}")]
    LoadBalancerError(String),

    /// Backend unavailable error.
    #[error("Backend unavailable: {0}")]
    BackendUnavailable(String),

    /// Circuit breaker open error.
    #[error("Circuit breaker open for backend: {0}")]
    CircuitBreakerOpen(String),

    /// Timeout error.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Redis connection error.
    #[cfg(feature = "redis")]
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// HTTP error.
    #[error("HTTP error: {0}")]
    HttpError(String),

    /// Internal server error.
    #[error("Internal server error: {0}")]
    InternalError(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

impl GatewayError {
    /// Returns HTTP status code for this error.
    pub fn status_code(&self) -> http::StatusCode {
        match self {
            Self::RateLimitExceeded { .. } => http::StatusCode::TOO_MANY_REQUESTS,
            Self::AuthenticationFailed(_) | Self::InvalidApiKey | Self::InvalidToken(_) => {
                http::StatusCode::UNAUTHORIZED
            }
            Self::AuthorizationFailed(_) => http::StatusCode::FORBIDDEN,
            Self::TokenExpired => http::StatusCode::UNAUTHORIZED,
            Self::InvalidRequest(_) | Self::SchemaValidationError(_) => {
                http::StatusCode::BAD_REQUEST
            }
            Self::UnsupportedVersion { .. } => http::StatusCode::NOT_ACCEPTABLE,
            Self::BackendUnavailable(_) | Self::CircuitBreakerOpen(_) => {
                http::StatusCode::SERVICE_UNAVAILABLE
            }
            Self::Timeout(_) => http::StatusCode::GATEWAY_TIMEOUT,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Returns whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimitExceeded { .. }
                | Self::BackendUnavailable(_)
                | Self::CircuitBreakerOpen(_)
                | Self::Timeout(_)
                | Self::LoadBalancerError(_)
        )
    }

    /// Returns retry-after duration in seconds if applicable.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimitExceeded { retry_after, .. } => *retry_after,
            Self::BackendUnavailable(_) => Some(5),
            Self::CircuitBreakerOpen(_) => Some(30),
            _ => None,
        }
    }

    /// Converts error to JSON error response.
    pub fn to_json_response(&self) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "code": self.error_code(),
                "message": self.to_string(),
                "status": self.status_code().as_u16(),
                "retryable": self.is_retryable(),
                "retry_after": self.retry_after(),
            }
        })
    }

    /// Returns error code string.
    pub fn error_code(&self) -> &str {
        match self {
            Self::RateLimitExceeded { .. } => "RATE_LIMIT_EXCEEDED",
            Self::AuthenticationFailed(_) => "AUTHENTICATION_FAILED",
            Self::AuthorizationFailed(_) => "AUTHORIZATION_FAILED",
            Self::InvalidApiKey => "INVALID_API_KEY",
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::InvalidToken(_) => "INVALID_TOKEN",
            Self::JwtError(_) => "JWT_ERROR",
            Self::OAuth2Error(_) => "OAUTH2_ERROR",
            Self::GraphQLError(_) => "GRAPHQL_ERROR",
            Self::WebSocketError(_) => "WEBSOCKET_ERROR",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::UnsupportedVersion { .. } => "UNSUPPORTED_VERSION",
            Self::TransformationError(_) => "TRANSFORMATION_ERROR",
            Self::SchemaValidationError(_) => "SCHEMA_VALIDATION_ERROR",
            Self::LoadBalancerError(_) => "LOAD_BALANCER_ERROR",
            Self::BackendUnavailable(_) => "BACKEND_UNAVAILABLE",
            Self::CircuitBreakerOpen(_) => "CIRCUIT_BREAKER_OPEN",
            Self::Timeout(_) => "TIMEOUT",
            #[cfg(feature = "redis")]
            Self::RedisError(_) => "REDIS_ERROR",
            Self::SerializationError(_) => "SERIALIZATION_ERROR",
            Self::HttpError(_) => "HTTP_ERROR",
            Self::InternalError(_) => "INTERNAL_ERROR",
            Self::ConfigError(_) => "CONFIG_ERROR",
        }
    }
}

impl From<GatewayError> for http::StatusCode {
    fn from(error: GatewayError) -> Self {
        error.status_code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_codes() {
        assert_eq!(
            GatewayError::RateLimitExceeded {
                message: "test".to_string(),
                retry_after: Some(60)
            }
            .status_code(),
            http::StatusCode::TOO_MANY_REQUESTS
        );

        assert_eq!(
            GatewayError::AuthenticationFailed("test".to_string()).status_code(),
            http::StatusCode::UNAUTHORIZED
        );

        assert_eq!(
            GatewayError::AuthorizationFailed("test".to_string()).status_code(),
            http::StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn test_error_retryable() {
        assert!(
            GatewayError::RateLimitExceeded {
                message: "test".to_string(),
                retry_after: Some(60)
            }
            .is_retryable()
        );

        assert!(GatewayError::BackendUnavailable("test".to_string()).is_retryable());

        assert!(!GatewayError::InvalidApiKey.is_retryable());
    }

    #[test]
    fn test_retry_after() {
        let error = GatewayError::RateLimitExceeded {
            message: "test".to_string(),
            retry_after: Some(60),
        };
        assert_eq!(error.retry_after(), Some(60));

        assert_eq!(
            GatewayError::BackendUnavailable("test".to_string()).retry_after(),
            Some(5)
        );
    }

    #[test]
    fn test_json_response() {
        let error = GatewayError::InvalidApiKey;
        let json = error.to_json_response();

        assert_eq!(json["error"]["code"], "INVALID_API_KEY");
        assert_eq!(json["error"]["status"], 401);
        assert_eq!(json["error"]["retryable"], false);
    }
}
