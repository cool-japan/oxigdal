//! # BackpressureConfig - Trait Implementations
//!
//! This module contains trait implementations for `BackpressureConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::functions::{DEFAULT_HIGH_WATERMARK, DEFAULT_LOW_WATERMARK};
use super::types::BackpressureConfig;

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            high_watermark: DEFAULT_HIGH_WATERMARK,
            low_watermark: DEFAULT_LOW_WATERMARK,
            channel_limit: 32 * 1024,
        }
    }
}
