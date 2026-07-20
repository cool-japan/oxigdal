//! Data filters for Zarr arrays
//!
//! This module provides filters that transform data before/after compression,
//! including shuffle, delta, and scale-offset filters.

use crate::error::Result;

/// Trait for data filters
pub trait Filter: Send + Sync {
    /// Returns the filter identifier
    fn id(&self) -> &str;

    /// Encodes (applies filter to) data
    ///
    /// # Errors
    /// Returns error if encoding fails
    fn encode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Decodes (reverses filter on) data
    ///
    /// # Errors
    /// Returns error if decoding fails
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Clones the filter
    fn clone_box(&self) -> Box<dyn Filter>;
}

/// Null filter (no-op)
#[derive(Debug, Clone)]
pub struct NullFilter;

impl Filter for NullFilter {
    fn id(&self) -> &str {
        "null"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn clone_box(&self) -> Box<dyn Filter> {
        Box::new(self.clone())
    }
}
