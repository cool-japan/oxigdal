//! Service worker messaging for communication between main thread and service worker.

use crate::error::{PwaError, Result};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{MessageChannel, MessageEvent, ServiceWorkerRegistration};

/// Message types for service worker communication.
#[derive(Debug, Clone)]
pub enum ServiceWorkerMessage {
    /// Skip waiting and activate immediately
    SkipWaiting,

    /// Claim all clients
    ClaimClients,

    /// Clear all caches
    ClearCaches,

    /// Clear specific cache
    ClearCache {
        /// Name of cache to clear
        name: String,
    },

    /// Get cache names
    GetCacheNames,

    /// Prefetch resources
    PrefetchResources {
        /// URLs to prefetch
        urls: Vec<String>,
    },

    /// Custom message with arbitrary data
    Custom {
        /// Action identifier
        action: String,
        /// Arbitrary data payload
        data: JsValue,
    },

    /// Response from service worker
    Response {
        /// Whether the operation was successful
        success: bool,
        /// Optional response data
        data: Option<JsValue>,
    },

    /// Error from service worker
    Error {
        /// Error message
        message: String,
    },
}

impl ServiceWorkerMessage {
    /// Convert message to JsValue for posting.
    pub fn to_js_value(&self) -> Result<JsValue> {
        let obj = js_sys::Object::new();

        match self {
            ServiceWorkerMessage::SkipWaiting => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("SkipWaiting"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
            }
            ServiceWorkerMessage::ClaimClients => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("ClaimClients"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
            }
            ServiceWorkerMessage::ClearCaches => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("ClearCaches"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
            }
            ServiceWorkerMessage::ClearCache { name } => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("ClearCache"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
                js_sys::Reflect::set(&obj, &JsValue::from_str("name"), &JsValue::from_str(name))
                    .map_err(|_| PwaError::Serialization("Failed to set name".to_string()))?;
            }
            ServiceWorkerMessage::GetCacheNames => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("GetCacheNames"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
            }
            ServiceWorkerMessage::PrefetchResources { urls } => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("PrefetchResources"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
                let array = js_sys::Array::new();
                for url in urls {
                    array.push(&JsValue::from_str(url));
                }
                js_sys::Reflect::set(&obj, &JsValue::from_str("urls"), &array)
                    .map_err(|_| PwaError::Serialization("Failed to set urls".to_string()))?;
            }
            ServiceWorkerMessage::Custom { action, data } => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("Custom"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("action"),
                    &JsValue::from_str(action),
                )
                .map_err(|_| PwaError::Serialization("Failed to set action".to_string()))?;
                js_sys::Reflect::set(&obj, &JsValue::from_str("data"), data)
                    .map_err(|_| PwaError::Serialization("Failed to set data".to_string()))?;
            }
            ServiceWorkerMessage::Response { success, data } => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("Response"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("success"),
                    &JsValue::from_bool(*success),
                )
                .map_err(|_| PwaError::Serialization("Failed to set success".to_string()))?;
                if let Some(d) = data {
                    js_sys::Reflect::set(&obj, &JsValue::from_str("data"), d)
                        .map_err(|_| PwaError::Serialization("Failed to set data".to_string()))?;
                }
            }
            ServiceWorkerMessage::Error { message } => {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("type"),
                    &JsValue::from_str("Error"),
                )
                .map_err(|_| PwaError::Serialization("Failed to set type".to_string()))?;
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("message"),
                    &JsValue::from_str(message),
                )
                .map_err(|_| PwaError::Serialization("Failed to set message".to_string()))?;
            }
        }

        Ok(obj.into())
    }

    /// Convert JsValue to message.
    pub fn from_js_value(value: JsValue) -> Result<Self> {
        let type_val = js_sys::Reflect::get(&value, &JsValue::from_str("type"))
            .map_err(|_| PwaError::Deserialization("Failed to get type".to_string()))?;

        let type_str = type_val
            .as_string()
            .ok_or_else(|| PwaError::Deserialization("Type is not a string".to_string()))?;

        match type_str.as_str() {
            "SkipWaiting" => Ok(ServiceWorkerMessage::SkipWaiting),
            "ClaimClients" => Ok(ServiceWorkerMessage::ClaimClients),
            "ClearCaches" => Ok(ServiceWorkerMessage::ClearCaches),
            "ClearCache" => {
                let name = js_sys::Reflect::get(&value, &JsValue::from_str("name"))
                    .map_err(|_| PwaError::Deserialization("Failed to get name".to_string()))?
                    .as_string()
                    .ok_or_else(|| PwaError::Deserialization("Name is not a string".to_string()))?;
                Ok(ServiceWorkerMessage::ClearCache { name })
            }
            "GetCacheNames" => Ok(ServiceWorkerMessage::GetCacheNames),
            "PrefetchResources" => {
                let urls_val = js_sys::Reflect::get(&value, &JsValue::from_str("urls"))
                    .map_err(|_| PwaError::Deserialization("Failed to get urls".to_string()))?;
                let array = js_sys::Array::from(&urls_val);
                let mut urls = Vec::new();
                for i in 0..array.length() {
                    if let Some(url) = array.get(i).as_string() {
                        urls.push(url);
                    }
                }
                Ok(ServiceWorkerMessage::PrefetchResources { urls })
            }
            "Response" => {
                let success = js_sys::Reflect::get(&value, &JsValue::from_str("success"))
                    .map_err(|_| PwaError::Deserialization("Failed to get success".to_string()))?
                    .as_bool()
                    .ok_or_else(|| {
                        PwaError::Deserialization("Success is not a boolean".to_string())
                    })?;
                let data = js_sys::Reflect::get(&value, &JsValue::from_str("data")).ok();
                Ok(ServiceWorkerMessage::Response { success, data })
            }
            "Error" => {
                let message = js_sys::Reflect::get(&value, &JsValue::from_str("message"))
                    .map_err(|_| PwaError::Deserialization("Failed to get message".to_string()))?
                    .as_string()
                    .ok_or_else(|| {
                        PwaError::Deserialization("Message is not a string".to_string())
                    })?;
                Ok(ServiceWorkerMessage::Error { message })
            }
            _ => Err(PwaError::Deserialization(format!(
                "Unknown message type: {}",
                type_str
            ))),
        }
    }
}

