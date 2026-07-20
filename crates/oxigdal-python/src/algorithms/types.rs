//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

/// Configuration parameters for Enhanced Vegetation Index (EVI) calculation
#[derive(Debug, Clone, Copy)]
pub struct EviConfig {
    /// Gain factor (default: 2.5)
    pub g: f64,
    /// Coefficient for aerosol resistance (default: 6.0)
    pub c1: f64,
    /// Coefficient for aerosol resistance (default: 7.5)
    pub c2: f64,
    /// Soil adjustment factor (default: 1.0)
    pub l: f64,
}
/// Boundary handling mode for [`convolve_with_boundary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConvBoundary {
    /// Mirror samples across the edge (scipy.ndimage-style half-sample
    /// symmetric reflection: `d c b a | a b c d | d c b a`).
    Reflect,
    /// Use a constant `fill_value` for out-of-range samples.
    Constant,
    /// Replicate the nearest edge sample (`a a a | a b c d | d d d`).
    Nearest,
    /// Wrap around to the opposite edge (`a b c d | a b c d | a b c d`).
    Wrap,
}
