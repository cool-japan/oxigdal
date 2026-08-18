//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::metadata::EnumeratedColorSpace;

/// Image information
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Number of components
    pub num_components: u16,
    /// Number of tiles
    pub num_tiles: u32,
    /// Color space
    pub color_space: Option<EnumeratedColorSpace>,
    /// Number of wavelet decomposition levels
    pub num_decomposition_levels: u8,
    /// Is JP2 format (vs raw codestream)
    pub is_jp2: bool,
}
/// Progressive decoding state
#[derive(Debug, Clone)]
pub(super) struct ProgressiveDecodingState {
    /// Current quality layer being decoded
    pub(super) current_layer: u16,
    /// Maximum quality layer available
    #[allow(dead_code)]
    pub(super) max_layers: u16,
    /// Intermediate decoded data (partial quality)
    pub(super) intermediate_data: Vec<u8>,
    /// Width of intermediate image
    #[allow(dead_code)]
    pub(super) width: usize,
    /// Height of intermediate image
    #[allow(dead_code)]
    pub(super) height: usize,
}
