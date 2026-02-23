//! Metadata optimization for Cloud Optimized GeoTIFF
//!
//! This module provides functionality to minimize metadata size while
//! preserving essential geospatial information.

use oxigdal_core::error::Result;

use crate::tiff::{TiffFile, TiffTag};

/// Metadata optimization result
#[derive(Debug, Clone)]
pub struct MetadataOptimization {
    /// Tags that can be removed
    pub removable_tags: Vec<u16>,
    /// Tags that can be compressed
    pub compressible_tags: Vec<u16>,
    /// Estimated space savings (bytes)
    pub estimated_savings: u64,
    /// Optimization suggestions
    pub suggestions: Vec<String>,
}

/// Metadata preservation level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreservationLevel {
    /// Minimal - only essential geospatial metadata
    Minimal,
    /// Standard - common metadata preserved
    Standard,
    /// Full - preserve all metadata
    Full,
}

/// Analyzes metadata and recommends optimization
pub fn analyze_metadata(tiff: &TiffFile, preservation: PreservationLevel) -> MetadataOptimization {
    let mut removable_tags = Vec::new();
    let mut compressible_tags = Vec::new();
    let mut estimated_savings = 0u64;
    let mut suggestions = Vec::new();

    if let Some(ifd) = tiff.ifds.first() {
        for entry in &ifd.entries {
            let tag = entry.tag;

            // Essential tags that should never be removed
            if is_essential_tag(tag) {
                continue;
            }

            // Check if tag can be removed based on preservation level
            if should_remove_tag(tag, preservation) {
                removable_tags.push(tag);
                estimated_savings += estimate_tag_size(entry.count, entry.field_type as u16);
                continue;
            }

            // Check if tag is compressible (large text fields)
            if is_compressible_tag(tag, entry.count) {
                compressible_tags.push(tag);
                let potential_savings =
                    estimate_compression_savings(entry.count, entry.field_type as u16);
                estimated_savings += potential_savings;
            }
        }
    }

    // Generate suggestions
    if !removable_tags.is_empty() {
        suggestions.push(format!(
            "Remove {} non-essential tags to save {} bytes",
            removable_tags.len(),
            estimated_savings
        ));
    }

    if !compressible_tags.is_empty() {
        suggestions.push(format!(
            "Compress {} large metadata fields",
            compressible_tags.len()
        ));
    }

    if preservation == PreservationLevel::Full {
        suggestions.push("Full preservation mode - no metadata will be removed".to_string());
    }

    MetadataOptimization {
        removable_tags,
        compressible_tags,
        estimated_savings,
        suggestions,
    }
}

/// Checks if tag is essential for COG functionality
fn is_essential_tag(tag: u16) -> bool {
    matches!(
        tag,
        256 | // ImageWidth
        257 | // ImageLength
        258 | // BitsPerSample
        259 | // Compression
        262 | // PhotometricInterpretation
        273 | // StripOffsets
        277 | // SamplesPerPixel
        278 | // RowsPerStrip
        279 | // StripByteCounts
        322 | // TileWidth
        323 | // TileLength
        324 | // TileOffsets
        325 | // TileByteCounts
        339 | // SampleFormat
        34735 | // GeoKeyDirectoryTag
        34736 | // GeoDoubleParamsTag
        34737 | // GeoAsciiParamsTag
        33550 | // ModelPixelScaleTag
        33922 | // ModelTiepointTag
        34264 // ModelTransformationTag
    )
}

/// Determines if tag should be removed based on preservation level
fn should_remove_tag(tag: u16, preservation: PreservationLevel) -> bool {
    match preservation {
        PreservationLevel::Full => false,
        PreservationLevel::Standard => is_non_standard_tag(tag),
        PreservationLevel::Minimal => !is_essential_tag(tag) && !is_standard_tag(tag),
    }
}

/// Checks if tag is part of standard TIFF/GeoTIFF
fn is_standard_tag(tag: u16) -> bool {
    // GeoTIFF tags
    if matches!(
        tag,
        33550 | 33922 | 34264 | 34735 | 34736 | 34737 | 42112 | 42113
    ) {
        return true;
    }

    // Standard TIFF tags
    tag < 32768
}

/// Checks if tag is non-standard (vendor-specific)
fn is_non_standard_tag(tag: u16) -> bool {
    // Private/vendor tags are typically >= 32768
    tag >= 32768 && !is_standard_tag(tag)
}

/// Checks if tag contains compressible data
fn is_compressible_tag(tag: u16, count: u64) -> bool {
    // ASCII strings with more than 100 characters
    if matches!(tag, 270 | 305 | 306 | 315 | 33432) && count > 100 {
        return true;
    }

    // Large arrays (>1000 elements)
    count > 1000
}

/// Estimates tag size in bytes
fn estimate_tag_size(count: u64, field_type: u16) -> u64 {
    let bytes_per_value = match field_type {
        1 | 6 | 7 => 1,   // BYTE, SBYTE, UNDEFINED
        2 => 1,           // ASCII
        3 | 8 => 2,       // SHORT, SSHORT
        4 | 9 | 11 => 4,  // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8, // RATIONAL, SRATIONAL, DOUBLE
        16 => 8,          // LONG8
        17 => 8,          // SLONG8
        18 => 16,         // IFD8
        _ => 4,
    };

    count * bytes_per_value + 12 // Tag entry overhead
}

/// Estimates compression savings for a tag
fn estimate_compression_savings(count: u64, field_type: u16) -> u64 {
    let tag_size = estimate_tag_size(count, field_type);

    // Assume 50% compression for text and arrays
    tag_size / 2
}

