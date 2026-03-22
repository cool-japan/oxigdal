//! High-level COG tools and utilities
//!
//! This module provides convenient high-level functions for COG creation,
//! validation, and optimization.

use oxigdal_core::error::Result;
use oxigdal_core::io::{DataSource, FileDataSource};
use oxigdal_core::types::RasterDataType;

use crate::tiff::{Compression, PhotometricInterpretation, TiffFile};

use super::converter::{CogConverter, ConversionResult};
use super::optimizer::{OptimizationGoal, analyze_for_cog, estimate_cloud_cost};
use super::validator::{DetailedCogValidation, validate_cog_detailed};

/// Quick COG validation
///
/// Validates a file and returns a simple boolean result.
pub fn is_valid_cog(path: impl AsRef<str>) -> Result<bool> {
    let source = FileDataSource::open(path.as_ref())?;
    let tiff = TiffFile::parse(&source)?;
    let validation = validate_cog_detailed(&tiff, &source)?;
    Ok(validation.is_valid_cog)
}

/// Detailed COG validation
///
/// Returns comprehensive validation report.
pub fn validate_cog_file(path: impl AsRef<str>) -> Result<DetailedCogValidation> {
    let source = FileDataSource::open(path.as_ref())?;
    let tiff = TiffFile::parse(&source)?;
    validate_cog_detailed(&tiff, &source)
}

/// Simple COG creation
///
/// Creates a COG with sensible defaults.
pub fn create_cog(
    input_path: impl AsRef<str>,
    output_path: impl AsRef<str>,
) -> Result<ConversionResult> {
    CogConverter::new(input_path.as_ref())
        .output(output_path.as_ref())
        .auto_optimize()
        .convert()
}

/// Creates an optimized COG with specific goal
pub fn create_optimized_cog(
    input_path: impl AsRef<str>,
    output_path: impl AsRef<str>,
    goal: OptimizationGoal,
) -> Result<ConversionResult> {
    CogConverter::new(input_path.as_ref())
        .output(output_path.as_ref())
        .auto_optimize()
        .with_goal(goal)
        .convert()
}

/// Analyzes a file for COG conversion
///
/// Returns recommendations without performing conversion.
pub fn analyze_file_for_cog(path: impl AsRef<str>) -> Result<super::optimizer::CogOptimization> {
    let source = FileDataSource::open(path.as_ref())?;
    let tiff = TiffFile::parse(&source)?;

    let ifd =
        tiff.ifds
            .first()
            .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
                parameter: "IFDs",
                message: "No IFDs found in file".to_string(),
            })?;

    let width = ifd
        .get_entry(crate::tiff::TiffTag::ImageWidth)
        .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
            parameter: "ImageWidth",
            message: "Missing required ImageWidth tag".to_string(),
        })?
        .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

    let height = ifd
        .get_entry(crate::tiff::TiffTag::ImageLength)
        .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
            parameter: "ImageLength",
            message: "Missing required ImageLength tag".to_string(),
        })?
        .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

    // Placeholder data for analysis
    let data_type = RasterDataType::UInt8;
    let samples_per_pixel = 1;
    let photometric = PhotometricInterpretation::BlackIsZero;

    let sample_size = (width.min(1024) * height.min(1024) * samples_per_pixel as u64) as usize;
    let sample_data = vec![0u8; sample_size];

    analyze_for_cog(
        &sample_data,
        width,
        height,
        data_type,
        samples_per_pixel,
        photometric,
        OptimizationGoal::Balanced,
        None,
    )
}

/// Estimates cloud storage costs for a COG
pub fn estimate_storage_cost(
    path: impl AsRef<str>,
    monthly_reads: u64,
    avg_tiles_per_read: u32,
) -> Result<super::optimizer::CloudCostEstimate> {
    let source = FileDataSource::open(path.as_ref())?;
    let file_size = source.size()?;

    let tiff = TiffFile::parse(&source)?;
    let ifd =
        tiff.ifds
            .first()
            .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
                parameter: "IFDs",
                message: "No IFDs found in file".to_string(),
            })?;

    let tile_width = ifd
        .get_entry(crate::tiff::TiffTag::TileWidth)
        .and_then(|e: &_| {
            e.get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)
                .ok()
        })
        .unwrap_or(512);

    // Estimate compression ratio
    let compression_ratio = 2.5; // Typical

    Ok(estimate_cloud_cost(
        file_size,
        monthly_reads,
        avg_tiles_per_read,
        tile_width as u32,
        compression_ratio,
    ))
}

