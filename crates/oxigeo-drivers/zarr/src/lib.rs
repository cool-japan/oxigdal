//! OxiGeo Zarr Driver - Pure Rust Zarr v2/v3 Support
//!
//! This crate provides a pure Rust implementation of the Zarr storage specification
//! for chunked, compressed, N-dimensional arrays. It supports both Zarr v2 and v3
//! specifications with various storage backends and compression codecs.
//!
//! # Features
//!
//! - **Zarr Versions**: Zarr v2 and v3 arrays (chunk read/write, codec and
//!   filter pipelines, v3 sharding)
//! - **Storage Backends**: Filesystem, S3, HTTP, and in-memory storage
//!   (with byte-range reads used for cloud-efficient shard access)
//! - **Compression**: Zstd, Gzip, LZ4 codecs (all Pure Rust via OxiARC),
//!   plus the v3 `crc32c` checksum codec
//! - **Filters** (opt-in cargo features): `shuffle`, `delta`, `scale-offset`
//! - **Async I/O**: Async support for cloud storage backends
//! - **Parallel**: Parallel chunk reading and writing
//! - **Caching**: LRU caching for chunks
//!
//! # Zarr Format
//!
//! Zarr is a format for the storage of chunked, compressed, N-dimensional arrays.
//! It was designed for use in parallel computing and is widely used in scientific
//! computing, particularly for earth observation and climate data.
//!
//! ## Key Concepts
//!
//! - **Array**: N-dimensional data structure
//! - **Chunk**: Fixed-size sub-array for storage
//! - **Codec**: Compression/encoding method
//! - **Filter**: Data transformation before/after codec
//! - **Group**: Hierarchical organization of arrays
//! - **Attributes**: Metadata attached to arrays/groups
//!
//! # Example - Reading a Zarr v2 Array
//!
//! ```no_run
//! # #[cfg(all(feature = "v2", feature = "filesystem"))]
//! # fn demo() -> oxigeo_zarr::Result<()> {
//! use oxigeo_zarr::{ZarrReaderV2, FilesystemStore};
//!
//! // Open a Zarr v2 array stored under the "temperature" path.
//! let store = FilesystemStore::open("data.zarr")?;
//! let reader = ZarrReaderV2::open(store, "temperature")?;
//!
//! println!("Shape: {:?}", reader.shape());
//! println!("Chunks: {:?}", reader.chunks());
//! println!("Data type: {}", reader.dtype());
//!
//! // Read one chunk (decoded element bytes in the array's stored order).
//! let chunk_data = reader.read_chunk(&[0, 0])?;
//! # let _ = chunk_data;
//! # Ok(())
//! # }
//! ```
//!
//! # Example - Writing a Zarr v2 Array
//!
//! ```no_run
//! # #[cfg(all(feature = "v2", feature = "filesystem", feature = "zstd"))]
//! # fn demo() -> oxigeo_zarr::Result<()> {
//! use oxigeo_zarr::{ZarrWriterV2, FilesystemStore};
//! use oxigeo_zarr::codecs::CompressorConfig;
//! use oxigeo_zarr::metadata::v2::ArrayMetadataV2;
//!
//! let store = FilesystemStore::create("output.zarr")?;
//! let metadata = ArrayMetadataV2::new(vec![100, 200], vec![10, 20], "<f4")
//!     .with_compressor(CompressorConfig::Zstd { level: 3 });
//!
//! let mut writer = ZarrWriterV2::create(store, "temperature", metadata)?;
//!
//! // Write one 10x20 float32 chunk.
//! let chunk_data = vec![0u8; 10 * 20 * 4];
//! writer.write_chunk(&[0, 0], &chunk_data)?;
//! writer.finalize()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Storage Backends
//!
//! ## Filesystem
//!
//! ```ignore
//! use oxigeo_zarr::FilesystemStore;
//!
//! let store = FilesystemStore::open("data.zarr")?;
//! ```
//!
//! ## S3
//!
//! ```ignore
//! use oxigeo_zarr::S3Store;
//!
//! let store = S3Store::new("bucket-name", "prefix/data.zarr").await?;
//! ```
//!
//! ## HTTP
//!
//! ```ignore
//! use oxigeo_zarr::HttpStore;
//!
//! let store = HttpStore::new("https://example.com/data.zarr")?;
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_arguments)]
// Allow dead code for internal structures
#![allow(dead_code)]
// Allow partial documentation during development
#![allow(missing_docs)]
// Allow manual div_ceil for chunk calculations
#![allow(clippy::manual_div_ceil)]
// Allow expect() for internal state invariants
#![allow(clippy::expect_used)]
// Allow complex types for zarr data structures
#![allow(clippy::type_complexity)]
// Allow collapsible match for zarr format handling
#![allow(clippy::collapsible_match)]
// Allow manual strip for path parsing
#![allow(clippy::manual_strip)]
// Allow vec push after creation for chunk building
#![allow(clippy::vec_init_then_push)]
// Allow should_implement_trait for builder patterns
#![allow(clippy::should_implement_trait)]
// Allow doc list item overindentation in complex nested lists
#![allow(clippy::doc_overindented_list_items)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod chunk;
pub mod codecs;
pub mod consolidation;
pub mod dimension;
pub mod error;
pub mod filters;
pub mod metadata;
pub mod reader;
pub mod sharding;
pub mod storage;
pub mod transformers;
pub mod writer;

// Re-export commonly used types
pub use chunk::{ChunkCoord, ChunkGrid, ChunkIndex};
pub use consolidation::{ConsolidatedMetadata, ConsolidatedStore, consolidate_metadata};
pub use dimension::{Dimension, DimensionSeparator, Shape};
pub use error::{Result, ZarrError};
pub use reader::ZarrReader;
#[cfg(feature = "v2")]
pub use reader::ZarrReaderV2;
#[cfg(feature = "v3")]
pub use reader::v3::ZarrV3Reader;
pub use storage::{Store, StoreKey};
pub use writer::ZarrWriter;
#[cfg(feature = "v2")]
pub use writer::ZarrWriterV2;
#[cfg(feature = "v3")]
pub use writer::v3::ZarrV3Writer;

#[cfg(feature = "filesystem")]
pub use storage::filesystem::FilesystemStore;

#[cfg(feature = "s3")]
pub use storage::s3::S3Storage;

#[cfg(feature = "http")]
pub use storage::http::HttpStorage;

#[cfg(feature = "memory")]
pub use storage::memory::MemoryStore;

#[cfg(feature = "cache")]
pub use storage::cache::CachingStorage;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Zarr specification version 2
pub const ZARR_VERSION_2: u8 = 2;

/// Zarr specification version 3
pub const ZARR_VERSION_3: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigeo-zarr");
    }

    #[test]
    fn test_zarr_versions() {
        assert_eq!(ZARR_VERSION_2, 2);
        assert_eq!(ZARR_VERSION_3, 3);
    }
}
