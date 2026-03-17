//! Pure Rust JPEG2000 (JP2/J2K) driver for OxiGDAL
//!
//! This crate provides a Pure Rust implementation of JPEG2000 image decoding,
//! supporting both JP2 (JPEG2000 Part 1) and raw J2K codestream formats.
//!
//! # Features
//!
//! - Pure Rust implementation (no C/C++ dependencies)
//! - **Full JP2 box structure parsing** with support for all standard boxes
//! - JPEG2000 codestream decoding
//! - Wavelet transforms (5/3 reversible and 9/7 irreversible)
//! - Multi-component images (RGB, RGBA, grayscale)
//! - Tiling support with partial tile decoding
//! - **Complete metadata extraction** (file type, resolution, color spec, XML, UUID)
//! - **Error resilience modes** for handling corrupted files (Basic and Full)
//! - **Progressive decoding** with quality layer support
//! - **Region of Interest (ROI) decoding** for spatial and resolution-level extraction
//!
//! # Limitations
//!
//! This is a reference implementation with simplified decoding for common cases.
//! Full JPEG2000 compliance (especially tier-1 EBCOT encoding) requires extensive
//! additional implementation. For production use with complex JPEG2000 files,
//! consider using a more complete decoder.
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```no_run
//! use oxigdal_jpeg2000::Jpeg2000Reader;
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("image.jp2")?;
//! let reader = BufReader::new(file);
//!
//! let mut decoder = Jpeg2000Reader::new(reader)?;
//! decoder.parse_headers()?;
//!
//! let width = decoder.width()?;
//! let height = decoder.height()?;
//! println!("Image size: {}x{}", width, height);
//!
//! let info = decoder.info()?;
//! println!("Color space: {:?}", info.color_space);
//! println!("Decomposition levels: {}", info.num_decomposition_levels);
//!
//! // Access metadata
//! if let Some(res) = decoder.capture_resolution_dpi() {
//!     println!("Resolution: {:.1} x {:.1} DPI", res.0, res.1);
//! }
//!
//! // Note: Full decoding not yet implemented
//! // let rgb_data = decoder.decode_rgb()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Progressive Decoding
//!
//! ```no_run
//! use oxigdal_jpeg2000::Jpeg2000Reader;
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("image.jp2")?;
//! let reader = BufReader::new(file);
//! let mut decoder = Jpeg2000Reader::new(reader)?;
//! decoder.parse_headers()?;
//!
//! // Decode progressively layer by layer
//! let mut progressive = decoder.decode_progressive()?;
//! while let Some(image_data) = progressive.next_layer()? {
//!     println!("Decoded layer {} of {}",
//!              progressive.current_layer(),
//!              progressive.total_layers());
//!     // Display or process intermediate quality image
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Region of Interest Decoding
//!
//! ```no_run
//! use oxigdal_jpeg2000::Jpeg2000Reader;
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("image.jp2")?;
//! let reader = BufReader::new(file);
//! let mut decoder = Jpeg2000Reader::new(reader)?;
//! decoder.parse_headers()?;
//!
//! // Decode only a specific region (more efficient than full decode)
//! let region = decoder.decode_region(100, 100, 256, 256)?;
//!
//! // Decode at lower resolution for thumbnail
//! let thumbnail = decoder.decode_region_at_resolution(0, 0, 64, 64, 2)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Error Resilience
//!
//! ```no_run
//! use oxigdal_jpeg2000::{Jpeg2000Reader, ResilienceMode};
//! use std::fs::File;
//! use std::io::BufReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("corrupted.jp2")?;
//! let reader = BufReader::new(file);
//! let mut decoder = Jpeg2000Reader::new(reader)?;
//!
//! // Enable error resilience for corrupted files
//! decoder.enable_full_error_resilience();
//!
//! // Parser will attempt to recover from errors
//! decoder.parse_headers()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! The decoder is organized into several layers:
//!
//! - **Box Reader** ([`box_reader`]): Parses JP2 box structure
//! - **Codestream** ([`codestream`]): Parses JPEG2000 codestream markers
//! - **Tier-2** ([`tier2`]): Packet decoding and layer management
//! - **Tier-1** ([`tier1`]): Code-block decoding (EBCOT)
//! - **Wavelet** ([`wavelet`]): Inverse wavelet transforms
//! - **Color** ([`color`]): Color space conversions
//! - **Metadata** ([`metadata`]): JP2 metadata boxes
//! - **Reader** ([`reader`]): High-level decoding interface
//!
//! # JPEG2000 Standard
//!
//! JPEG2000 is defined in ISO/IEC 15444-1:2019. This implementation follows
//! the standard for basic decoding functionality.
//!
//! # Performance Considerations
//!
//! - Wavelet transforms are implemented with minimal optimizations
//! - SIMD optimizations are not yet implemented
//! - Memory usage is not optimized for large images
//! - For high-performance applications, consider using native implementations
//!
//! # TODO
//!
//! - Complete tier-1 EBCOT decoder implementation (currently placeholder)
//! - Add writing/encoding support for JP2/J2K files
//! - SIMD optimization for wavelet transforms
//! - Parallel tile decoding with multi-threading support
//! - JPX (JPEG2000 Part 2) extended features
//! - Memory-mapped file support for large images
//!
//! # Recently Implemented
//!
//! - ✅ Full JP2 format support (all standard boxes, complete metadata parsing)
//! - ✅ Error resilience modes (None, Basic, Full) with packet-level error handling
//! - ✅ Progressive decoding with quality layer support
//! - ✅ ROI decoding support (spatial regions and resolution levels)

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod box_reader;
pub mod codestream;
pub mod color;
pub mod error;
pub mod jp2_boxes;
pub mod metadata;
pub mod reader;
pub mod tier1;
pub mod tier2;
pub mod wavelet;

