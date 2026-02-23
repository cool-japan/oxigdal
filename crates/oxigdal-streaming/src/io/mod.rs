//! Chunked I/O operations for efficient data streaming.
//!
//! This module provides chunked reading and writing capabilities
//! for efficient processing of large datasets.

pub mod buffer;
pub mod chunked;
pub mod reader;
pub mod writer;

pub use buffer::{ChunkedBuffer, ChunkDescriptor};
pub use chunked::{ChunkStrategy, ChunkedIO};
pub use reader::ChunkedReader;
pub use writer::ChunkedWriter;
