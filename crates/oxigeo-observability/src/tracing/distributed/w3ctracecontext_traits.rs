//! # W3CTraceContext - Trait Implementations
//!
//! This module contains trait implementations for `W3CTraceContext`.
//!
//! ## Implemented Traits
//!
//! - `Display`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use std::fmt;

use super::types::W3CTraceContext;

impl fmt::Display for W3CTraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_header())
    }
}
