//! Push notification support for PWA.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Notification, NotificationOptions, NotificationPermission, PushManager, PushSubscription,
    ServiceWorkerRegistration,
};

/// Notification permission status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Permission {
    /// Permission granted
    Granted,

    /// Permission denied
    Denied,

    /// Permission not yet requested
    Default,
}

impl From<NotificationPermission> for Permission {
    fn from(perm: NotificationPermission) -> Self {
        match perm {
            NotificationPermission::Granted => Permission::Granted,
            NotificationPermission::Denied => Permission::Denied,
            NotificationPermission::Default => Permission::Default,
            _ => Permission::Default,
        }
    }
}

impl Permission {
    /// Check if permission is granted.
    pub fn is_granted(&self) -> bool {
        matches!(self, Permission::Granted)
    }

    /// Check if permission is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, Permission::Denied)
    }
}

/// Notification configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Notification title
    pub title: String,

    /// Notification body
    pub body: Option<String>,

    /// Icon URL
    pub icon: Option<String>,

    /// Badge URL
    pub badge: Option<String>,

    /// Tag for grouping notifications
    pub tag: Option<String>,

    /// Whether notification should be silent
    pub silent: bool,

    /// Require interaction to dismiss
    pub require_interaction: bool,

    /// Data to attach to notification
    pub data: Option<serde_json::Value>,

    /// Actions for the notification
    pub actions: Vec<NotificationAction>,
}

/// Notification action button.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationAction {
    /// Action identifier
    pub action: String,

    /// Action title
    pub title: String,

    /// Icon for the action
    pub icon: Option<String>,
}

impl NotificationConfig {
    /// Create a new notification configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: None,
            icon: None,
            badge: None,
            tag: None,
            silent: false,
            require_interaction: false,
            data: None,
            actions: Vec::new(),
        }
    }

    /// Set the body text.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the icon URL.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set the badge URL.
    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Set the tag for grouping.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Set whether the notification should be silent.
    pub fn with_silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }

    /// Set whether interaction is required.
    pub fn with_require_interaction(mut self, require: bool) -> Self {
        self.require_interaction = require;
        self
    }

    /// Set custom data.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Add an action button.
    pub fn add_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Convert to web_sys::NotificationOptions.
    pub fn to_options(&self) -> Result<NotificationOptions> {
        let options = NotificationOptions::new();

        if let Some(body) = &self.body {
            options.set_body(body);
        }

        if let Some(icon) = &self.icon {
            options.set_icon(icon);
        }

        if let Some(badge) = &self.badge {
            options.set_badge(badge);
        }

        if let Some(tag) = &self.tag {
            options.set_tag(tag);
        }

        options.set_silent(Some(self.silent));
        options.set_require_interaction(self.require_interaction);

        if let Some(data) = &self.data {
            let js_data = serde_wasm_bindgen::to_value(data)
                .map_err(|e| PwaError::Serialization(format!("{:?}", e)))?;
            options.set_data(&js_data);
        }

        Ok(options)
    }
}

impl NotificationAction {
    /// Create a new notification action.
    pub fn new(action: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            title: title.into(),
            icon: None,
        }
    }

    /// Set the icon URL.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

/// Notification manager for displaying notifications.
pub struct NotificationManager;

impl NotificationManager {
    /// Create a new notification manager.
    pub fn new() -> Self {
        Self
    }

