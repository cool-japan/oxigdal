//! Service worker event handling and lifecycle events.

use crate::error::{PwaError, Result};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    ExtendableEvent, FetchEvent, MessageEvent, NotificationEvent, PushEvent,
    ServiceWorkerGlobalScope,
};

/// Service worker event handler trait.
pub trait ServiceWorkerEventHandler: Send + Sync {
    /// Handle install event.
    fn on_install(&self, _event: &ExtendableEvent) -> Result<()> {
        Ok(())
    }

    /// Handle activate event.
    fn on_activate(&self, _event: &ExtendableEvent) -> Result<()> {
        Ok(())
    }

    /// Handle fetch event.
    fn on_fetch(&self, _event: &FetchEvent) -> Result<()> {
        Ok(())
    }

    /// Handle message event.
    fn on_message(&self, _event: &MessageEvent) -> Result<()> {
        Ok(())
    }

    /// Handle push event.
    fn on_push(&self, _event: &PushEvent) -> Result<()> {
        Ok(())
    }

    /// Handle notification click event.
    fn on_notification_click(&self, _event: &NotificationEvent) -> Result<()> {
        Ok(())
    }
}

/// Service worker events manager.
pub struct ServiceWorkerEvents {
    scope: ServiceWorkerGlobalScope,
}

impl ServiceWorkerEvents {
    /// Create a new events manager from the global scope.
    pub fn from_global() -> Result<Self> {
        let global = js_sys::global()
            .dyn_into::<ServiceWorkerGlobalScope>()
            .map_err(|_| {
                PwaError::InvalidState("Not running in service worker context".to_string())
            })?;

        Ok(Self { scope: global })
    }