// Re-exports
pub use error::{Jpeg2000Error, ResilienceMode, Result};
pub use jp2_boxes::{BoxType as Jp2BoxType, ColorSpace, Jp2Box, Jp2Parser};
pub use reader::{ImageInfo, Jpeg2000Reader, ProgressiveDecoder};
pub use tier2::progression::{CodeBlockAddress, ProgressionIterator};
pub use tier2::rate_control::{QualityLayer, RateController, SlopeEntry};
pub use tier2::roi::{RoiMap, RoiShift};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if a file is likely a JP2 file based on magic bytes
///
/// # Example
///
/// ```
/// use oxigdal_jpeg2000::is_jp2;
/// use std::io::Cursor;
///
/// let data = vec![
///     0x00, 0x00, 0x00, 0x0C,
///     0x6A, 0x50, 0x20, 0x20,
///     0x0D, 0x0A, 0x87, 0x0A,
/// ];
///
/// assert!(is_jp2(&mut Cursor::new(data)).expect("check failed"));
/// ```
pub fn is_jp2<R: std::io::Read>(reader: &mut R) -> std::io::Result<bool> {
    let mut magic = [0u8; 12];
    match reader.read_exact(&mut magic) {
        Ok(()) => Ok(&magic[4..8] == b"jP  " && magic[8..12] == [0x0D, 0x0A, 0x87, 0x0A]),
        Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

/// Check if a file is likely a J2K codestream based on SOC marker
///
/// # Example
///
/// ```
/// use oxigdal_jpeg2000::is_j2k;
/// use std::io::Cursor;
///
/// let data = vec![0xFF, 0x4F]; // SOC marker
///
/// assert!(is_j2k(&mut Cursor::new(data)).expect("check failed"));
/// ```
pub fn is_j2k<R: std::io::Read>(reader: &mut R) -> std::io::Result<bool> {
    let mut magic = [0u8; 2];
    reader.read_exact(&mut magic)?;

    Ok(magic == [0xFF, 0x4F])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_is_jp2() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];

        let mut cursor = Cursor::new(data);
        assert!(is_jp2(&mut cursor).expect("check failed"));
    }

    #[test]
    fn test_is_j2k() {
        let data = vec![0xFF, 0x4F];

        let mut cursor = Cursor::new(data);
        assert!(is_j2k(&mut cursor).expect("check failed"));
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