/// Service worker messaging handler.
pub struct ServiceWorkerMessaging {
    registration: ServiceWorkerRegistration,
}

impl ServiceWorkerMessaging {
    /// Create a new messaging handler for a registration.
    pub fn new(registration: ServiceWorkerRegistration) -> Self {
        Self { registration }
    }

    /// Post a message to the service worker.
    pub fn post_message(&self, message: &ServiceWorkerMessage) -> Result<()> {
        let active = self
            .registration
            .active()
            .ok_or_else(|| PwaError::InvalidState("No active service worker".to_string()))?;

        let js_message = message.to_js_value()?;

        active
            .post_message(&js_message)
            .map_err(|e| PwaError::JsError(format!("Failed to post message: {:?}", e)))?;

        Ok(())
    }

    /// Post a message and wait for response using MessageChannel.
    pub async fn post_message_with_response(
        &self,
        message: &ServiceWorkerMessage,
    ) -> Result<ServiceWorkerMessage> {
        let active = self
            .registration
            .active()
            .ok_or_else(|| PwaError::InvalidState("No active service worker".to_string()))?;

        let channel = MessageChannel::new()
            .map_err(|e| PwaError::JsError(format!("Failed to create MessageChannel: {:?}", e)))?;

        let port1 = channel.port1();
        let port2 = channel.port2();

        // Create promise for response
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let onmessage = Closure::once(move |event: MessageEvent| {
                resolve.call1(&JsValue::NULL, &event.data()).ok();
            });

            port1.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();

            // Set timeout
            if let Some(window) = web_sys::window() {
                let timeout_reject = reject.clone();
                let timeout = Closure::once(move || {
                    timeout_reject
                        .call1(&JsValue::NULL, &JsValue::from_str("Message timeout"))
                        .ok();
                });

                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        timeout.as_ref().unchecked_ref(),
                        5000,
                    )
                    .ok();
                timeout.forget();
            }
        });

        // Post message with port
        let js_message = message.to_js_value()?;
        let transfer = js_sys::Array::new();
        transfer.push(&port2);

        active
            .post_message_with_transferable(&js_message, &transfer)
            .map_err(|e| PwaError::JsError(format!("Failed to post message: {:?}", e)))?;

        // Wait for response
        let response = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::JsError(format!("Message failed: {:?}", e)))?;

        ServiceWorkerMessage::from_js_value(response)
    }

    /// Send skip waiting message.
    pub fn skip_waiting(&self) -> Result<()> {
        self.post_message(&ServiceWorkerMessage::SkipWaiting)
    }

    /// Send claim clients message.
    pub fn claim_clients(&self) -> Result<()> {
        self.post_message(&ServiceWorkerMessage::ClaimClients)
    }

    /// Send clear caches message.
    pub async fn clear_caches(&self) -> Result<ServiceWorkerMessage> {
        self.post_message_with_response(&ServiceWorkerMessage::ClearCaches)
            .await
    }

    /// Send clear specific cache message.
    pub async fn clear_cache(&self, name: &str) -> Result<ServiceWorkerMessage> {
        self.post_message_with_response(&ServiceWorkerMessage::ClearCache {
            name: name.to_string(),
        })
        .await
    }

    /// Get cache names from service worker.
    pub async fn get_cache_names(&self) -> Result<Vec<String>> {
        let response = self
            .post_message_with_response(&ServiceWorkerMessage::GetCacheNames)
            .await?;

        match response {
            ServiceWorkerMessage::Response { data: Some(d), .. } => {
                if let Ok(array) = d.dyn_into::<js_sys::Array>() {
                    let mut names = Vec::new();
                    for i in 0..array.length() {
                        if let Some(name) = array.get(i).as_string() {
                            names.push(name);
                        }
                    }
                    Ok(names)
                } else {
                    Err(PwaError::Deserialization(
                        "Invalid cache names response".to_string(),
                    ))
                }
            }
            _ => Err(PwaError::JsError("Unexpected response".to_string())),
        }
    }

    /// Prefetch resources.
    pub async fn prefetch_resources(&self, urls: Vec<String>) -> Result<ServiceWorkerMessage> {
        self.post_message_with_response(&ServiceWorkerMessage::PrefetchResources { urls })
            .await
    }

    /// Post custom message.
    pub async fn post_custom(
        &self,
        action: impl Into<String>,
        data: JsValue,
    ) -> Result<ServiceWorkerMessage> {
        self.post_message_with_response(&ServiceWorkerMessage::Custom {
            action: action.into(),
            data,
        })
        .await
    }
}

