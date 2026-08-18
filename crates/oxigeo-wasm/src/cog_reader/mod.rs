//! WASM-specific COG Reader with async I/O
//!
//! This module provides a minimal COG reader implementation specifically
//! designed for WebAssembly environments where all I/O must be asynchronous.
//!
//! Unlike the main `oxigeo-geotiff::CogReader` which uses the synchronous
//! `DataSource` trait, this implementation directly uses async fetch methods.

pub mod constants;
pub mod functions;
pub mod functions_2;
pub mod types;
pub mod types_2;
pub mod types_3;

// Re-export all types
//
// `types::*` (IfdMetadata/OverviewMetadata) has no non-test consumer through
// this facade path today — sentinel/core.rs's tests reach it via
// `crate::cog_reader::IfdMetadata`, which only compiles under `cfg(test)` —
// so a plain `cargo check` sees it as unused. Kept `pub` for facade-path
// stability (matches the glob-reexport convention used by every other
// generated submodule here); silence the false positive rather than drop it.
#[allow(unused_imports)]
pub use types::*;
pub use types_2::*;
pub use types_3::*;
