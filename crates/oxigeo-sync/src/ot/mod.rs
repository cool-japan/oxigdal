//! Operational Transformation (OT)
//!
//! This module provides operational transformation for concurrent text editing.
//! OT allows multiple users to edit the same document concurrently while
//! maintaining consistency.

pub mod composer;
pub mod text_operation;

pub use composer::{OperationComposer, transform_pair};
pub use text_operation::{Operation, TextOperation};

use crate::SyncResult;
use serde::{Deserialize, Serialize};

/// Tie-breaking priority used by [`Transform::transform`] when both operands
/// insert content at the same position concurrently.
///
/// Operational transformation cannot infer, from the operations alone, which
/// of two concurrent insertions at the same point should end up first in the
/// merged document -- that choice has to be made consistently by the caller.
/// [`composer::transform_pair`] assigns [`Priority::Left`] to one operand and
/// [`Priority::Right`] to the other so that both `transform()` calls in the
/// pair agree on the same tie-break, which is required for the standard OT
/// convergence property (TP1): `apply(b', apply(a, s)) == apply(a', apply(b, s))`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// This operation's insertions are ordered before the other operation's
    /// insertions when both insert at the same position.
    Left,
    /// This operation's insertions are ordered after the other operation's
    /// insertions when both insert at the same position.
    Right,
}

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
    /// * `priority` - Which side wins ties when both operations insert
    ///   content at the same position. Callers transforming a pair of
    ///   concurrent operations against each other MUST use opposite
    ///   priorities for the two calls (see [`composer::transform_pair`])
    ///   so both results agree on the same insertion order.
    ///
    /// # Returns
    ///
    /// The transformed operation
    fn transform(&self, other: &Self, priority: Priority) -> SyncResult<Self>;

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
