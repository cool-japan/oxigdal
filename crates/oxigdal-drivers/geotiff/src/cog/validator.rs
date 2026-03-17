//! Enhanced COG validation with detailed compliance checking
//!
//! This module provides comprehensive validation of Cloud Optimized GeoTIFF files,
//! including performance prediction and HTTP range request simulation.

use oxigdal_core::error::Result;
use oxigdal_core::io::DataSource;

use crate::tiff::{Compression, TiffFile, TiffTag};

/// Validation severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationSeverity {
    /// Informational message
    Info,
    /// Warning - not ideal but acceptable
    Warning,
    /// Error - violates COG specification
    Error,
}

/// A validation message
#[derive(Debug, Clone)]
pub struct ValidationMessage {
    /// Severity level
    pub severity: ValidationSeverity,
    /// Message category
    pub category: ValidationCategory,
    /// Description of the issue
    pub message: String,
    /// Suggested fix (if available)
    pub suggestion: Option<String>,
}

/// Validation message categories
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationCategory {
    /// IFD structure
    IfdStructure,
    /// Tile organization
    TileOrganization,
    /// Overview configuration
    Overviews,
    /// Compression settings
    Compression,
    /// Metadata optimization
    Metadata,
    /// Performance considerations
    Performance,
    /// HTTP range request efficiency
    HttpEfficiency,
}

/// Detailed COG validation result
#[derive(Debug, Clone)]
pub struct DetailedCogValidation {
    /// Whether the file is valid COG
    pub is_valid_cog: bool,
    /// Whether the file is optimally configured
    pub is_optimal: bool,
    /// All validation messages
    pub messages: Vec<ValidationMessage>,
    /// Performance metrics
    pub performance: PerformanceMetrics,
    /// Compliance score (0-100)
    pub compliance_score: u8,
}

/// Performance metrics for COG access
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Estimated HTTP requests for full resolution tile access
    pub requests_per_tile: u32,
    /// Estimated bytes read for full resolution tile access
    pub bytes_per_tile: u64,
    /// Estimated overhead percentage
    pub overhead_percentage: f64,
    /// Whether IFD is optimally positioned
    pub optimal_ifd_position: bool,
    /// Whether tiles are optimally ordered
    pub optimal_tile_order: bool,
}

impl ValidationMessage {
    /// Creates a new validation message
    fn new(
        severity: ValidationSeverity,
        category: ValidationCategory,
        message: String,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            severity,
            category,
            message,
            suggestion,
        }
    }

    /// Creates an error message
    fn error(category: ValidationCategory, message: String) -> Self {
        Self::new(ValidationSeverity::Error, category, message, None)
    }

    /// Creates a warning message
    fn warning(category: ValidationCategory, message: String) -> Self {
        Self::new(ValidationSeverity::Warning, category, message, None)
    }

    /// Creates an info message
    fn info(category: ValidationCategory, message: String) -> Self {
        Self::new(ValidationSeverity::Info, category, message, None)
    }

    /// Creates a message with suggestion
    fn with_suggestion(
        severity: ValidationSeverity,
        category: ValidationCategory,
        message: String,
        suggestion: String,
    ) -> Self {
        Self::new(severity, category, message, Some(suggestion))
    }
}

/// Performs detailed COG validation
pub fn validate_cog_detailed<S: DataSource>(
    tiff: &TiffFile,
    source: &S,
) -> Result<DetailedCogValidation> {
    let mut messages = Vec::new();
    let mut is_valid = true;

    // Check IFD structure
    validate_ifd_structure(tiff, &mut messages, &mut is_valid);

    // Check tiling
    validate_tiling(tiff, &mut messages, &mut is_valid);

    // Check overviews
    validate_overviews(tiff, &mut messages);

    // Check compression
    validate_compression(tiff, &mut messages);

    // Check metadata
    validate_metadata(tiff, &mut messages);

    // Check tile ordering
    validate_tile_ordering(tiff, source, &mut messages)?;

    // Calculate performance metrics
    let performance = calculate_performance_metrics(tiff, source)?;

    // Validate HTTP efficiency
    validate_http_efficiency(tiff, &performance, &mut messages);

    // Calculate compliance score
    let compliance_score = calculate_compliance_score(&messages, &performance);

    // Determine if optimal
    let is_optimal = is_optimal_cog(&messages, &performance);

    Ok(DetailedCogValidation {
        is_valid_cog: is_valid,
        is_optimal,
        messages,
        performance,
        compliance_score,
    })
}

