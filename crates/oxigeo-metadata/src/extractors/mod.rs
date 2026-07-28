//! Pluggable metadata extractors for specific dataset formats.

// Not gated behind `#[cfg(feature = "netcdf")]`: most of this module (CF
// global-attribute extraction, bbox/temporal-extent/grid-mapping parsing) is
// pure logic with no dependency on the real `oxigeo_netcdf` reader, and stays
// unit-testable regardless of the feature. Only the `real_dataset` inner
// module (and the real-reader branch of `NetCdfCfExtractor::extract`) is
// feature-gated internally.
pub mod netcdf_cf;
