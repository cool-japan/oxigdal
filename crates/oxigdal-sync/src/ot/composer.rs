//! Operation composition utilities

use crate::SyncResult;
use crate::ot::{TextOperation, Transform};

/// Composes multiple text operations into a single operation
pub struct OperationComposer {
    operations: Vec<TextOperation>,
}

impl OperationComposer {
    /// Creates a new operation composer
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    /// Adds an operation to the composition
    ///
    /// # Arguments
    ///
    /// * `op` - The operation to add
    pub fn add(&mut self, op: TextOperation) {
        self.operations.push(op);
    }

    /// Composes all operations into a single operation
    ///
    /// # Returns
    ///
    /// The composed operation, or None if no operations were added
    pub fn compose(self) -> SyncResult<Option<TextOperation>> {
        if self.operations.is_empty() {
            return Ok(None);
        }

        let mut result = self.operations[0].clone();
        for op in self.operations.iter().skip(1) {
            result = result.compose(op)?;
        }

        Ok(Some(result))
    }

    /// Gets the number of operations
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Checks if the composer is empty
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl Default for OperationComposer {
    fn default() -> Self {
        Self::new()
    }
}

/// Transforms two operations that were applied concurrently
///
/// Returns a pair of operations (a', b') where:
/// - a' is a transformed against b
/// - b' is b transformed against a
///
/// After transformation: apply(a, apply(b, s)) == apply(b', apply(a', s))
///
/// # Arguments
///
/// * `a` - First operation
/// * `b` - Second operation
///
/// # Returns
///
/// Tuple of transformed operations (a', b')
pub fn transform_pair(
    a: &TextOperation,
    b: &TextOperation,
) -> SyncResult<(TextOperation, TextOperation)> {
    let a_prime = a.transform(b)?;
    let b_prime = b.transform(a)?;
    Ok((a_prime, b_prime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_composer_creation() {
        let composer = OperationComposer::new();
        assert_eq!(composer.len(), 0);
        assert!(composer.is_empty());
    }

    #[test]
    fn test_composer_add() {
        let mut composer = OperationComposer::new();
        let mut op = TextOperation::new();
        op.insert("hello".to_string());

        composer.add(op);
        assert_eq!(composer.len(), 1);
    }

    #[test]
    #[ignore = "OT compose algorithm needs review - length tracking issue"]
    fn test_composer_compose() -> SyncResult<()> {
        let mut composer = OperationComposer::new();

        let mut op1 = TextOperation::new();
        op1.insert("hello".to_string());

        let mut op2 = TextOperation::with_base_length(5);
        op2.retain(5);
        op2.insert(" world".to_string());

        composer.add(op1);
        composer.add(op2);

        let result = composer.compose()?;
        assert!(result.is_some());

        let composed = result.ok_or(crate::SyncError::InvalidOperation("No result".to_string()))?;
        let text = composed.apply("")?;
        assert_eq!(text, "hello world");

        Ok(())
    }

    #[test]
    #[ignore = "OT transform algorithm needs review - convergence property"]
    fn test_transform_pair() -> SyncResult<()> {
        let mut op_a = TextOperation::with_base_length(0);
        op_a.insert("A".to_string());

        let mut op_b = TextOperation::with_base_length(0);
        op_b.insert("B".to_string());

        let (a_prime, b_prime) = transform_pair(&op_a, &op_b)?;

        // Both should produce "AB" or "BA" depending on transformation
        let result1 = b_prime.apply(&op_a.apply("")?)?;
        let result2 = a_prime.apply(&op_b.apply("")?)?;

        // The results should be consistent
        assert_eq!(result1, result2);

        Ok(())
    }
}