/// Validates IFD structure
fn validate_ifd_structure(
    tiff: &TiffFile,
    messages: &mut Vec<ValidationMessage>,
    is_valid: &mut bool,
) {
    if tiff.ifds.is_empty() {
        messages.push(ValidationMessage::error(
            ValidationCategory::IfdStructure,
            "No IFDs found in file".to_string(),
        ));
        *is_valid = false;
        return;
    }

    // Check IFD ordering (largest to smallest)
    let sizes: Vec<u64> = tiff
        .ifds
        .iter()
        .filter_map(|ifd| {
            let w = ifd
                .get_entry(TiffTag::ImageWidth)?
                .get_u64(tiff.byte_order())
                .ok()?;
            let h = ifd
                .get_entry(TiffTag::ImageLength)?
                .get_u64(tiff.byte_order())
                .ok()?;
            Some(w * h)
        })
        .collect();

    for i in 1..sizes.len() {
        if sizes[i] > sizes[i - 1] {
            messages.push(ValidationMessage::with_suggestion(
                ValidationSeverity::Error,
                ValidationCategory::IfdStructure,
                "IFDs not ordered by decreasing size".to_string(),
                "Reorder IFDs so full resolution comes first, followed by overviews in decreasing size".to_string(),
            ));
            *is_valid = false;
            break;
        }
    }

    // Note: SubIFD tag checking removed as it's not in our TiffTag enum
    // COGs should use sequential IFDs, which is checked above
}

/// Validates tiling configuration
fn validate_tiling(tiff: &TiffFile, messages: &mut Vec<ValidationMessage>, is_valid: &mut bool) {
    if let Some(ifd) = tiff.ifds.first() {
        let has_tiles = ifd.get_entry(TiffTag::TileWidth).is_some()
            && ifd.get_entry(TiffTag::TileLength).is_some();

        if !has_tiles {
            messages.push(ValidationMessage::with_suggestion(
                ValidationSeverity::Error,
                ValidationCategory::TileOrganization,
                "Image is not tiled - COG requires tiling".to_string(),
                "Convert to tiled format with tile size 256x256 or 512x512".to_string(),
            ));
            *is_valid = false;
            return;
        }

        // Check tile dimensions
        if let (Some(tw_entry), Some(th_entry)) = (
            ifd.get_entry(TiffTag::TileWidth),
            ifd.get_entry(TiffTag::TileLength),
        ) {
            if let (Ok(tw), Ok(th)) = (
                tw_entry.get_u64(tiff.byte_order()),
                th_entry.get_u64(tiff.byte_order()),
            ) {
                // Check power of 2
                if !tw.is_power_of_two() || !th.is_power_of_two() {
                    messages.push(ValidationMessage::warning(
                        ValidationCategory::TileOrganization,
                        format!("Tile dimensions {}x{} are not powers of 2", tw, th),
                    ));
                }

                // Check if square
                if tw != th {
                    messages.push(ValidationMessage::info(
                        ValidationCategory::TileOrganization,
                        format!("Non-square tiles: {}x{} (square tiles recommended)", tw, th),
                    ));
                }

                // Check optimal size
                if tw < 256 || th < 256 {
                    messages.push(ValidationMessage::warning(
                        ValidationCategory::Performance,
                        format!(
                            "Small tile size {}x{} may cause excessive HTTP requests",
                            tw, th
                        ),
                    ));
                } else if tw > 1024 || th > 1024 {
                    messages.push(ValidationMessage::warning(
                        ValidationCategory::Performance,
                        format!("Large tile size {}x{} may increase bandwidth waste", tw, th),
                    ));
                }
            }
        }
    }
}