/// Message listener for receiving messages from service worker.
pub struct MessageListener {
    callback: Box<dyn Fn(ServiceWorkerMessage)>,
}

impl MessageListener {
    /// Create a new message listener.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(ServiceWorkerMessage) + 'static,
    {
        Self {
            callback: Box::new(callback),
        }
    }

    /// Handle a message event.
    pub fn handle_event(&self, event: MessageEvent) {
        if let Ok(message) = ServiceWorkerMessage::from_js_value(event.data()) {
            (self.callback)(message);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_message_serialization() -> Result<()> {
        let message = ServiceWorkerMessage::SkipWaiting;
        let js_value = message.to_js_value()?;
        let deserialized = ServiceWorkerMessage::from_js_value(js_value)?;

        match deserialized {
            ServiceWorkerMessage::SkipWaiting => Ok(()),
            _ => Err(PwaError::Deserialization("Wrong message type".to_string())),
        }
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_message_with_payload() -> Result<()> {
        let message = ServiceWorkerMessage::ClearCache {
            name: "test-cache".to_string(),
        };
        let js_value = message.to_js_value()?;
        let deserialized = ServiceWorkerMessage::from_js_value(js_value)?;

        match deserialized {
            ServiceWorkerMessage::ClearCache { name } => {
                assert_eq!(name, "test-cache");
                Ok(())
            }
            _ => Err(PwaError::Deserialization("Wrong message type".to_string())),
        }
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_prefetch_message() -> Result<()> {
        let urls = vec!["/api/data.json".to_string(), "/images/logo.png".to_string()];
        let message = ServiceWorkerMessage::PrefetchResources { urls: urls.clone() };
        let js_value = message.to_js_value()?;
        let deserialized = ServiceWorkerMessage::from_js_value(js_value)?;

        match deserialized {
            ServiceWorkerMessage::PrefetchResources {
                urls: returned_urls,
            } => {
                assert_eq!(returned_urls, urls);
                Ok(())
            }
            _ => Err(PwaError::Deserialization("Wrong message type".to_string())),
        }
    }
}
