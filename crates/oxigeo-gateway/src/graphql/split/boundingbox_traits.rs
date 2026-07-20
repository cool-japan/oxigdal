//! # BoundingBox - Trait Implementations
//!
//! This module contains trait implementations for `BoundingBox`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::BoundingBox;

impl Default for BoundingBox {
    fn default() -> Self {
        Self {
            min_x: -180.0,
            min_y: -90.0,
            max_x: 180.0,
            max_y: 90.0,
        }
    }
}
