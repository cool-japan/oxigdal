//! PWA lifecycle management and install prompts.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Event, ServiceWorkerRegistration};

/// PWA installation state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallState {
    /// PWA is not installed
    NotInstalled,

    /// PWA install prompt is available
    PromptAvailable,

    /// PWA is installed
    Installed,

    /// PWA is being installed
    Installing,

    /// Install was dismissed
    Dismissed,
}

/// Display mode detection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisplayModeDetection {
    /// Running in browser
    Browser,

    /// Running as standalone PWA
    Standalone,

    /// Running in minimal UI mode
    MinimalUi,

    /// Running in fullscreen
    Fullscreen,

    /// Unknown display mode
    Unknown,
}

impl DisplayModeDetection {
    /// Detect current display mode.
    pub fn detect() -> Self {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return Self::Unknown,
        };

        // Check matchMedia for display-mode
        if Self::check_match_media(&window, "(display-mode: standalone)") {
            Self::Standalone
        } else if Self::check_match_media(&window, "(display-mode: fullscreen)") {
            Self::Fullscreen
        } else if Self::check_match_media(&window, "(display-mode: minimal-ui)") {
            Self::MinimalUi
        } else if Self::check_match_media(&window, "(display-mode: browser)") {
            Self::Browser
        } else {
            // Fallback: check navigator.standalone (iOS)
            if Self::is_ios_standalone(&window) {
                Self::Standalone
            } else {
                Self::Browser
            }
        }
    }

    /// Check if running as PWA.
    pub fn is_pwa(&self) -> bool {
        matches!(self, Self::Standalone | Self::MinimalUi | Self::Fullscreen)
    }

    /// Check matchMedia for a query.
    fn check_match_media(window: &web_sys::Window, query: &str) -> bool {
        if let Ok(Some(media_query_list)) = window.match_media(query) {
            media_query_list.matches()
        } else {
            false
        }
    }

    /// Check if running as standalone on iOS.
    fn is_ios_standalone(window: &web_sys::Window) -> bool {
        let navigator = window.navigator();
        js_sys::Reflect::get(&navigator, &JsValue::from_str("standalone"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

/// Install prompt handler for beforeinstallprompt event.
pub struct InstallPrompt {
    prompt_event: Option<JsValue>,
    state: InstallState,
}

impl InstallPrompt {
    /// Create a new install prompt handler.
    pub fn new() -> Self {
        Self {
            prompt_event: None,
            state: InstallState::NotInstalled,
        }
    }

    /// Set up the install prompt listener.
    pub fn setup(&mut self) -> Result<()> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        let self_ptr = self as *mut InstallPrompt;

        let closure = Closure::wrap(Box::new(move |event: Event| {
            // Prevent default to keep the prompt
            event.prevent_default();

            // Store the event
            #[allow(unsafe_code)]
            unsafe {
                if let Some(prompt_handler) = self_ptr.as_mut() {
                    prompt_handler.prompt_event = Some(event.into());
                    prompt_handler.state = InstallState::PromptAvailable;
                }
            }
        }) as Box<dyn FnMut(Event)>);

        window
            .add_event_listener_with_callback(
                "beforeinstallprompt",
                closure.as_ref().unchecked_ref(),
            )
            .map_err(|e| {
                PwaError::InstallPromptError(format!("Failed to add listener: {:?}", e))
            })?;

        closure.forget();

        Ok(())
    }

    /// Show the install prompt.
    pub async fn show_prompt(&mut self) -> Result<bool> {
        let event = self
            .prompt_event
            .as_ref()
            .ok_or_else(|| PwaError::InstallPromptError("No prompt event available".to_string()))?;

        // Call prompt() method on the event
        if let Ok(prompt_fn) = js_sys::Reflect::get(event, &JsValue::from_str("prompt")) {
            if let Ok(function) = prompt_fn.dyn_into::<js_sys::Function>() {
                function
                    .call0(event)
                    .map_err(|e| PwaError::InstallPromptError(format!("Prompt failed: {:?}", e)))?;

                // Wait for user choice
                if let Ok(user_choice_promise) =
                    js_sys::Reflect::get(event, &JsValue::from_str("userChoice"))
                {
                    if let Ok(promise) = user_choice_promise.dyn_into::<js_sys::Promise>() {
                        let result = wasm_bindgen_futures::JsFuture::from(promise)
                            .await
                            .map_err(|e| {
                                PwaError::InstallPromptError(format!("User choice failed: {:?}", e))
                            })?;

                        // Check the outcome
                        if let Ok(outcome) =
                            js_sys::Reflect::get(&result, &JsValue::from_str("outcome"))
                        {
                            if let Some(outcome_str) = outcome.as_string() {
                                if outcome_str == "accepted" {
                                    self.state = InstallState::Installing;
                                    return Ok(true);
                                } else {
                                    self.state = InstallState::Dismissed;
                                    return Ok(false);
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(PwaError::InstallPromptError(
            "Failed to show prompt".to_string(),
        ))
    }

    /// Check if prompt is available.
    pub fn is_available(&self) -> bool {
        self.prompt_event.is_some()
    }

    /// Get the current state.
    pub fn state(&self) -> InstallState {
        self.state
    }

    /// Clear the stored prompt event.
    pub fn clear(&mut self) {
        self.prompt_event = None;
        self.state = InstallState::NotInstalled;
    }
}

impl Default for InstallPrompt {
    fn default() -> Self {
        Self::new()
    }
}

/// PWA lifecycle manager.
pub struct PwaLifecycle {
    registration: Option<ServiceWorkerRegistration>,
    install_prompt: InstallPrompt,
    display_mode: DisplayModeDetection,
}

impl PwaLifecycle {
    /// Create a new PWA lifecycle manager.
    pub fn new() -> Self {
        Self {
            registration: None,
            install_prompt: InstallPrompt::new(),
            display_mode: DisplayModeDetection::detect(),
        }
    }

    /// Create with service worker registration.
    pub fn with_registration(registration: ServiceWorkerRegistration) -> Self {
        Self {
            registration: Some(registration),
            install_prompt: InstallPrompt::new(),
            display_mode: DisplayModeDetection::detect(),
        }
    }

    /// Initialize the lifecycle manager.
    pub fn initialize(&mut self) -> Result<()> {
        self.install_prompt.setup()?;
        self.display_mode = DisplayModeDetection::detect();
        Ok(())
    }

    /// Check if running as a PWA.
    pub fn is_pwa(&self) -> bool {
        self.display_mode.is_pwa()
    }

    /// Get the display mode.
    pub fn display_mode(&self) -> DisplayModeDetection {
        self.display_mode
    }

    /// Check if install prompt is available.
    pub fn can_install(&self) -> bool {
        self.install_prompt.is_available()
    }

    /// Show the install prompt.
    pub async fn prompt_install(&mut self) -> Result<bool> {
        self.install_prompt.show_prompt().await
    }

    /// Get the install state.
    pub fn install_state(&self) -> InstallState {
        if self.is_pwa() {
            InstallState::Installed
        } else {
            self.install_prompt.state()
        }
    }

    /// Set up app installed listener.
    pub fn on_app_installed<F>(&self, callback: F) -> Result<()>
    where
        F: Fn() + 'static,
    {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        let closure = Closure::wrap(Box::new(callback) as Box<dyn Fn()>);

        window
            .add_event_listener_with_callback("appinstalled", closure.as_ref().unchecked_ref())
            .map_err(|e| PwaError::LifecycleError(format!("Failed to add listener: {:?}", e)))?;

        closure.forget();

        Ok(())
    }

    /// Set up display mode change listener.
    pub fn on_display_mode_change<F>(&mut self, callback: F) -> Result<()>
    where
        F: Fn(DisplayModeDetection) + 'static,
    {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        // Listen to mediaQuery changes for display-mode
        let modes = vec![
            "(display-mode: standalone)",
            "(display-mode: fullscreen)",
            "(display-mode: minimal-ui)",
            "(display-mode: browser)",
        ];

        let callback = std::sync::Arc::new(callback);

        for query in modes {
            if let Ok(Some(media_query_list)) = window.match_media(query) {
                let callback_clone = callback.clone();
                let closure = Closure::wrap(Box::new(move |_event: web_sys::MediaQueryListEvent| {
                    let new_mode = DisplayModeDetection::detect();
                    callback_clone(new_mode);
                })
                    as Box<dyn FnMut(web_sys::MediaQueryListEvent)>);

                media_query_list
                    .add_listener_with_opt_callback(Some(closure.as_ref().unchecked_ref()))
                    .ok();

                closure.forget();
            }
        }

        Ok(())
    }

    /// Get service worker registration.
    pub fn registration(&self) -> Option<&ServiceWorkerRegistration> {
        self.registration.as_ref()
    }

    /// Update the service worker registration.
    pub fn set_registration(&mut self, registration: ServiceWorkerRegistration) {
        self.registration = Some(registration);
    }
}

impl Default for PwaLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// PWA update manager for handling service worker updates.
pub struct UpdateManager {
    registration: ServiceWorkerRegistration,
}

impl UpdateManager {
    /// Create a new update manager.
    pub fn new(registration: ServiceWorkerRegistration) -> Self {
        Self { registration }
    }

    /// Check for updates.
    pub async fn check_for_updates(&self) -> Result<bool> {
        let promise = self
            .registration
            .update()
            .map_err(|e| PwaError::LifecycleError(format!("Update check failed: {:?}", e)))?;

        let _result = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::LifecycleError(format!("Update promise failed: {:?}", e)))?;

        // Check if there's a waiting worker
        let has_update = self.registration.waiting().is_some();

        Ok(has_update)
    }

    /// Set up update available listener.
    pub fn on_update_available<F>(&self, callback: F) -> Result<()>
    where
        F: Fn() + 'static,
    {
        let closure = Closure::wrap(Box::new(callback) as Box<dyn Fn()>);

        self.registration
            .set_onupdatefound(Some(closure.as_ref().unchecked_ref()));

        closure.forget();

        Ok(())
    }

    /// Activate waiting service worker.
    pub fn activate_update(&self) -> Result<()> {
        if let Some(waiting) = self.registration.waiting() {
            waiting
                .post_message(&JsValue::from_str("skipWaiting"))
                .map_err(|e| PwaError::LifecycleError(format!("Skip waiting failed: {:?}", e)))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_state() {
        assert_eq!(InstallState::NotInstalled, InstallState::NotInstalled);
        assert_ne!(InstallState::NotInstalled, InstallState::Installed);
    }

    #[test]
    fn test_display_mode() {
        let mode = DisplayModeDetection::Browser;
        assert!(!mode.is_pwa());

        let mode = DisplayModeDetection::Standalone;
        assert!(mode.is_pwa());
    }

    #[test]
    fn test_install_prompt_creation() {
        let prompt = InstallPrompt::new();
        assert_eq!(prompt.state(), InstallState::NotInstalled);
        assert!(!prompt.is_available());
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_lifecycle_creation() {
        let lifecycle = PwaLifecycle::new();
        assert!(!lifecycle.can_install());
    }
}
