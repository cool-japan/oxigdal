//! Array metadata structures
//!
//! This module provides common array metadata types.

use super::{ArrayOrder, dtype::DType};
use crate::dimension::Shape;
use serde::{Deserialize, Serialize};

/// Generic array metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrayMetadata {
    /// Array shape
    pub shape: Vec<usize>,
    /// Chunk shape
    pub chunks: Vec<usize>,
    /// Data type
    pub dtype: DType,
    /// Fill value
    pub fill_value: serde_json::Value,
    /// Array order (C or F)
    pub order: ArrayOrder,
}

impl ArrayMetadata {
    /// Creates new array metadata
    #[must_use]
    pub fn new(shape: Vec<usize>, chunks: Vec<usize>, dtype: DType) -> Self {
        Self {
            shape,
            chunks,
            dtype,
            fill_value: serde_json::Value::Null,
            order: ArrayOrder::C,
        }
    }

    /// Returns the array shape
    #[must_use]
    pub fn shape(&self) -> Shape {
        Shape::new_unchecked(self.shape.clone())
    }

    /// Returns the chunk shape
    #[must_use]
    pub fn chunk_shape(&self) -> Shape {
        Shape::new_unchecked(self.chunks.clone())
    }
}
