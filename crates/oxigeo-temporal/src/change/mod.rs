//! Change Detection Module
//!
//! Implements various change detection algorithms for temporal analysis
//! including BFAST, LandTrendr, simple differencing methods, and breakpoint detection.

pub mod bfast;
pub mod breakpoint;
pub mod detection;
pub mod landtrendr;

pub use breakpoint::*;
pub use detection::*;
pub use landtrendr::{
    LandTrendrOptions, LandTrendrResult, LandTrendrSegment, LandTrendrVertex, landtrendr_segment,
};
