//! GeoSentinel — in-browser Sentinel-2 change detection.
//!
//! Modules (filled in by the GeoSentinel lane work packages):
//!
//! - [`utm`] — self-contained UTM ↔ WGS84 conversion (Krüger series, WP A2)
//! - [`stac`] — Earth Search STAC pairing client (`WasmStacClient`, WP A3)
//! - [`core`] — `GeoSentinel` scene management and JS-facing bindings (WP A4)
//! - [`pipeline`] — pure NDVI-difference change-detection core (WP A4)
//!
//! This module tree is registered by WP W0; the submodules are
//! intentionally empty stubs until their owning work packages land.

pub mod core;
pub mod pipeline;
pub mod stac;
pub mod utm;
