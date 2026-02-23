//! # MultiplexerConfig - Trait Implementations
//!
//! This module contains trait implementations for `MultiplexerConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::{BackpressureConfig, HeartbeatConfig, MultiplexerConfig};

impl Default for MultiplexerConfig {
    fn default() -> Self {
        Self {
            max_channels: 64,
            channel_buffer_size: 256,
            heartbeat: HeartbeatConfig::default(),
            backpressure: BackpressureConfig::default(),
            default_compression: true,
        }
    }
}