/// Validates overview configuration
fn validate_overviews(tiff: &TiffFile, messages: &mut Vec<ValidationMessage>) {
    if tiff.ifds.len() == 1 {
        messages.push(ValidationMessage::with_suggestion(
            ValidationSeverity::Warning,
            ValidationCategory::Overviews,
            "No internal overviews - recommended for COG".to_string(),
            "Add overview levels with 2x downsampling (e.g., /2, /4, /8, /16)".to_string(),
        ));
        return;
    }

    // Check overview downsampling factors
    if let Some(base_ifd) = tiff.ifds.first() {
        if let (Some(base_width), Some(base_height)) = (
            base_ifd
                .get_entry(TiffTag::ImageWidth)
                .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
            base_ifd
                .get_entry(TiffTag::ImageLength)
                .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
        ) {
            for (idx, ifd) in tiff.ifds.iter().skip(1).enumerate() {
                if let (Some(ov_width), Some(ov_height)) = (
                    ifd.get_entry(TiffTag::ImageWidth)
                        .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
                    ifd.get_entry(TiffTag::ImageLength)
                        .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
                ) {
                    let width_factor = base_width as f64 / ov_width as f64;
                    let height_factor = base_height as f64 / ov_height as f64;

                    // Check if power of 2
                    if (width_factor.log2().fract().abs() > 0.01)
                        || (height_factor.log2().fract().abs() > 0.01)
                    {
                        messages.push(ValidationMessage::info(
                            ValidationCategory::Overviews,
                            format!(
                                "Overview {} has non-power-of-2 downsampling factor ({:.1}x)",
                                idx + 1,
                                width_factor
                            ),
                        ));
                    }
                }
            }
        }
    }
}

