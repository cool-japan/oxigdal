//! Error types for PWA operations.

use thiserror::Error;
use wasm_bindgen::{JsCast, JsValue};

/// Result type for PWA operations.
pub type Result<T> = std::result::Result<T, PwaError>;

/// Errors that can occur during PWA operations.
#[derive(Error, Debug)]
pub enum PwaError {
    /// Service worker registration failed
    #[error("Service worker registration failed: {0}")]
    ServiceWorkerRegistration(String),

    /// Service worker not supported
    #[error("Service workers are not supported in this browser")]
    ServiceWorkerNotSupported,

    /// Cache operation failed
    #[error("Cache operation failed: {0}")]
    CacheOperation(String),

    /// Cache not found
    #[error("Cache not found: {0}")]
    CacheNotFound(String),

    /// Request not in cache
    #[error("Request not in cache: {0}")]
    CacheRequestNotFound(String),

    /// Fetch failed
    #[error("Fetch failed: {0}")]
    FetchFailed(String),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Background sync registration failed
    #[error("Background sync registration failed: {0}")]
    BackgroundSyncRegistration(String),

    /// Background sync not supported
    #[error("Background sync is not supported in this browser")]
    BackgroundSyncNotSupported,

    /// Push notification subscription failed
    #[error("Push notification subscription failed: {0}")]
    PushSubscriptionFailed(String),

    /// Push notifications not supported
    #[error("Push notifications are not supported in this browser")]
    PushNotSupported,

    /// Notifications not supported
    #[error("Notifications are not supported in this browser")]
    NotificationsNotSupported,

    /// Notification permission denied
    #[error("Notification permission was denied")]
    NotificationPermissionDenied,

    /// Permission denied
    #[error("Permission was denied")]
    PermissionDenied,

    /// Permission request failed
    #[error("Permission request failed: {0}")]
    PermissionRequest(String),

    /// Notification display failed
    #[error("Failed to display notification: {0}")]
    NotificationDisplayFailed(String),

    /// Notification failed
    #[error("Notification failed: {0}")]
    NotificationFailed(String),

    /// Manifest generation failed
    #[error("Manifest generation failed: {0}")]
    ManifestGenerationFailed(String),

    /// Storage quota exceeded
    #[error("Storage quota exceeded")]
    QuotaExceeded,

    /// Storage estimate failed
    #[error("Failed to estimate storage: {0}")]
    StorageEstimateFailed(String),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Invalid cache strategy
    #[error("Invalid cache strategy: {0}")]
    InvalidCacheStrategy(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// JavaScript error
    #[error("JavaScript error: {0}")]
    JsError(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Lifecycle error
    #[error("PWA lifecycle error: {0}")]
    LifecycleError(String),

    /// Install prompt error
    #[error("Install prompt error: {0}")]
    InstallPromptError(String),
}

impl From<JsValue> for PwaError {
    fn from(value: JsValue) -> Self {
        if let Some(s) = value.as_string() {
            PwaError::JsError(s)
        } else if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
            PwaError::JsError(format!("{:?}", obj))
        } else {
            PwaError::JsError("Unknown JavaScript error".to_string())
        }
    }
}

impl From<serde_json::Error> for PwaError {
    fn from(err: serde_json::Error) -> Self {
        PwaError::Serialization(err.to_string())
    }
}

impl From<url::ParseError> for PwaError {
    fn from(err: url::ParseError) -> Self {
        PwaError::InvalidUrl(err.to_string())
    }
}

impl From<PwaError> for JsValue {
    fn from(err: PwaError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PwaError::ServiceWorkerNotSupported;
        assert_eq!(
            err.to_string(),
            "Service workers are not supported in this browser"
        );
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_error_from_js_string() {
        let js_val = JsValue::from_str("test error");
        let err = PwaError::from(js_val);
        assert!(matches!(err, PwaError::JsError(s) if s == "test error"));
    }

    #[test]
    fn test_error_chain() {
        let json_err = serde_json::from_str::<u32>("not a number");
        assert!(json_err.is_err());
        if let Err(e) = json_err {
            let pwa_err = PwaError::from(e);
            assert!(matches!(pwa_err, PwaError::Serialization(_)));
        }
    }
}