/// Compares two COG files
pub fn compare_cogs(path1: impl AsRef<str>, path2: impl AsRef<str>) -> Result<CogComparison> {
    let source1 = FileDataSource::open(path1.as_ref())?;
    let source2 = FileDataSource::open(path2.as_ref())?;

    let tiff1 = TiffFile::parse(&source1)?;
    let tiff2 = TiffFile::parse(&source2)?;

    let size1 = source1.size()?;
    let size2 = source2.size()?;

    let validation1 = validate_cog_detailed(&tiff1, &source1)?;
    let validation2 = validate_cog_detailed(&tiff2, &source2)?;

    Ok(CogComparison {
        file1_size: size1,
        file2_size: size2,
        file1_valid: validation1.is_valid_cog,
        file2_valid: validation2.is_valid_cog,
        file1_compliance_score: validation1.compliance_score,
        file2_compliance_score: validation2.compliance_score,
        size_difference_percent: ((size2 as i64 - size1 as i64) as f64 / size1 as f64) * 100.0,
    })
}

/// COG comparison result
#[derive(Debug, Clone)]
pub struct CogComparison {
    /// First file size
    pub file1_size: u64,
    /// Second file size
    pub file2_size: u64,
    /// First file COG validity
    pub file1_valid: bool,
    /// Second file COG validity
    pub file2_valid: bool,
    /// First file compliance score
    pub file1_compliance_score: u8,
    /// Second file compliance score
    pub file2_compliance_score: u8,
    /// Size difference (percentage)
    pub size_difference_percent: f64,
}

/// Optimizes an existing COG
pub fn optimize_existing_cog(
    input_path: impl AsRef<str>,
    output_path: impl AsRef<str>,
    goal: OptimizationGoal,
) -> Result<ConversionResult> {
    create_optimized_cog(input_path, output_path, goal)
}

/// Information about a COG file
#[derive(Debug, Clone)]
pub struct CogInfo {
    /// File size
    pub file_size: u64,
    /// Image dimensions
    pub dimensions: (u64, u64),
    /// Tile size
    pub tile_size: Option<(u32, u32)>,
    /// Compression
    pub compression: Compression,
    /// Number of overview levels
    pub overview_count: usize,
    /// Is valid COG
    pub is_valid_cog: bool,
    /// Compliance score
    pub compliance_score: u8,
}

/// Gets information about a COG file
pub fn get_cog_info(path: impl AsRef<str>) -> Result<CogInfo> {
    let source = FileDataSource::open(path.as_ref())?;
    let tiff = TiffFile::parse(&source)?;

    let file_size = source.size()?;

    let ifd =
        tiff.ifds
            .first()
            .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
                parameter: "IFDs",
                message: "No IFDs found in file".to_string(),
            })?;

    let width = ifd
        .get_entry(crate::tiff::TiffTag::ImageWidth)
        .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
            parameter: "ImageWidth",
            message: "Missing required ImageWidth tag".to_string(),
        })?
        .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

    let height = ifd
        .get_entry(crate::tiff::TiffTag::ImageLength)
        .ok_or_else(|| oxigdal_core::error::OxiGdalError::InvalidParameter {
            parameter: "ImageLength",
            message: "Missing required ImageLength tag".to_string(),
        })?
        .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

    let tile_width = ifd
        .get_entry(crate::tiff::TiffTag::TileWidth)
        .and_then(|e: &_| {
            e.get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)
                .ok()
        })
        .map(|v| v as u32);

    let tile_height = ifd
        .get_entry(crate::tiff::TiffTag::TileLength)
        .and_then(|e: &_| {
            e.get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)
                .ok()
        })
        .map(|v| v as u32);

    let tile_size = match (tile_width, tile_height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };

    let compression_val: u16 = ifd
        .get_entry(crate::tiff::TiffTag::Compression)
        .and_then(|e: &crate::tiff::IfdEntry| {
            e.get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)
                .ok()
                .map(|v| v as u16)
        })
        .unwrap_or(1);

    let compression = Compression::from_u16(compression_val).unwrap_or(Compression::None);

    let overview_count = tiff.ifds.len().saturating_sub(1);

    let validation = validate_cog_detailed(&tiff, &source)?;

    Ok(CogInfo {
        file_size,
        dimensions: (width, height),
        tile_size,
        compression,
        overview_count,
        is_valid_cog: validation.is_valid_cog,
        compliance_score: validation.compliance_score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cog_comparison_struct() {
        let comparison = CogComparison {
            file1_size: 1000000,
            file2_size: 800000,
            file1_valid: true,
            file2_valid: true,
            file1_compliance_score: 90,
            file2_compliance_score: 95,
            size_difference_percent: -20.0,
        };

        assert_eq!(comparison.file1_size, 1000000);
        assert_eq!(comparison.file2_size, 800000);
        assert!(comparison.size_difference_percent < 0.0);
    }

    #[test]
    fn test_cog_info_struct() {
        let info = CogInfo {
            file_size: 1000000,
            dimensions: (1024, 1024),
            tile_size: Some((256, 256)),
            compression: Compression::Deflate,
            overview_count: 3,
            is_valid_cog: true,
            compliance_score: 95,
        };

        assert_eq!(info.dimensions.0, 1024);
        assert!(info.tile_size.is_some());
        assert!(info.is_valid_cog);
    }
}