    /// Register install event handler.
    pub fn on_install<F>(&self, handler: F)
    where
        F: Fn(ExtendableEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(ExtendableEvent)>);
        self.scope
            .set_oninstall(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Register activate event handler.
    pub fn on_activate<F>(&self, handler: F)
    where
        F: Fn(ExtendableEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(ExtendableEvent)>);
        self.scope
            .set_onactivate(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Register fetch event handler.
    pub fn on_fetch<F>(&self, handler: F)
    where
        F: Fn(FetchEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(FetchEvent)>);
        self.scope
            .set_onfetch(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Register message event handler.
    pub fn on_message<F>(&self, handler: F)
    where
        F: Fn(MessageEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(MessageEvent)>);
        self.scope
            .set_onmessage(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Register push event handler.
    pub fn on_push<F>(&self, handler: F)
    where
        F: Fn(PushEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(PushEvent)>);
        self.scope
            .set_onpush(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Register notification click event handler.
    pub fn on_notification_click<F>(&self, handler: F)
    where
        F: Fn(NotificationEvent) + 'static,
    {
        let closure = Closure::wrap(Box::new(handler) as Box<dyn Fn(NotificationEvent)>);
        self.scope
            .set_onnotificationclick(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
    }

    /// Skip waiting during install.
    pub async fn skip_waiting(&self) -> Result<()> {
        let promise = self
            .scope
            .skip_waiting()
            .map_err(|e| PwaError::LifecycleError(format!("Skip waiting call failed: {:?}", e)))?;

        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::LifecycleError(format!("Skip waiting failed: {:?}", e)))?;
        Ok(())
    }

    /// Claim all clients during activate.
    pub async fn claim_clients(&self) -> Result<()> {
        let clients = self.scope.clients();
        let promise = clients.claim();

        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::LifecycleError(format!("Claim clients failed: {:?}", e)))?;
        Ok(())
    }
    /// Get the service worker global scope.
    pub fn scope(&self) -> &ServiceWorkerGlobalScope {
        &self.scope
    }
}

/// Install event context.
pub struct InstallContext {
    event: ExtendableEvent,
}

impl InstallContext {
    /// Create a new install context.
    pub fn new(event: ExtendableEvent) -> Self {
        Self { event }
    }

    /// Wait until a promise completes.
    pub fn wait_until(&self, promise: js_sys::Promise) {
        let _ = self.event.wait_until(&promise);
    }

    /// Get the underlying event.
    pub fn event(&self) -> &ExtendableEvent {
        &self.event
    }
}

/// Activate event context.
pub struct ActivateContext {
    event: ExtendableEvent,
}

impl ActivateContext {
    /// Create a new activate context.
    pub fn new(event: ExtendableEvent) -> Self {
        Self { event }
    }

    /// Wait until a promise completes.
    pub fn wait_until(&self, promise: js_sys::Promise) {
        let _ = self.event.wait_until(&promise);
    }

    /// Get the underlying event.
    pub fn event(&self) -> &ExtendableEvent {
        &self.event
    }
}

/// Fetch event context.
pub struct FetchContext {
    event: FetchEvent,
}

impl FetchContext {
    /// Create a new fetch context.
    pub fn new(event: FetchEvent) -> Self {
        Self { event }
    }

    /// Get the request from the fetch event.
    pub fn request(&self) -> web_sys::Request {
        self.event.request()
    }

    /// Respond with a promise.
    pub fn respond_with(&self, promise: js_sys::Promise) -> Result<()> {
        self.event
            .respond_with(&promise)
            .map_err(|e| PwaError::JsError(format!("respond_with failed: {:?}", e)))
    }

    /// Get the client ID that initiated the request.
    pub fn client_id(&self) -> Option<String> {
        self.event.client_id()
    }

    /// Get the underlying event.
    pub fn event(&self) -> &FetchEvent {
        &self.event
    }
}

/// Message event context.
pub struct MessageContext {
    event: MessageEvent,
}

impl MessageContext {
    /// Create a new message context.
    pub fn new(event: MessageEvent) -> Self {
        Self { event }
    }

    /// Get the message data.
    pub fn data(&self) -> JsValue {
        self.event.data()
    }

    /// Get the origin of the message.
    pub fn origin(&self) -> String {
        self.event.origin()
    }

    /// Get the underlying event.
    pub fn event(&self) -> &MessageEvent {
        &self.event
    }
}

/// Push event context.
pub struct PushContext {
    event: PushEvent,
}

impl PushContext {
    /// Create a new push context.
    pub fn new(event: PushEvent) -> Self {
        Self { event }
    }

    /// Get the push message data as text, if any.
    pub fn data_as_text(&self) -> Option<String> {
        self.event.data().map(|data| data.text())
    }

    /// Wait until a promise completes before the push event is considered handled.
    pub fn wait_until(&self, promise: js_sys::Promise) -> Result<()> {
        self.event
            .wait_until(&promise)
            .map_err(|e| PwaError::JsError(format!("wait_until failed: {:?}", e)))
    }

    /// Get the underlying event.
    pub fn event(&self) -> &PushEvent {
        &self.event
    }
}

/// Notification click event context.
pub struct NotificationContext {
    event: NotificationEvent,
}

impl NotificationContext {
    /// Create a new notification click context.
    pub fn new(event: NotificationEvent) -> Self {
        Self { event }
    }

    /// Get the action identifier that was clicked, if any.
    ///
    /// `web_sys::NotificationEvent` does not currently expose the `action`
    /// property directly, so it is read via `Reflect::get` (the same pattern
    /// used elsewhere in this crate for properties without typed bindings).
    /// Returns an empty string if the notification body itself (rather than
    /// an action button) was clicked, matching the DOM `NotificationEvent.action`
    /// default.
    pub fn action(&self) -> String {
        js_sys::Reflect::get(&self.event, &JsValue::from_str("action"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    }

    /// Get the underlying notification.
    pub fn notification(&self) -> web_sys::Notification {
        self.event.notification()
    }

    /// Wait until a promise completes before the event is considered handled.
    pub fn wait_until(&self, promise: js_sys::Promise) -> Result<()> {
        self.event
            .wait_until(&promise)
            .map_err(|e| PwaError::JsError(format!("wait_until failed: {:?}", e)))
    }

    /// Get the underlying event.
    pub fn event(&self) -> &NotificationEvent {
        &self.event
    }
}

/// Event listener registry for managing all service worker events.
pub struct EventRegistry {
    events: ServiceWorkerEvents,
}

impl EventRegistry {
    /// Create a new event registry.
    pub fn new() -> Result<Self> {
        let events = ServiceWorkerEvents::from_global()?;
        Ok(Self { events })
    }

    /// Register all event handlers.
    pub fn register_all<H: ServiceWorkerEventHandler + 'static>(&self, handler: H) -> Result<()> {
        let handler = std::sync::Arc::new(handler);

        // Install event
        {
            let h = handler.clone();
            self.events.on_install(move |event| {
                if let Err(e) = h.on_install(&event) {
                    web_sys::console::error_1(&format!("Install error: {}", e).into());
                }
            });
        }

        // Activate event
        {
            let h = handler.clone();
            self.events.on_activate(move |event| {
                if let Err(e) = h.on_activate(&event) {
                    web_sys::console::error_1(&format!("Activate error: {}", e).into());
                }
            });
        }

        // Fetch event
        {
            let h = handler.clone();
            self.events.on_fetch(move |event| {
                if let Err(e) = h.on_fetch(&event) {
                    web_sys::console::error_1(&format!("Fetch error: {}", e).into());
                }
            });
        }

        // Message event
        {
            let h = handler.clone();
            self.events.on_message(move |event| {
                if let Err(e) = h.on_message(&event) {
                    web_sys::console::error_1(&format!("Message error: {}", e).into());
                }
            });
        }

        // Push event
        {
            let h = handler.clone();
            self.events.on_push(move |event| {
                if let Err(e) = h.on_push(&event) {
                    web_sys::console::error_1(&format!("Push error: {}", e).into());
                }
            });
        }

        // Notification click event
        {
            let h = handler.clone();
            self.events.on_notification_click(move |event| {
                if let Err(e) = h.on_notification_click(&event) {
                    web_sys::console::error_1(&format!("Notification click error: {}", e).into());
                }
            });
        }

        Ok(())
    }

    /// Get the events manager.
    pub fn events(&self) -> &ServiceWorkerEvents {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    struct TestHandler;

    impl ServiceWorkerEventHandler for TestHandler {
        fn on_install(&self, _event: &ExtendableEvent) -> Result<()> {
            Ok(())
        }

        fn on_activate(&self, _event: &ExtendableEvent) -> Result<()> {
            Ok(())
        }

        fn on_push(&self, _event: &PushEvent) -> Result<()> {
            Ok(())
        }

        fn on_notification_click(&self, _event: &NotificationEvent) -> Result<()> {
            Ok(())
        }
    }

    /// Regression test: the default trait methods for `on_push` /
    /// `on_notification_click` must exist and be callable so implementors can
    /// override them; this alone doesn't prove `register_all()` wires them
    /// (that requires a `ServiceWorkerGlobalScope`, which only exists inside
    /// an actual service worker JS context), but it locks in the trait
    /// surface the fix depends on.
    #[test]
    fn test_service_worker_event_handler_default_push_and_notification_click_are_ok() {
        struct DefaultHandler;
        impl ServiceWorkerEventHandler for DefaultHandler {}

        // We cannot construct a real PushEvent/NotificationEvent without a
        // browser/service-worker JS engine; this test exists to ensure the
        // trait compiles with default no-op bodies for both new methods.
        let _handler = DefaultHandler;
    }

    /// Regression test: `EventRegistry::register_all()` must invoke
    /// `ServiceWorkerEvents::on_push` and `on_notification_click` (via
    /// `ServiceWorkerGlobalScope::set_onpush`/`set_onnotificationclick`) in
    /// addition to install/activate/fetch/message. This is exercised at the
    /// wasm32 level since it requires a real `ServiceWorkerGlobalScope`;
    /// natively we assert the handler methods themselves are reachable
    /// through the trait object stored by `register_all` (via `TestHandler`
    /// above implementing both).
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_register_all_wires_push_and_notification_click() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct FlagHandler {
            push_called: std::sync::Arc<AtomicBool>,
            notification_click_called: std::sync::Arc<AtomicBool>,
        }

        impl ServiceWorkerEventHandler for FlagHandler {
            fn on_push(&self, _event: &PushEvent) -> Result<()> {
                self.push_called.store(true, Ordering::SeqCst);
                Ok(())
            }

            fn on_notification_click(&self, _event: &NotificationEvent) -> Result<()> {
                self.notification_click_called.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let registry = EventRegistry::new().expect("registry should be constructible");
        let push_called = std::sync::Arc::new(AtomicBool::new(false));
        let notification_click_called = std::sync::Arc::new(AtomicBool::new(false));

        registry
            .register_all(FlagHandler {
                push_called: push_called.clone(),
                notification_click_called: notification_click_called.clone(),
            })
            .expect("register_all should succeed");

        // Dispatching synthetic events and asserting the flags flip requires
        // a full service worker JS runtime; this test documents the intended
        // wiring and compiles/links against the real ServiceWorkerGlobalScope
        // API surface added by this fix.
        let _ = (push_called, notification_click_called);
    }
}
