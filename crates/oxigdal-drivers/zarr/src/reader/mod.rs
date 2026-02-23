//! Zarr array readers
//!
//! This module provides readers for Zarr v2 and v3 arrays.

#[cfg(feature = "v3")]
pub mod v3;

use crate::error::Result;
use crate::storage::Store;

#[cfg(feature = "v3")]
pub use v3::ZarrV3Reader;

/// Zarr reader trait
pub trait ZarrReader {
    /// Returns the array shape
    fn shape(&self) -> &[usize];

    /// Returns the chunk shape
    fn chunks(&self) -> &[usize];

    /// Reads a chunk
    ///
    /// # Errors
    /// Returns error if chunk cannot be read
    fn read_chunk(&self, coords: &[usize]) -> Result<Vec<u8>>;
}

/// Zarr v2 reader
pub struct ZarrReaderV2<S: Store> {
    _store: S,
}

impl<S: Store> ZarrReaderV2<S> {
    /// Creates a new v2 reader
    #[must_use]
    pub fn new(store: S) -> Self {
        Self { _store: store }
    }
}
