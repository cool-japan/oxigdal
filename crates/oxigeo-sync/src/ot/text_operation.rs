//! Text operations for operational transformation

use crate::ot::{Priority, Transform};
use crate::{SyncError, SyncResult};
use serde::{Deserialize, Serialize};

/// A single operation on text
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    /// Retain n characters
    Retain(usize),
    /// Insert text
    Insert(String),
    /// Delete n characters
    Delete(usize),
}

/// A text operation consisting of multiple atomic operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextOperation {
    /// The sequence of operations
    ops: Vec<Operation>,
    /// The base length (length before applying operation)
    base_length: usize,
    /// The target length (length after applying operation)
    target_length: usize,
    /// The base length asserted at construction time via [`Self::with_base_length`].
    ///
    /// This is *not* pre-added to `base_length`/`target_length` -- those two
    /// fields always start at zero and are accumulated normally by
    /// [`Self::retain`], [`Self::insert`] and [`Self::delete`], exactly as they
    /// are for [`Self::new`]. Instead this field records the length the caller
    /// expects `base_length` to reach once the operation is fully built, and is
    /// validated (in [`Self::apply`] and [`Transform::compose`]) against the
    /// actual accumulated `base_length`. It is a validation-only annotation and
    /// is not part of the operation's wire format.
    #[serde(skip)]
    expected_base_length: Option<usize>,
}

