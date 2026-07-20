//! Context management for distributed tracing.

use opentelemetry::global;
use opentelemetry::propagation::{Extractor, Injector};
use std::collections::HashMap;

/// Extract OpenTelemetry context from HTTP headers.
pub fn extract_context(headers: &HashMap<String, String>) -> opentelemetry::Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
}

/// Inject OpenTelemetry context into HTTP headers.
pub fn inject_context(ctx: &opentelemetry::Context, headers: &mut HashMap<String, String>) {
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(ctx, &mut HeaderInjector(headers))
    })
}

struct HeaderExtractor<'a>(&'a HashMap<String, String>);

impl<'a> Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|s| s.as_str()).collect()
    }
}

struct HeaderInjector<'a>(&'a mut HashMap<String, String>);

impl<'a> Injector for HeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_extraction() {
        let mut headers = HashMap::new();
        headers.insert(
            "traceparent".to_string(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_string(),
        );

        let _ctx = extract_context(&headers);
    }

    #[test]
    fn test_context_injection() {
        let ctx = opentelemetry::Context::current();
        let mut headers = HashMap::new();

        inject_context(&ctx, &mut headers);
    }
}
