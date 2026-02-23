//! # OtelHeaderExtractor - Trait Implementations
//!
//! This module contains trait implementations for `OtelHeaderExtractor`.
//!
//! ## Implemented Traits
//!
//! - `Extractor`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use opentelemetry::propagation::Extractor;

use super::types::OtelHeaderExtractor;

impl<'a> Extractor for OtelHeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(|s| s.as_str())
    }
    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(|s| s.as_str()).collect()
    }
}