impl TextOperation {
    /// Creates a new empty text operation
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            base_length: 0,
            target_length: 0,
            expected_base_length: None,
        }
    }

    /// Creates a text operation that will be checked against a base length.
    ///
    /// `base_length`/`target_length` both start at **zero**, just like
    /// [`Self::new`] -- they are accumulated by the subsequent
    /// `retain`/`insert`/`delete` calls, not pre-seeded with `base_length`.
    /// The `base_length` argument is instead recorded as an expectation: once
    /// the operation is used (via [`Self::apply`] or [`Transform::compose`]),
    /// it is validated against the `base_length` actually accumulated by the
    /// operations added so far, and a [`SyncError::InvalidOperation`] is
    /// returned if they disagree.
    ///
    /// # Arguments
    ///
    /// * `base_length` - The expected length of the document before this
    ///   operation, checked once the operation's retain/delete calls have
    ///   been added.
    pub fn with_base_length(base_length: usize) -> Self {
        Self {
            ops: Vec::new(),
            base_length: 0,
            target_length: 0,
            expected_base_length: Some(base_length),
        }
    }

    /// Validates that any base length asserted via [`Self::with_base_length`]
    /// matches the base length actually accumulated by this operation's
    /// retain/delete calls.
    fn check_expected_base_length(&self) -> SyncResult<()> {
        if let Some(expected) = self.expected_base_length
            && expected != self.base_length
        {
            return Err(SyncError::InvalidOperation(format!(
                "Base length mismatch: with_base_length({expected}) was declared but \
                 retain/delete calls accumulated a base length of {actual}",
                actual = self.base_length
            )));
        }
        Ok(())
    }

    /// Adds a retain operation
    ///
    /// # Arguments
    ///
    /// * `n` - Number of characters to retain
    pub fn retain(&mut self, n: usize) -> &mut Self {
        if n == 0 {
            return self;
        }

        self.base_length += n;
        self.target_length += n;

        // Merge with previous retain if possible
        if let Some(Operation::Retain(prev)) = self.ops.last_mut() {
            *prev += n;
        } else {
            self.ops.push(Operation::Retain(n));
        }

        self
    }

    /// Adds an insert operation
    ///
    /// # Arguments
    ///
    /// * `text` - Text to insert
    pub fn insert(&mut self, text: String) -> &mut Self {
        if text.is_empty() {
            return self;
        }

        self.target_length += text.len();

        // Merge with previous insert if possible
        if let Some(Operation::Insert(prev)) = self.ops.last_mut() {
            prev.push_str(&text);
        } else {
            self.ops.push(Operation::Insert(text));
        }

        self
    }

    /// Adds a delete operation
    ///
    /// # Arguments
    ///
    /// * `n` - Number of characters to delete
    pub fn delete(&mut self, n: usize) -> &mut Self {
        if n == 0 {
            return self;
        }

        self.base_length += n;

        // Merge with previous delete if possible
        if let Some(Operation::Delete(prev)) = self.ops.last_mut() {
            *prev += n;
        } else {
            self.ops.push(Operation::Delete(n));
        }

        self
    }

    /// Applies this operation to a string
    ///
    /// # Arguments
    ///
    /// * `text` - The text to apply the operation to
    ///
    /// # Returns
    ///
    /// The resulting text after applying the operation
    pub fn apply(&self, text: &str) -> SyncResult<String> {
        self.check_expected_base_length()?;

        if text.len() != self.base_length {
            return Err(SyncError::InvalidOperation(format!(
                "Base length mismatch: expected {}, got {}",
                self.base_length,
                text.len()
            )));
        }

        let mut result = String::with_capacity(self.target_length);
        let mut chars = text.chars();

        for op in &self.ops {
            match op {
                Operation::Retain(n) => {
                    for _ in 0..*n {
                        if let Some(ch) = chars.next() {
                            result.push(ch);
                        } else {
                            return Err(SyncError::InvalidOperation(
                                "Retain beyond document length".to_string(),
                            ));
                        }
                    }
                }
                Operation::Insert(s) => {
                    result.push_str(s);
                }
                Operation::Delete(n) => {
                    for _ in 0..*n {
                        if chars.next().is_none() {
                            return Err(SyncError::InvalidOperation(
                                "Delete beyond document length".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        // Ensure we consumed the entire input
        if chars.next().is_some() {
            return Err(SyncError::InvalidOperation(
                "Operation did not consume entire document".to_string(),
            ));
        }

        Ok(result)
    }

    /// Gets the base length
    pub fn base_length(&self) -> usize {
        self.base_length
    }

    /// Gets the target length
    pub fn target_length(&self) -> usize {
        self.target_length
    }

    /// Gets the operations
    pub fn operations(&self) -> &[Operation] {
        &self.ops
    }

    /// Checks if the operation is a no-op
    pub fn is_noop(&self) -> bool {
        self.ops.is_empty() || (self.ops.len() == 1 && matches!(self.ops[0], Operation::Retain(_)))
    }
}

impl Default for TextOperation {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for TextOperation {
    fn transform(&self, other: &Self, priority: Priority) -> SyncResult<Self> {
        self.check_expected_base_length()?;
        other.check_expected_base_length()?;

        if self.base_length != other.base_length {
            return Err(SyncError::InvalidOperation(
                "Base length mismatch in transform".to_string(),
            ));
        }

        let mut result = TextOperation::new();
        let mut i1 = 0;
        let mut i2 = 0;
        let mut ops1 = self.ops.clone();
        let mut ops2 = other.ops.clone();

        while i1 < ops1.len() || i2 < ops2.len() {
            let self_has_insert = i1 < ops1.len() && matches!(ops1[i1], Operation::Insert(_));
            let other_has_insert = i2 < ops2.len() && matches!(ops2[i2], Operation::Insert(_));

            // When both sides have a simultaneous insert, `priority` decides
            // which one is ordered first in the result. `transform_pair`
            // gives opposite priorities to the two operands of a pair so
            // that both calls agree on the same insertion order -- required
            // for the standard OT convergence property (TP1).
            let self_goes_first = match priority {
                Priority::Left => self_has_insert,
                Priority::Right => self_has_insert && !other_has_insert,
            };

            if self_goes_first {
                if let Operation::Insert(s) = &ops1[i1] {
                    result.insert(s.clone());
                }
                i1 += 1;
                continue;
            }

            if other_has_insert {
                if let Operation::Insert(s) = &ops2[i2] {
                    result.retain(s.len());
                }
                i2 += 1;
                continue;
            }

            if i1 >= ops1.len() || i2 >= ops2.len() {
                break;
            }

            match (&ops1[i1], &ops2[i2]) {
                (Operation::Retain(n1), Operation::Retain(n2)) => {
                    let min = (*n1).min(*n2);
                    result.retain(min);

                    if n1 > n2 {
                        ops1[i1] = Operation::Retain(n1 - n2);
                        i2 += 1;
                    } else if n2 > n1 {
                        ops2[i2] = Operation::Retain(n2 - n1);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                (Operation::Delete(n1), Operation::Delete(n2)) => {
                    if n1 > n2 {
                        ops1[i1] = Operation::Delete(n1 - n2);
                        i2 += 1;
                    } else if n2 > n1 {
                        ops2[i2] = Operation::Delete(n2 - n1);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                (Operation::Retain(n), Operation::Delete(m)) => {
                    let min = (*n).min(*m);
                    result.delete(min);

                    if n > m {
                        ops1[i1] = Operation::Retain(n - m);
                        i2 += 1;
                    } else if m > n {
                        ops2[i2] = Operation::Delete(m - n);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                (Operation::Delete(n), Operation::Retain(m)) => {
                    let min = (*n).min(*m);
                    result.delete(min);

                    if n > m {
                        ops1[i1] = Operation::Delete(n - m);
                        i2 += 1;
                    } else if m > n {
                        ops2[i2] = Operation::Retain(m - n);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                _ => {
                    return Err(SyncError::InvalidOperation(
                        "Invalid operation combination in transform".to_string(),
                    ));
                }
            }
        }

        Ok(result)
    }

    fn compose(&self, other: &Self) -> SyncResult<Self> {
        self.check_expected_base_length()?;
        other.check_expected_base_length()?;

        if self.target_length != other.base_length {
            return Err(SyncError::InvalidOperation(
                "Target/base length mismatch in compose".to_string(),
            ));
        }

        let mut result = TextOperation::new();
        let mut i1 = 0;
        let mut i2 = 0;
        let mut ops1 = self.ops.clone();
        let mut ops2 = other.ops.clone();

        while i1 < ops1.len() || i2 < ops2.len() {
            if i1 < ops1.len() && matches!(ops1[i1], Operation::Delete(_)) {
                if let Operation::Delete(n) = ops1[i1] {
                    result.delete(n);
                }
                i1 += 1;
                continue;
            }

            if i2 < ops2.len() && matches!(ops2[i2], Operation::Insert(_)) {
                if let Operation::Insert(s) = &ops2[i2] {
                    result.insert(s.clone());
                }
                i2 += 1;
                continue;
            }

            if i1 >= ops1.len() || i2 >= ops2.len() {
                break;
            }

            match (&ops1[i1], &ops2[i2]) {
                (Operation::Retain(n1), Operation::Retain(n2)) => {
                    let min = (*n1).min(*n2);
                    result.retain(min);

                    if n1 > n2 {
                        ops1[i1] = Operation::Retain(n1 - n2);
                        i2 += 1;
                    } else if n2 > n1 {
                        ops2[i2] = Operation::Retain(n2 - n1);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                (Operation::Insert(s), Operation::Retain(n)) => {
                    let len = s.len();

                    if len <= *n {
                        result.insert(s.clone());
                        i1 += 1;
                        if *n > len {
                            ops2[i2] = Operation::Retain(n - len);
                        } else {
                            i2 += 1;
                        }
                    } else {
                        result.insert(s[..*n].to_string());
                        ops1[i1] = Operation::Insert(s[*n..].to_string());
                        i2 += 1;
                    }
                }
                (Operation::Insert(s), Operation::Delete(n)) => {
                    let len = s.len();
                    if len > *n {
                        ops1[i1] = Operation::Insert(s[*n..].to_string());
                        i2 += 1;
                    } else if *n > len {
                        ops2[i2] = Operation::Delete(n - len);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                (Operation::Retain(n1), Operation::Delete(n2)) => {
                    let min = (*n1).min(*n2);
                    result.delete(min);

                    if n1 > n2 {
                        ops1[i1] = Operation::Retain(n1 - n2);
                        i2 += 1;
                    } else if n2 > n1 {
                        ops2[i2] = Operation::Delete(n2 - n1);
                        i1 += 1;
                    } else {
                        i1 += 1;
                        i2 += 1;
                    }
                }
                _ => {
                    return Err(SyncError::InvalidOperation(
                        "Invalid operation combination in compose".to_string(),
                    ));
                }
            }
        }

        result.base_length = self.base_length;
        result.target_length = other.target_length;

        Ok(result)
    }

    fn invert(&self) -> SyncResult<Self> {
        self.check_expected_base_length()?;

        let mut result = TextOperation::new();
        result.base_length = self.target_length;
        result.target_length = self.base_length;

        for op in self.ops.iter().rev() {
            match op {
                Operation::Retain(n) => {
                    result.ops.insert(0, Operation::Retain(*n));
                }
                Operation::Insert(s) => {
                    result.ops.insert(0, Operation::Delete(s.len()));
                }
                Operation::Delete(_n) => {
                    // Note: We can't reconstruct the deleted text without additional context
                    // This is a limitation - full invert would require storing deleted content
                    result.ops.insert(0, Operation::Insert("".to_string()));
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_operation_creation() {
        let op = TextOperation::new();
        assert_eq!(op.base_length(), 0);
        assert_eq!(op.target_length(), 0);
        assert!(op.is_noop());
    }

    #[test]
    fn test_text_operation_retain() {
        let mut op = TextOperation::with_base_length(0);
        op.retain(5);
        assert_eq!(op.base_length(), 5);
        assert_eq!(op.target_length(), 5);
    }

    #[test]
    fn test_text_operation_insert() {
        let mut op = TextOperation::new();
        op.insert("hello".to_string());
        assert_eq!(op.base_length(), 0);
        assert_eq!(op.target_length(), 5);
    }

    #[test]
    fn test_text_operation_delete() {
        let mut op = TextOperation::with_base_length(0);
        op.delete(3);
        assert_eq!(op.base_length(), 3);
        assert_eq!(op.target_length(), 0);
    }

    #[test]
    fn test_text_operation_apply() -> SyncResult<()> {
        let mut op = TextOperation::with_base_length(0);
        op.insert("hello".to_string());

        let result = op.apply("")?;
        assert_eq!(result, "hello");
        Ok(())
    }

    #[test]
    fn test_text_operation_apply_complex() -> SyncResult<()> {
        // Input "hello!" has 6 characters: h-e-l-l-o-!
        // retain/delete operations add to base_length, so start with 0
        let mut op = TextOperation::new();
        op.retain(5); // Keep "hello" (base_length becomes 5)
        op.insert(", world".to_string());
        op.retain(1); // Keep "!" (base_length becomes 6)

        let result = op.apply("hello!")?;
        assert_eq!(result, "hello, world!");
        Ok(())
    }

    #[test]
    fn test_text_operation_compose() -> SyncResult<()> {
        let mut op1 = TextOperation::new();
        op1.insert("hello".to_string());

        let mut op2 = TextOperation::with_base_length(5);
        op2.retain(5);
        op2.insert(" world".to_string());

        let composed = op1.compose(&op2)?;
        let result = composed.apply("")?;
        assert_eq!(result, "hello world");
        Ok(())
    }

    /// Regression test for the `with_base_length` + `retain`/`delete`
    /// double-counting bug: `with_base_length(n)` must NOT pre-seed
    /// `base_length`/`target_length` with `n` -- those fields must start at
    /// zero and be accumulated normally by `retain`/`delete`, exactly as they
    /// are for `TextOperation::new()`. Before the fix,
    /// `with_base_length(5).retain(5)` produced `base_length == 10`.
    #[test]
    fn test_with_base_length_does_not_double_count() {
        let mut op = TextOperation::with_base_length(5);
        op.retain(5);
        assert_eq!(
            op.base_length(),
            5,
            "with_base_length(5) followed by retain(5) must accumulate to 5, not 10"
        );
        assert_eq!(op.target_length(), 5);
    }

    /// `with_base_length` records an *expectation* that is validated lazily:
    /// if the retain/delete calls added afterward don't actually sum to the
    /// declared base length, `apply()` must report a precise error instead of
    /// silently operating on the wrong length.
    #[test]
    fn test_with_base_length_mismatch_is_rejected_by_apply() {
        let mut op = TextOperation::with_base_length(5);
        op.retain(3); // only accumulates 3, but 5 was declared

        let err = op.apply("abc").expect_err(
            "declared base length of 5 must be validated against actual base length of 3",
        );
        assert!(matches!(err, SyncError::InvalidOperation(_)));
    }

    /// `with_base_length` records an *expectation* that is validated lazily:
    /// `compose()` must also reject a mismatched declaration rather than
    /// composing against the wrong length.
    #[test]
    fn test_with_base_length_mismatch_is_rejected_by_compose() {
        let mut op1 = TextOperation::new();
        op1.insert("hi".to_string());

        let mut op2 = TextOperation::with_base_length(5);
        op2.retain(2); // declared 5, only accumulated 2

        let err = op1.compose(&op2).expect_err(
            "declared base length of 5 must be validated against actual base length of 2",
        );
        assert!(matches!(err, SyncError::InvalidOperation(_)));
    }

    /// Reproduces the exact scenario from the work-order evidence:
    /// `TextOperation::with_base_length(5); op.retain(5);` used to yield
    /// `base_length == 10`, which made `compose()`'s
    /// `self.target_length != other.base_length` guard reject a legitimately
    /// composable pair of operations.
    #[test]
    fn test_with_base_length_then_retain_composes_correctly() -> SyncResult<()> {
        let mut op1 = TextOperation::new();
        op1.insert("hello".to_string());
        assert_eq!(op1.target_length(), 5);

        let mut op2 = TextOperation::with_base_length(5);
        op2.retain(5);
        assert_eq!(
            op2.base_length(),
            5,
            "retain(5) after with_base_length(5) must not double-count"
        );

        // Previously this failed with "Target/base length mismatch in compose"
        // because op2.base_length() was 10 instead of 5.
        let composed = op1.compose(&op2)?;
        assert_eq!(composed.apply("")?, "hello");
        Ok(())
    }

    /// Regression test for the OT convergence property (TP1) that
    /// `transform_pair` relies on: `Priority::Left`/`Priority::Right` must
    /// break ties consistently so that `apply(b', apply(a, s))` and
    /// `apply(a', apply(b, s))` agree, even when both operations retain
    /// content before inserting at the same conceptual position.
    #[test]
    fn test_transform_with_priority_converges_on_concurrent_inserts_after_retain() -> SyncResult<()>
    {
        use crate::ot::composer::transform_pair;

        // Both users start from "hi" and insert at the end concurrently.
        let mut a = TextOperation::new();
        a.retain(2);
        a.insert("A".to_string());

        let mut b = TextOperation::new();
        b.retain(2);
        b.insert("B".to_string());

        let (a_prime, b_prime) = transform_pair(&a, &b)?;

        let result1 = b_prime.apply(&a.apply("hi")?)?;
        let result2 = a_prime.apply(&b.apply("hi")?)?;

        assert_eq!(result1, result2);
        assert_eq!(result1, "hiAB");
        Ok(())
    }
}
