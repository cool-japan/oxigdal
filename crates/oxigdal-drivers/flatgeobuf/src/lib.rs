//! `FlatGeobuf` driver for `OxiGDAL`
//!
//! `FlatGeobuf` is a performant binary encoding format for geographic data.

#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod error;
pub mod geometry;
pub mod header;
pub mod index;
pub mod reader;
pub mod writer;

#[cfg(feature = "http")]
pub mod http;

// Re-export main types
pub use error::{FlatGeobufError, Result};
pub use header::{Column, ColumnType, CrsInfo, GeometryType, Header};
pub use reader::FlatGeobufReader;
pub use writer::{FlatGeobufWriter, FlatGeobufWriterBuilder};

#[cfg(feature = "async")]
pub use reader::AsyncFlatGeobufReader;

#[cfg(feature = "http")]
pub use http::HttpReader;

/// `FlatGeobuf` magic bytes
pub const MAGIC_BYTES: &[u8; 8] = b"fgb\x03fgb\x00";

/// `FlatGeobuf` version
pub const VERSION: u8 = 3;

/// Crate version
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
