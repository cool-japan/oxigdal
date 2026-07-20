//! Command implementations for OxiGeo CLI

pub mod buildvrt;
pub mod calc;
pub mod clip;
pub mod contour;
pub mod convert;
pub mod dem;
pub mod fillnodata;
pub mod info;
pub mod inspect;
pub mod merge;
pub mod polygonize;
pub mod profile;
pub mod proximity;
pub mod rasterize;
pub mod reproject;
pub mod sieve;
pub mod stats;
#[cfg(test)]
pub(crate) mod test_fixtures;
pub mod tileindex;
pub mod translate;
pub mod validate;
pub mod warp;