    /// Check if notifications are supported.
    pub fn is_supported() -> bool {
        if let Some(window) = web_sys::window() {
            js_sys::Reflect::has(&window.navigator(), &JsValue::from_str("Notification"))
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Get current permission status.
    pub fn get_permission() -> Permission {
        if !Self::is_supported() {
            return Permission::Denied;
        }

        if let Some(window) = web_sys::window() {
            let notification_class =
                js_sys::Reflect::get(&window, &JsValue::from_str("Notification"))
                    .ok()
                    .and_then(|v| v.dyn_into::<js_sys::Function>().ok());

            if let Some(notification) = notification_class {
                if let Ok(permission) =
                    js_sys::Reflect::get(&notification, &JsValue::from_str("permission"))
                {
                    if let Some(perm_str) = permission.as_string() {
                        return match perm_str.as_str() {
                            "granted" => Permission::Granted,
                            "denied" => Permission::Denied,
                            _ => Permission::Default,
                        };
                    }
                }
            }
        }

        Permission::Default
    }

    /// Request notification permission.
    pub async fn request_permission() -> Result<Permission> {
        if !Self::is_supported() {
            return Err(PwaError::NotificationsNotSupported);
        }

        let promise = Notification::request_permission()
            .map_err(|e| PwaError::PermissionRequest(format!("{:?}", e)))?;

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::PermissionRequest(format!("Request failed: {:?}", e)))?;

        let permission_string = result
            .as_string()
            .ok_or_else(|| PwaError::PermissionRequest("Invalid permission result".to_string()))?;

        Ok(match permission_string.as_str() {
            "granted" => Permission::Granted,
            "denied" => Permission::Denied,
            _ => Permission::Default,
        })
    }

    /// Show a notification.
    pub async fn show(&self, config: &NotificationConfig) -> Result<Notification> {
        if !Self::is_supported() {
            return Err(PwaError::NotificationsNotSupported);
        }

        let permission = Self::get_permission();
        if !permission.is_granted() {
            return Err(PwaError::PermissionDenied);
        }

        let options = config.to_options()?;

        let notification = Notification::new_with_options(&config.title, &options)
            .map_err(|e| PwaError::NotificationFailed(format!("{:?}", e)))?;

        Ok(notification)
    }

    /// Show a notification with a service worker registration.
    pub async fn show_with_registration(
        registration: &ServiceWorkerRegistration,
        config: &NotificationConfig,
    ) -> Result<()> {
        if !Self::is_supported() {
            return Err(PwaError::NotificationsNotSupported);
        }

        let permission = Self::get_permission();
        if !permission.is_granted() {
            return Err(PwaError::PermissionDenied);
        }

        let options = config.to_options()?;

        let promise = registration
            .show_notification_with_options(&config.title, &options)
            .map_err(|e| {
                PwaError::NotificationFailed(format!("Show notification failed: {:?}", e))
            })?;

        JsFuture::from(promise).await.map_err(|e| {
            PwaError::NotificationFailed(format!("Show notification promise failed: {:?}", e))
        })?;

        Ok(())
    }

    /// Close a notification.
    pub fn close(&self, notification: &Notification) {
        notification.close();
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Push notification manager for handling push subscriptions.
pub struct PushNotificationManager {
    registration: ServiceWorkerRegistration,
}

impl PushNotificationManager {
    /// Create a new push notification manager.
    pub fn new(registration: ServiceWorkerRegistration) -> Self {
        Self { registration }
    }

    /// Check if push notifications are supported.
    pub fn is_supported() -> bool {
        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let sw_container = navigator.service_worker();
            js_sys::Reflect::has(&sw_container, &JsValue::from_str("pushManager")).unwrap_or(false)
        } else {
            false
        }
    }

    /// Get the push manager.
    fn get_push_manager(&self) -> Result<PushManager> {
        let push_manager = self
            .registration
            .push_manager()
            .map_err(|_e| PwaError::PushNotSupported)?;
        Ok(push_manager)
    }

    /// Subscribe to push notifications.
    pub async fn subscribe(&self, application_server_key: &[u8]) -> Result<PushSubscription> {
        if !Self::is_supported() {
            return Err(PwaError::PushNotSupported);
        }

        let push_manager = self.get_push_manager()?;

        // Create subscription options as a JS object
        let options = js_sys::Object::new();
        js_sys::Reflect::set(
            &options,
            &JsValue::from_str("userVisibleOnly"),
            &JsValue::from_bool(true),
        )
        .map_err(|e| {
            PwaError::PushSubscriptionFailed(format!("Failed to set userVisibleOnly: {:?}", e))
        })?;

        // Convert application server key to Uint8Array
        let key_array = js_sys::Uint8Array::from(application_server_key);
        js_sys::Reflect::set(
            &options,
            &JsValue::from_str("applicationServerKey"),
            &key_array,
        )
        .map_err(|e| {
            PwaError::PushSubscriptionFailed(format!("Failed to set applicationServerKey: {:?}", e))
        })?;

        // Use Reflect to call subscribe with options
        let push_manager_val: &JsValue = push_manager.as_ref();
        let subscribe_fn = js_sys::Reflect::get(push_manager_val, &JsValue::from_str("subscribe"))
            .map_err(|e| {
                PwaError::PushSubscriptionFailed(format!("Failed to get subscribe method: {:?}", e))
            })?;
        let subscribe_fn = subscribe_fn.dyn_into::<js_sys::Function>().map_err(|_| {
            PwaError::PushSubscriptionFailed("subscribe is not a function".to_string())
        })?;

        let args = js_sys::Array::new();
        args.push(&options);

        let promise = subscribe_fn
            .apply(push_manager_val, &args)
            .map_err(|e| {
                PwaError::PushSubscriptionFailed(format!("Subscribe call failed: {:?}", e))
            })?
            .dyn_into::<js_sys::Promise>()
            .map_err(|_| {
                PwaError::PushSubscriptionFailed("Subscribe did not return a Promise".to_string())
            })?;

        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::PushSubscriptionFailed(format!("Subscribe promise failed: {:?}", e))
        })?;

        result.dyn_into::<PushSubscription>().map_err(|_| {
            PwaError::PushSubscriptionFailed("Invalid subscription object".to_string())
        })
    }

