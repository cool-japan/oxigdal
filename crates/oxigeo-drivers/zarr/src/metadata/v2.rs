//! Zarr v2 metadata structures
//!
//! This module provides metadata types specific to Zarr v2 specification.

use super::ArrayOrder;
use crate::codecs::CompressorConfig;
use serde::{Deserialize, Serialize};

/// Zarr v2 array metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrayMetadataV2 {
    /// Array shape
    pub shape: Vec<usize>,
    /// Chunk shape
    pub chunks: Vec<usize>,
    /// Data type
    pub dtype: String,
    /// Compressor configuration
    pub compressor: Option<CompressorConfig>,
    /// Fill value
    pub fill_value: serde_json::Value,
    /// Array order
    pub order: ArrayOrder,
    /// Filters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<serde_json::Value>>,
    /// Zarr format version (always 2)
    pub zarr_format: u8,
}

impl ArrayMetadataV2 {
    /// Creates new v2 array metadata
    #[must_use]
    pub fn new(shape: Vec<usize>, chunks: Vec<usize>, dtype: impl Into<String>) -> Self {
        Self {
            shape,
            chunks,
            dtype: dtype.into(),
            compressor: None,
            fill_value: serde_json::Value::Null,
            order: ArrayOrder::C,
            filters: None,
            zarr_format: 2,
        }
    }
}
