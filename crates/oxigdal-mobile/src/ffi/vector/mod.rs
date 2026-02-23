//! Vector operations FFI functions.
//!
//! Provides C-compatible functions for working with vector datasets (layers and features).

// Submodules
pub mod feature;
pub mod geometry;
pub mod layer;

#[cfg(test)]
mod tests;

// Re-export all public items from submodules
pub use feature::*;
pub use geometry::*;
pub use layer::*;
