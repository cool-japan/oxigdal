//! Change Detection Module
//!
//! Implements various change detection algorithms for temporal analysis
//! including BFAST, LandTrendr, simple differencing methods, and breakpoint detection.

pub mod breakpoint;
pub mod detection;

pub use breakpoint::*;
pub use detection::*;
