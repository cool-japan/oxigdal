//! # OtelHeaderInjector - Trait Implementations
//!
//! This module contains trait implementations for `OtelHeaderInjector`.
//!
//! ## Implemented Traits
//!
//! - `Injector`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::propagation::Injector;

use super::types::OtelHeaderInjector;

impl<'a> Injector for OtelHeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.headers.insert(key.to_string(), value);
    }
}