/// Optimizes GeoTIFF metadata keys
pub fn optimize_geokeys(tiff: &TiffFile) -> Result<GeoKeyOptimization> {
    let mut suggestions = Vec::new();
    let mut estimated_savings = 0u64;

    if let Some(ifd) = tiff.ifds.first() {
        // Check GeoKeyDirectory size
        if let Some(entry) = ifd.get_entry(TiffTag::GeoKeyDirectory) {
            if entry.count > 100 {
                suggestions.push(
                    "Large GeoKeyDirectory - consider using WKT instead of detailed parameters"
                        .to_string(),
                );
                estimated_savings += (entry.count - 50) * 2;
            }
        }

        // Check GeoAsciiParams
        if let Some(entry) = ifd.get_entry(TiffTag::GeoAsciiParams) {
            if entry.count > 500 {
                suggestions.push(format!(
                    "Large GeoAsciiParams ({} bytes) - consider trimming or using EPSG codes",
                    entry.count
                ));
                estimated_savings += entry.count / 2;
            }
        }

        // Check for redundant geotags
        let has_model_tiepoint = ifd.get_entry(TiffTag::ModelTiepoint).is_some();
        let has_model_transform = ifd.get_entry(TiffTag::ModelTransformation).is_some();

        if has_model_tiepoint && has_model_transform {
            suggestions.push(
                "Both ModelTiepoint and ModelTransformation present - one may be redundant"
                    .to_string(),
            );
        }
    }

    Ok(GeoKeyOptimization {
        suggestions,
        estimated_savings,
    })
}

/// GeoKey optimization result
#[derive(Debug, Clone)]
pub struct GeoKeyOptimization {
    /// Optimization suggestions
    pub suggestions: Vec<String>,
    /// Estimated space savings (bytes)
    pub estimated_savings: u64,
}

/// Removes redundant TIFF tags
pub fn find_redundant_tags(tiff: &TiffFile) -> Vec<u16> {
    let mut redundant = Vec::new();

    if let Some(ifd) = tiff.ifds.first() {
        // Check for tags that are mutually exclusive or redundant

        // Strip tags in tiled images
        let is_tiled = ifd.get_entry(TiffTag::TileWidth).is_some();
        if is_tiled {
            if ifd.get_entry(TiffTag::RowsPerStrip).is_some() {
                redundant.push(TiffTag::RowsPerStrip as u16);
            }
            if ifd.get_entry(TiffTag::StripOffsets).is_some() {
                redundant.push(TiffTag::StripOffsets as u16);
            }
            if ifd.get_entry(TiffTag::StripByteCounts).is_some() {
                redundant.push(TiffTag::StripByteCounts as u16);
            }
        }

        // Check for default values that don't need to be stored
        if let Some(entry) = ifd.get_entry(TiffTag::PlanarConfiguration) {
            if let Ok(value) = entry.get_u64(tiff.byte_order()) {
                if value == 1 {
                    // Default is Chunky
                    redundant.push(TiffTag::PlanarConfiguration as u16);
                }
            }
        }

        if let Some(entry) = ifd.get_entry(TiffTag::ResolutionUnit) {
            if let Ok(value) = entry.get_u64(tiff.byte_order()) {
                if value == 2 {
                    // Default is inches
                    redundant.push(TiffTag::ResolutionUnit as u16);
                }
            }
        }
    }

    redundant
}

/// Compresses ASCII metadata fields
pub fn compress_ascii_fields(text: &str) -> String {
    // Remove excessive whitespace
    let compressed = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // Collapse multiple spaces to single space
    let mut result = String::with_capacity(compressed.len());
    let mut prev_space = false;
    for ch in compressed.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(ch);
                prev_space = true;
            }
        } else {
            result.push(ch);
            prev_space = false;
        }
    }

    // Limit length if excessively long
    if result.len() > 1000 {
        format!("{}...", &result[..997])
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_essential_tags() {
        assert!(is_essential_tag(256)); // ImageWidth
        assert!(is_essential_tag(257)); // ImageLength
        assert!(is_essential_tag(322)); // TileWidth
        assert!(!is_essential_tag(270)); // ImageDescription
    }

    #[test]
    fn test_standard_tags() {
        assert!(is_standard_tag(256)); // ImageWidth
        assert!(is_standard_tag(34735)); // GeoKeyDirectoryTag
        assert!(!is_standard_tag(50000)); // Some private tag
    }

    #[test]
    fn test_tag_size_estimation() {
        assert_eq!(estimate_tag_size(1, 4), 16); // 1 LONG + overhead
        assert_eq!(estimate_tag_size(10, 2), 22); // 10 ASCII + overhead
    }

    #[test]
    fn test_ascii_compression() {
        let text = "This is    a test\n   with   extra   whitespace   \n\n   and empty lines";
        let compressed = compress_ascii_fields(text);
        assert!(compressed.len() < text.len());
        assert!(!compressed.contains("\n\n"));
        assert!(!compressed.contains("   "));
    }

    #[test]
    fn test_ascii_truncation() {
        let long_text = "x".repeat(2000);
        let compressed = compress_ascii_fields(&long_text);
        assert!(compressed.len() <= 1000);
        assert!(compressed.ends_with("..."));
    }

    #[test]
    fn test_compressible_tag_detection() {
        assert!(is_compressible_tag(270, 200)); // ImageDescription with 200 chars
        assert!(!is_compressible_tag(270, 50)); // Short description
        assert!(is_compressible_tag(33550, 2000)); // Large array
    }

    #[test]
    fn test_preservation_levels() {
        assert!(!should_remove_tag(256, PreservationLevel::Minimal)); // Essential
        assert!(!should_remove_tag(270, PreservationLevel::Full)); // Never remove in Full
        assert!(should_remove_tag(50000, PreservationLevel::Minimal)); // Private tag
    }
}