    /// Get current push subscription.
    pub async fn get_subscription(&self) -> Result<Option<PushSubscription>> {
        if !Self::is_supported() {
            return Ok(None);
        }

        let push_manager = self.get_push_manager()?;
        let promise = push_manager.get_subscription().map_err(|e| {
            PwaError::PushSubscriptionFailed(format!("Get subscription failed: {:?}", e))
        })?;

        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::PushSubscriptionFailed(format!("Get subscription promise failed: {:?}", e))
        })?;

        if result.is_null() || result.is_undefined() {
            Ok(None)
        } else {
            let subscription = result.dyn_into::<PushSubscription>().map_err(|_| {
                PwaError::PushSubscriptionFailed("Invalid subscription object".to_string())
            })?;
            Ok(Some(subscription))
        }
    }

    /// Unsubscribe from push notifications.
    pub async fn unsubscribe(&self) -> Result<bool> {
        if let Some(subscription) = self.get_subscription().await? {
            let promise = subscription.unsubscribe().map_err(|e| {
                PwaError::PushSubscriptionFailed(format!("Unsubscribe failed: {:?}", e))
            })?;

            let result = JsFuture::from(promise).await.map_err(|e| {
                PwaError::PushSubscriptionFailed(format!("Unsubscribe promise failed: {:?}", e))
            })?;

            result.as_bool().ok_or_else(|| {
                PwaError::PushSubscriptionFailed("Invalid unsubscribe result".to_string())
            })
        } else {
            Ok(false)
        }
    }

    /// Get subscription as JSON.
    pub async fn get_subscription_json(&self) -> Result<Option<String>> {
        if let Some(subscription) = self.get_subscription().await? {
            // Use Reflect to call toJSON method
            let subscription_obj: &JsValue = subscription.as_ref();
            let to_json_fn =
                js_sys::Reflect::get(subscription_obj, &JsValue::from_str("toJSON"))
                    .map_err(|e| PwaError::Serialization(format!("toJSON not found: {:?}", e)))?;

            let to_json = to_json_fn
                .dyn_into::<js_sys::Function>()
                .map_err(|_| PwaError::Serialization("toJSON is not a function".to_string()))?;

            let json = to_json
                .call0(subscription_obj)
                .map_err(|e| PwaError::Serialization(format!("toJSON call failed: {:?}", e)))?;

            let json_str = js_sys::JSON::stringify(&json)
                .map_err(|e| PwaError::Serialization(format!("{:?}", e)))?
                .as_string()
                .ok_or_else(|| PwaError::Serialization("Invalid JSON string".to_string()))?;

            Ok(Some(json_str))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission() {
        let granted = Permission::Granted;
        assert!(granted.is_granted());
        assert!(!granted.is_denied());

        let denied = Permission::Denied;
        assert!(denied.is_denied());
        assert!(!denied.is_granted());
    }

    #[test]
    fn test_notification_config() {
        let config = NotificationConfig::new("Test")
            .with_body("Body")
            .with_icon("/icon.png")
            .with_silent(true);

        assert_eq!(config.title, "Test");
        assert_eq!(config.body, Some("Body".to_string()));
        assert_eq!(config.icon, Some("/icon.png".to_string()));
        assert!(config.silent);
    }

    #[test]
    fn test_notification_action() {
        let action = NotificationAction::new("view", "View").with_icon("/view.png");

        assert_eq!(action.action, "view");
        assert_eq!(action.title, "View");
        assert_eq!(action.icon, Some("/view.png".to_string()));
    }
}
