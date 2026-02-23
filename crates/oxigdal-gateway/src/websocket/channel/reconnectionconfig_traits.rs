//! # ReconnectionConfig - Trait Implementations
//!
//! This module contains trait implementations for `ReconnectionConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use std::time::Duration;

use super::types::ReconnectionConfig;

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
            max_attempts: Some(10),
            jitter: 0.1,
        }
    }
}
