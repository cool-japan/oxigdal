//! # BaggageMetadata - Trait Implementations
//!
//! This module contains trait implementations for `BaggageMetadata`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::BaggageMetadata;

impl Default for BaggageMetadata {
    fn default() -> Self {
        Self {
            propagate: true,
            source_service: None,
            ttl_seconds: 0,
        }
    }
}
