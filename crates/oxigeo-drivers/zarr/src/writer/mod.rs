//! Zarr array writers
//!
//! This module provides writers for Zarr v2 and v3 arrays.

#[cfg(feature = "v3")]
pub mod v3;

use crate::error::Result;
use crate::storage::Store;

#[cfg(feature = "v3")]
pub use v3::ZarrV3Writer;

/// Zarr writer trait
pub trait ZarrWriter {
    /// Writes a chunk
    ///
    /// # Errors
    /// Returns error if chunk cannot be written
    fn write_chunk(&mut self, coords: &[usize], data: &[u8]) -> Result<()>;

    /// Finalizes the write
    ///
    /// # Errors
    /// Returns error if finalization fails
    fn finalize(&mut self) -> Result<()>;
}

/// Zarr v2 writer
pub struct ZarrWriterV2<S: Store> {
    _store: S,
}

impl<S: Store> ZarrWriterV2<S> {
    /// Creates a new v2 writer
    #[must_use]
    pub fn new(store: S) -> Self {
        Self { _store: store }
    }
}