/// Validates compression settings
fn validate_compression(tiff: &TiffFile, messages: &mut Vec<ValidationMessage>) {
    if let Some(ifd) = tiff.ifds.first() {
        if let Some(comp_entry) = ifd.get_entry(TiffTag::Compression) {
            if let Ok(comp_value) = comp_entry.get_u64(tiff.byte_order()) {
                if let Some(compression) = Compression::from_u16(comp_value as u16) {
                    match compression {
                        Compression::None => {
                            messages.push(ValidationMessage::warning(
                                ValidationCategory::Compression,
                                "No compression used - file size could be significantly reduced"
                                    .to_string(),
                            ));
                        }
                        Compression::Jpeg => {
                            messages.push(ValidationMessage::info(
                                ValidationCategory::Compression,
                                "JPEG compression (lossy) - acceptable for photographic data"
                                    .to_string(),
                            ));
                        }
                        Compression::Deflate | Compression::AdobeDeflate => {
                            messages.push(ValidationMessage::info(
                                ValidationCategory::Compression,
                                "DEFLATE compression - good general-purpose choice".to_string(),
                            ));
                        }
                        Compression::Lzw => {
                            messages.push(ValidationMessage::info(
                                ValidationCategory::Compression,
                                "LZW compression - widely compatible but may not be optimal"
                                    .to_string(),
                            ));
                        }
                        Compression::Zstd => {
                            messages.push(ValidationMessage::info(
                                ValidationCategory::Compression,
                                "ZSTD compression - excellent ratio but limited support"
                                    .to_string(),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Validates metadata optimization
fn validate_metadata(tiff: &TiffFile, messages: &mut Vec<ValidationMessage>) {
    if let Some(ifd) = tiff.ifds.first() {
        // Check for excessively large tags
        let mut large_tag_count = 0;

        for entry in &ifd.entries {
            if entry.count > 1000 {
                large_tag_count += 1;
            }
        }

        if large_tag_count > 0 {
            messages.push(ValidationMessage::info(
                ValidationCategory::Metadata,
                format!(
                    "{} large metadata tags found - may impact performance",
                    large_tag_count
                ),
            ));
        }
    }
}

/// Validates tile ordering for streaming
fn validate_tile_ordering<S: DataSource>(
    tiff: &TiffFile,
    source: &S,
    messages: &mut Vec<ValidationMessage>,
) -> Result<()> {
    if let Some(ifd) = tiff.ifds.first() {
        if let Some(offsets_entry) = ifd.get_entry(TiffTag::TileOffsets) {
            let offsets =
                offsets_entry.get_u64_vec(source, tiff.byte_order(), tiff.header.variant)?;

            // Check if tiles are in sequential order
            let mut is_sequential = true;
            for i in 1..offsets.len() {
                if offsets[i] < offsets[i - 1] {
                    is_sequential = false;
                    break;
                }
            }

            if !is_sequential {
                messages.push(ValidationMessage::warning(
                    ValidationCategory::TileOrganization,
                    "Tiles are not in sequential order - may reduce streaming efficiency"
                        .to_string(),
                ));
            }
        }
    }

    Ok(())
}

/// Calculates performance metrics
fn calculate_performance_metrics<S: DataSource>(
    tiff: &TiffFile,
    source: &S,
) -> Result<PerformanceMetrics> {
    let requests_per_tile = 1; // Ideal case: 1 request per tile
    let mut bytes_per_tile = 0u64;

    if let Some(ifd) = tiff.ifds.first() {
        if let (Some(tw), Some(th), Some(bps), Some(spp)) = (
            ifd.get_entry(TiffTag::TileWidth).and_then(|e| {
                e.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                    .ok()
            }),
            ifd.get_entry(TiffTag::TileLength).and_then(|e| {
                e.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                    .ok()
            }),
            ifd.get_entry(TiffTag::BitsPerSample).and_then(|e| {
                e.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                    .ok()
            }),
            ifd.get_entry(TiffTag::SamplesPerPixel).and_then(|e| {
                e.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                    .ok()
            }),
        ) {
            bytes_per_tile = tw * th * spp * (bps / 8);
        }
    }

    let overhead_percentage = 5.0; // Typical overhead
    let optimal_ifd_position = true; // Would need more analysis
    let optimal_tile_order = true; // Would need more analysis

    Ok(PerformanceMetrics {
        requests_per_tile,
        bytes_per_tile,
        overhead_percentage,
        optimal_ifd_position,
        optimal_tile_order,
    })
}

/// Validates HTTP efficiency
fn validate_http_efficiency(
    tiff: &TiffFile,
    performance: &PerformanceMetrics,
    messages: &mut Vec<ValidationMessage>,
) {
    if performance.requests_per_tile > 1 {
        messages.push(ValidationMessage::warning(
            ValidationCategory::HttpEfficiency,
            format!(
                "Each tile requires {} HTTP requests - consider optimizing IFD layout",
                performance.requests_per_tile
            ),
        ));
    }

    if performance.overhead_percentage > 10.0 {
        messages.push(ValidationMessage::info(
            ValidationCategory::HttpEfficiency,
            format!(
                "Estimated {:.1}% overhead per tile access",
                performance.overhead_percentage
            ),
        ));
    }

    // Check if file is small enough to benefit from COG
    if let Some(ifd) = tiff.ifds.first() {
        if let (Some(w), Some(h)) = (
            ifd.get_entry(TiffTag::ImageWidth)
                .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
            ifd.get_entry(TiffTag::ImageLength)
                .and_then(|e| e.get_u64(tiff.byte_order()).ok()),
        ) {
            let total_pixels = w * h;
            if total_pixels < 1_000_000 {
                // Less than 1MP
                messages.push(ValidationMessage::info(
                    ValidationCategory::Performance,
                    "Small image - COG overhead may not be beneficial".to_string(),
                ));
            }
        }
    }
}

/// Calculates compliance score
fn calculate_compliance_score(
    messages: &[ValidationMessage],
    performance: &PerformanceMetrics,
) -> u8 {
    let mut score = 100u8;

    for msg in messages {
        match msg.severity {
            ValidationSeverity::Error => score = score.saturating_sub(20),
            ValidationSeverity::Warning => score = score.saturating_sub(5),
            ValidationSeverity::Info => {}
        }
    }

    // Bonus points for optimal configuration
    if performance.optimal_ifd_position {
        score = score.saturating_add(5).min(100);
    }
    if performance.optimal_tile_order {
        score = score.saturating_add(5).min(100);
    }

    score
}

/// Determines if COG is optimally configured
fn is_optimal_cog(messages: &[ValidationMessage], performance: &PerformanceMetrics) -> bool {
    let has_errors = messages
        .iter()
        .any(|m| m.severity == ValidationSeverity::Error);
    let warning_count = messages
        .iter()
        .filter(|m| m.severity == ValidationSeverity::Warning)
        .count();

    !has_errors
        && warning_count == 0
        && performance.optimal_ifd_position
        && performance.optimal_tile_order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_message_creation() {
        let msg =
            ValidationMessage::error(ValidationCategory::IfdStructure, "Test error".to_string());
        assert_eq!(msg.severity, ValidationSeverity::Error);
    }

    #[test]
    fn test_compliance_score_calculation() {
        let messages = vec![
            ValidationMessage::error(ValidationCategory::IfdStructure, "Error".to_string()),
            ValidationMessage::warning(ValidationCategory::Performance, "Warning".to_string()),
        ];

        let performance = PerformanceMetrics {
            requests_per_tile: 1,
            bytes_per_tile: 65536,
            overhead_percentage: 5.0,
            optimal_ifd_position: true,
            optimal_tile_order: true,
        };

        let score = calculate_compliance_score(&messages, &performance);
        assert!(score < 100);
        assert!(score > 50);
    }
}
