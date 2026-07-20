//! # GraphQLConfig - Trait Implementations
//!
//! This module contains trait implementations for `GraphQLConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::GraphQLConfig;

impl Default for GraphQLConfig {
    fn default() -> Self {
        Self {
            enable_introspection: true,
            max_depth: 10,
            max_complexity: 1000,
            enable_subscriptions: true,
            enable_dataloader: true,
            enable_tracing: false,
            cache_ttl_secs: 300,
            max_page_size: 100,
        }
    }
}
