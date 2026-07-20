//! # TaskStatus - Trait Implementations
//!
//! This module contains trait implementations for `TaskStatus`.
//!
//! ## Implemented Traits
//!
//! - `Default`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::TaskStatus;

impl Default for TaskStatus {
    fn default() -> Self {
        Self::WaitingDependencies {
            pending_deps: Vec::new(),
        }
    }
}

