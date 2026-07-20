//! # `EviConfig` - Trait Implementations
//!
//! This module contains trait implementations for `EviConfig`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::EviConfig;

impl Default for EviConfig {
    fn default() -> Self {
        Self {
            g: 2.5,
            c1: 6.0,
            c2: 7.5,
            l: 1.0,
        }
    }
}
