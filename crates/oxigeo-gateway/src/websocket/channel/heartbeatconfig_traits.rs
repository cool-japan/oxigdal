//! # HeartbeatConfig - Trait Implementations
//!
//! This module contains trait implementations for `HeartbeatConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use std::time::Duration;

use super::functions::{DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_HEARTBEAT_TIMEOUT};
use super::types::HeartbeatConfig;

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(DEFAULT_HEARTBEAT_INTERVAL),
            timeout: Duration::from_secs(DEFAULT_HEARTBEAT_TIMEOUT),
            max_missed: 3,
        }
    }
}
