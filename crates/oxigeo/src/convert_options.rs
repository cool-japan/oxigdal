//! Options accepted by [`Dataset::convert`](crate::Dataset::convert).
//!
//! The conversion itself lives in [`crate::convert_ops`]; these are the knobs
//! it reads.  Kept in their own module so the option surface can grow without
//! pushing the conversion implementation past the house file-size limit.

/// Output compression codec for [`Dataset::convert`](crate::Dataset::convert).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// No compression (default).
    #[default]
    None,
    /// DEFLATE / zlib compression.
    Deflate,
    /// LZW compression.
    Lzw,
    /// PackBits run-length encoding.
    PackBits,
    /// ZSTD compression (not universally supported).
    Zstd,
}

/// Options controlling [`Dataset::convert`](crate::Dataset::convert).
///
/// All fields are optional — `ConversionOptions::default()` produces
/// a lossless identity conversion.
#[derive(Debug, Clone, Default)]
pub struct ConversionOptions {
    /// Output compression codec.  Defaults to [`Compression::None`].
    pub compression: Option<Compression>,
    /// Compression level 0–9 (format-specific meaning).
    pub compression_level: Option<u8>,
    /// Write as Cloud-Optimized GeoTIFF (COG) when `true`.
    pub cog: bool,
    /// Overview decimation factors to embed (e.g. `[2, 4, 8, 16]`).
    pub overviews: Vec<u32>,
    /// Output tile size in pixels (square); uses strip layout when `None`.
    pub tile_size: Option<u32>,
    /// Arbitrary driver creation options (e.g. `("PHOTOMETRIC", "RGB")`).
    pub creation_options: Vec<(String, String)>,
}
