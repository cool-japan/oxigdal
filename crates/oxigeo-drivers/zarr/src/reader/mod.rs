//! Zarr array readers
//!
//! This module provides readers for Zarr v2 and v3 arrays.

#[cfg(feature = "v2")]
pub mod v2;

#[cfg(feature = "v3")]
pub mod v3;

use crate::error::Result;

#[cfg(feature = "v2")]
pub use v2::ZarrReaderV2;

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
