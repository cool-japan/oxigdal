//! Operational Transformation (OT)
//!
//! This module provides operational transformation for concurrent text editing.
//! OT allows multiple users to edit the same document concurrently while
//! maintaining consistency.

pub mod composer;
pub mod text_operation;

pub use composer::OperationComposer;
pub use text_operation::{Operation, TextOperation};

use crate::SyncResult;
use serde::{Deserialize, Serialize};

/// Trait for transformable operations
pub trait Transform: Clone + Serialize + for<'de> Deserialize<'de> {
    /// Transforms this operation against another concurrent operation
    ///
    /// Returns the transformed version of this operation that can be
    /// applied after the other operation.
    ///
    /// # Arguments
    ///
    /// * `other` - The concurrent operation to transform against
    ///
    /// # Returns
    ///
    /// The transformed operation
    fn transform(&self, other: &Self) -> SyncResult<Self>;

    /// Composes this operation with another sequential operation
    ///
    /// Returns a single operation that has the same effect as applying
    /// this operation followed by the other operation.
    ///
    /// # Arguments
    ///
    /// * `other` - The operation to compose with
    ///
    /// # Returns
    ///
    /// The composed operation
    fn compose(&self, other: &Self) -> SyncResult<Self>;

    /// Inverts this operation
    ///
    /// Returns an operation that undoes the effect of this operation.
    ///
    /// # Returns
    ///
    /// The inverted operation
    fn invert(&self) -> SyncResult<Self>;
}
