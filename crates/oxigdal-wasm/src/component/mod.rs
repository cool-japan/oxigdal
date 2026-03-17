//! WASM Component Model bindings for OxiGDAL.
//!
//! This module provides a component-model-compatible API surface that works
//! with both `wasm32-wasip2` (WASM Component Model) and
//! `wasm32-unknown-unknown` (classic WASM / wasm-bindgen).
//!
//! # Design principles
//!
//! * **No C/Fortran dependencies** — all types are pure Rust and
//!   `#[no_std]`-friendly (except for the `std::collections` usage in the
//!   vector module).
//! * **Stable ABI** — discriminants and field layouts are chosen to be
//!   stable across crate versions.
//! * **Zero-copy where possible** — large data (raster pixels, WKB geometry)
//!   is kept in `Vec<u8>` and can be exposed via raw pointer exports for
//!   the host to read without an extra copy.
//!
//! # Sub-modules
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`types`] | Shared primitive types: [`ComponentBbox`], [`ComponentDataType`], [`ComponentError`], [`ImageDimensions`] |
//! | [`raster`] | [`ComponentRaster`] + [`ComponentRasterOps`] (clip, resample) |
//! | [`vector`] | [`ComponentFeature`], [`ComponentFeatureCollection`], [`PropertyValue`] |
//! | [`projection`] | [`ComponentProjection`], [`ComponentCoord`], [`ComponentTransform`] |

pub mod projection;
pub mod raster;
pub mod types;
pub mod vector;

// Re-export the most commonly used items at the module level so callers can
// write `use oxigdal_wasm::component::ComponentRaster;` without going deeper.
pub use projection::{ComponentCoord, ComponentProjection, ComponentTransform};
pub use raster::{ComponentRaster, ComponentRasterOps, RasterStats};
pub use types::{
    ComponentBbox, ComponentDataType, ComponentError, ComponentResult, ErrorCategory,
    ImageDimensions, PixelCoord,
};
pub use vector::{ComponentFeature, ComponentFeatureCollection, PropertyValue};
