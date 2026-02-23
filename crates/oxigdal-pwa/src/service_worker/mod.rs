//! Service worker integration for PWA functionality.

pub mod events;
pub mod messaging;
pub mod registration;
pub mod scope;

use crate::error::{PwaError, Result};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{ServiceWorkerContainer, ServiceWorkerRegistration};

pub use events::ServiceWorkerEvents;
pub use messaging::ServiceWorkerMessaging;
pub use registration::ServiceWorkerRegistry;
pub use scope::ServiceWorkerScope;

/// Get the service worker container from the window.
pub fn get_service_worker_container() -> Result<ServiceWorkerContainer> {
    let window = web_sys::window()
        .ok_or_else(|| PwaError::InvalidState("No window object available".to_string()))?;

    let navigator = window.navigator();
    let service_worker = navigator.service_worker();

    Ok(service_worker)
}

/// Check if service workers are supported.
pub fn is_service_worker_supported() -> bool {
    if let Some(window) = web_sys::window() {
        let navigator = window.navigator();
        // Check if service_worker property exists by trying to access it
        js_sys::Reflect::has(&navigator, &JsValue::from_str("serviceWorker")).unwrap_or(false)
    } else {
        false
    }
}

/// Register a service worker at the given URL.
pub async fn register_service_worker(
    url: &str,
    scope: Option<&str>,
) -> Result<ServiceWorkerRegistration> {
    if !is_service_worker_supported() {
        return Err(PwaError::ServiceWorkerNotSupported);
    }

    let container = get_service_worker_container()?;

    let promise = if let Some(scope_path) = scope {
        // Try to use options if available
        let options = web_sys::RegistrationOptions::new();
        options.set_scope(scope_path);

        // Call register with URL and options
        match js_sys::Reflect::get(&container, &JsValue::from_str("register")) {
            Ok(register_fn) => {
                let register_fn = register_fn.dyn_into::<js_sys::Function>().map_err(|_| {
                    PwaError::ServiceWorkerRegistration("register is not a function".to_string())
                })?;

                let args = js_sys::Array::new();
                args.push(&JsValue::from_str(url));
                args.push(&options);

                register_fn
                    .apply(&container, &args)
                    .map_err(|e| {
                        PwaError::ServiceWorkerRegistration(format!("Register failed: {:?}", e))
                    })?
                    .dyn_into::<js_sys::Promise>()
                    .map_err(|_| {
                        PwaError::ServiceWorkerRegistration(
                            "Register did not return a Promise".to_string(),
                        )
                    })?
            }
            Err(e) => {
                return Err(PwaError::ServiceWorkerRegistration(format!(
                    "Failed to get register method: {:?}",
                    e
                )));
            }
        }
    } else {
        container.register(url)
    };

    let registration = JsFuture::from(promise).await.map_err(|e| {
        PwaError::ServiceWorkerRegistration(format!("Registration promise failed: {:?}", e))
    })?;

    registration
        .dyn_into::<ServiceWorkerRegistration>()
        .map_err(|_| PwaError::ServiceWorkerRegistration("Invalid registration object".to_string()))
}

/// Unregister a service worker registration.
pub async fn unregister_service_worker(registration: &ServiceWorkerRegistration) -> Result<bool> {
    let promise = registration
        .unregister()
        .map_err(|e| PwaError::ServiceWorkerRegistration(format!("Unregister failed: {:?}", e)))?;

    let result = JsFuture::from(promise).await.map_err(|e| {
        PwaError::ServiceWorkerRegistration(format!("Unregister promise failed: {:?}", e))
    })?;

    result
        .as_bool()
        .ok_or_else(|| PwaError::ServiceWorkerRegistration("Invalid unregister result".to_string()))
}

/// Get the current active service worker registration.
pub async fn get_registration(scope: Option<&str>) -> Result<Option<ServiceWorkerRegistration>> {
    if !is_service_worker_supported() {
        return Ok(None);
    }

    let container = get_service_worker_container()?;

    let promise = if let Some(scope_url) = scope {
        container.get_registration_with_document_url(scope_url)
    } else {
        container.get_registration()
    };

    let result = JsFuture::from(promise).await.map_err(|e| {
        PwaError::ServiceWorkerRegistration(format!("Get registration promise failed: {:?}", e))
    })?;

    if result.is_undefined() || result.is_null() {
        Ok(None)
    } else {
        let registration = result
            .dyn_into::<ServiceWorkerRegistration>()
            .map_err(|_| {
                PwaError::ServiceWorkerRegistration("Invalid registration object".to_string())
            })?;
        Ok(Some(registration))
    }
}

/// Get all service worker registrations.
pub async fn get_registrations() -> Result<Vec<ServiceWorkerRegistration>> {
    if !is_service_worker_supported() {
        return Ok(Vec::new());
    }

    let container = get_service_worker_container()?;
    let promise = container.get_registrations();

    let result = JsFuture::from(promise).await.map_err(|e| {
        PwaError::ServiceWorkerRegistration(format!("Get registrations promise failed: {:?}", e))
    })?;

    let array = js_sys::Array::from(&result);
    let mut registrations = Vec::new();

    for i in 0..array.length() {
        if let Ok(registration) = array.get(i).dyn_into::<ServiceWorkerRegistration>() {
            registrations.push(registration);
        }
    }

    Ok(registrations)
}

/// Update a service worker registration.
pub async fn update_registration(
    registration: &ServiceWorkerRegistration,
) -> Result<ServiceWorkerRegistration> {
    let promise = registration
        .update()
        .map_err(|e| PwaError::ServiceWorkerRegistration(format!("Update failed: {:?}", e)))?;

    let result = JsFuture::from(promise).await.map_err(|e| {
        PwaError::ServiceWorkerRegistration(format!("Update promise failed: {:?}", e))
    })?;

    result
        .dyn_into::<ServiceWorkerRegistration>()
        .map_err(|_| PwaError::ServiceWorkerRegistration("Invalid update result".to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_service_worker_support_check() {
        // This will return false in non-browser environments
        let _supported = is_service_worker_supported();
        // Just ensure it doesn't panic
    }
}
