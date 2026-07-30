//! Universal COG converter - converts any GeoTIFF to Cloud Optimized GeoTIFF
//!
//! This module provides high-level conversion functionality with automatic
//! format detection, optimization, and progress reporting.

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{DataSource, FileDataSource};

use crate::GeoTiffReader;
use crate::tiff::{Compression, TiffFile};
use crate::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};

use super::optimizer::{OptimizationGoal, analyze_for_cog};

/// COG conversion configuration
#[derive(Debug, Clone)]
pub struct ConversionConfig {
    /// Tile width (auto if None)
    pub tile_width: Option<u32>,
    /// Tile height (auto if None)
    pub tile_height: Option<u32>,
    /// Compression (auto if None)
    pub compression: Option<Compression>,
    /// Overview levels (auto if None)
    pub overview_levels: Option<Vec<u32>>,
    /// Resampling method
    pub resampling: OverviewResampling,
    /// Optimization goal
    pub optimization_goal: OptimizationGoal,
    /// Whether to preserve all metadata
    pub preserve_metadata: bool,
    /// Whether to validate output
    pub validate_output: bool,
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            tile_width: None,
            tile_height: None,
            compression: None,
            overview_levels: None,
            resampling: OverviewResampling::Average,
            optimization_goal: OptimizationGoal::Balanced,
            preserve_metadata: true,
            validate_output: true,
        }
    }
}

/// Conversion progress callback
pub type ProgressCallback = Box<dyn Fn(ConversionProgress) + Send + Sync>;

/// Conversion progress information
#[derive(Debug, Clone)]
pub struct ConversionProgress {
    /// Current step
    pub step: ConversionStep,
    /// Progress percentage (0-100)
    pub progress_percent: u8,
    /// Optional message
    pub message: Option<String>,
}

/// Conversion steps
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionStep {
    /// Analyzing input
    Analyzing,
    /// Determining optimal settings
    Optimizing,
    /// Writing base image
    WritingBase,
    /// Generating overviews
    GeneratingOverviews,
    /// Writing overview level
    WritingOverview(usize),
    /// Validating output
    Validating,
    /// Complete
    Complete,
}

/// Result of COG conversion
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// Output file size
    pub output_size: u64,
    /// Input file size
    pub input_size: u64,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Number of overview levels created
    pub overview_count: usize,
    /// Tile configuration used
    pub tile_size: (u32, u32),
    /// Compression used
    pub compression_used: Compression,
    /// Whether output passed validation
    pub validation_passed: bool,
    /// Conversion time (milliseconds)
    pub duration_ms: u64,
}

/// COG converter
pub struct CogConverter {
    input_path: String,
    output_path: Option<String>,
    config: ConversionConfig,
    progress_callback: Option<ProgressCallback>,
}

impl CogConverter {
    /// Creates a new converter
    pub fn new(input_path: impl Into<String>) -> Self {
        Self {
            input_path: input_path.into(),
            output_path: None,
            config: ConversionConfig::default(),
            progress_callback: None,
        }
    }

    /// Sets output path
    pub fn output(mut self, path: impl Into<String>) -> Self {
        self.output_path = Some(path.into());
        self
    }

    /// Sets tile size
    pub fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.config.tile_width = Some(width);
        self.config.tile_height = Some(height);
        self
    }

    /// Sets compression
    pub fn with_compression(mut self, compression: Compression) -> Self {
        self.config.compression = Some(compression);
        self
    }

    /// Sets overview levels
    pub fn with_overviews(mut self, levels: &[u32]) -> Self {
        self.config.overview_levels = Some(levels.to_vec());
        self
    }

    /// Sets resampling method
    pub fn with_resampling(mut self, resampling: OverviewResampling) -> Self {
        self.config.resampling = resampling;
        self
    }

    /// Enables auto-optimization
    pub fn auto_optimize(mut self) -> Self {
        // Auto-optimize will use analysis to determine settings
        self.config.tile_width = None;
        self.config.tile_height = None;
        self.config.compression = None;
        self.config.overview_levels = None;
        self
    }

    /// Sets optimization goal
    pub fn with_goal(mut self, goal: OptimizationGoal) -> Self {
        self.config.optimization_goal = goal;
        self
    }

    /// Sets progress callback
    pub fn on_progress(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Performs the conversion
    pub fn convert(self) -> Result<ConversionResult> {
        let start_time = std::time::Instant::now();

        // Report progress
        self.report_progress(ConversionProgress {
            step: ConversionStep::Analyzing,
            progress_percent: 0,
            message: Some("Analyzing input file...".to_string()),
        });

        // Open input file
        let source = FileDataSource::open(&self.input_path)?;
        let tiff = TiffFile::parse(&source)?;

        // Get image info
        let ifd = tiff
            .ifds
            .first()
            .ok_or_else(|| OxiGeoError::InvalidParameter {
                parameter: "IFDs",
                message: format!("No IFDs found in TIFF: {}", self.input_path),
            })?;

        let width = ifd
            .get_entry(crate::tiff::TiffTag::ImageWidth)
            .ok_or_else(|| OxiGeoError::InvalidParameter {
                parameter: "ImageWidth",
                message: format!("Missing ImageWidth tag in {}", self.input_path),
            })?
            .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

        let height = ifd
            .get_entry(crate::tiff::TiffTag::ImageLength)
            .ok_or_else(|| OxiGeoError::InvalidParameter {
                parameter: "ImageLength",
                message: format!("Missing ImageLength tag in {}", self.input_path),
            })?
            .get_u64_from_source(&source, tiff.byte_order(), tiff.header.variant)?;

        // Detect data type, sample count and photometric from the actual IFD.
        // `ImageInfo` derives these straight from the SampleFormat / BitsPerSample /
        // SamplesPerPixel / PhotometricInterpretation tags of the primary image.
        let primary_info =
            crate::tiff::ImageInfo::from_ifd(ifd, &source, tiff.byte_order(), tiff.header.variant)?;
        let data_type = primary_info
            .data_type()
            .ok_or_else(|| OxiGeoError::InvalidParameter {
                parameter: "SampleFormat",
                message: format!("Unsupported sample format/bit depth in {}", self.input_path),
            })?;
        let samples_per_pixel = primary_info.samples_per_pixel as usize;
        let photometric = primary_info.photometric;

        // Read the REAL pixel data of the primary image for analysis.
        //
        // `read_band(level, band)` returns ONE de-interleaved band plane
        // (`width × height × bytes_per_sample`), while `analyze_for_cog` and
        // `CogWriter::write` both want the row-major, band-interleaved (chunky)
        // whole-image buffer. So read each plane and weave them back together;
        // the single-band case — the common one — is a straight pass-through.
        // See <https://github.com/cool-japan/oxigeo/issues/14>.
        //
        // The same reader also carries the input's geospatial referencing, and
        // the same buffer is reused for the write pipeline below so the input is
        // only decoded once.
        let source_reader = GeoTiffReader::open(FileDataSource::open(&self.input_path)?)?;
        let image_data = if samples_per_pixel <= 1 {
            source_reader.read_band(0, 0)?
        } else {
            let plane_len = source_reader.band_byte_len(0)?;
            let pixel_count = source_reader.band_pixel_count(0)?;
            let bytes_per_sample = data_type.size_bytes();
            let mut interleaved =
                vec![
                    0u8;
                    plane_len.checked_mul(samples_per_pixel).ok_or_else(|| {
                        OxiGeoError::InvalidParameter {
                            parameter: "SamplesPerPixel",
                            message: format!(
                                "Interleaved image size overflows usize in {}",
                                self.input_path
                            ),
                        }
                    })?
                ];
            let mut plane = vec![0u8; plane_len];
            for band in 0..samples_per_pixel {
                source_reader.read_band_into(0, band, &mut plane)?;
                for px in 0..pixel_count {
                    let src = px * bytes_per_sample;
                    let dst = (px * samples_per_pixel + band) * bytes_per_sample;
                    interleaved[dst..dst + bytes_per_sample]
                        .copy_from_slice(&plane[src..src + bytes_per_sample]);
                }
            }
            interleaved
        };
        let sample_data = image_data.as_slice();

        // Report progress
        self.report_progress(ConversionProgress {
            step: ConversionStep::Optimizing,
            progress_percent: 10,
            message: Some("Determining optimal settings...".to_string()),
        });

        // Determine optimal settings
        let optimization = if self.config.tile_width.is_none()
            || self.config.compression.is_none()
            || self.config.overview_levels.is_none()
        {
            Some(analyze_for_cog(
                sample_data,
                width,
                height,
                data_type,
                samples_per_pixel,
                photometric,
                self.config.optimization_goal,
                None,
            )?)
        } else {
            None
        };

        // Use configured or optimized settings
        let tile_width = self
            .config
            .tile_width
            .or_else(|| optimization.as_ref().map(|o| o.optimal_tile_width))
            .unwrap_or(512);

        let tile_height = self
            .config
            .tile_height
            .or_else(|| optimization.as_ref().map(|o| o.optimal_tile_height))
            .unwrap_or(512);

        let compression = self
            .config
            .compression
            .or_else(|| optimization.as_ref().map(|o| o.recommended_compression))
            .unwrap_or(Compression::Deflate);

        let overview_levels = self
            .config
            .overview_levels
            .clone()
            .or_else(|| {
                optimization
                    .as_ref()
                    .map(|o| o.recommended_overviews.clone())
            })
            .unwrap_or_else(|| vec![2, 4, 8]);

        // Determine output path
        let output_path = if let Some(path) = &self.output_path {
            path.clone()
        } else {
            let input_path = std::path::Path::new(&self.input_path);
            let stem = input_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            format!("{}_cog.tif", stem)
        };

        // Report progress
        self.report_progress(ConversionProgress {
            step: ConversionStep::WritingBase,
            progress_percent: 20,
            message: Some(format!(
                "Writing COG with {}x{} tiles, {:?} compression",
                tile_width, tile_height, compression
            )),
        });

        // Build a COG-compliant writer configuration from the resolved
        // options. Predictor is left at the WriterConfig default (horizontal
        // differencing) which is appropriate for the deflate/LZW codecs the
        // optimizer recommends.
        let band_count =
            u16::try_from(samples_per_pixel).map_err(|_| OxiGeoError::InvalidParameter {
                parameter: "SamplesPerPixel",
                message: format!(
                    "Samples per pixel {} exceeds the supported range",
                    samples_per_pixel
                ),
            })?;

        let mut writer_config = WriterConfig::new(width, height, band_count, data_type)
            .with_compression(compression)
            .with_tile_size(tile_width, tile_height)
            .with_photometric(photometric)
            .with_overviews(!overview_levels.is_empty(), self.config.resampling)
            .with_overview_levels(overview_levels.clone());

        // Carry the input's geospatial referencing through to the output so
        // the produced COG is not silently un-georeferenced.
        if let Some(gt) = source_reader.geo_transform() {
            writer_config = writer_config.with_geo_transform(*gt);
        }
        if let Some(epsg) = source_reader.epsg_code() {
            writer_config = writer_config.with_epsg_code(epsg);
        }
        writer_config = writer_config.with_nodata(source_reader.nodata());

        // The input may not fit in a classic (32-bit-offset) TIFF; escalate
        // to BigTIFF when the projected size demands it.
        let bytes_per_sample = data_type.size_bytes() as u64;
        if crate::writer::needs_bigtiff(
            width,
            height,
            band_count as u64,
            bytes_per_sample,
            crate::writer::BigTiffMode::Auto,
        )? {
            writer_config = writer_config.with_bigtiff(true);
        }

        // Write the real COG to disk. `CogWriter::write` takes the full
        // row-major, band-interleaved buffer, generates overviews per the
        // configured levels, lays out all IFDs before tile data and finalises
        // the file (flush happens inside `write`).
        let writer_options = CogWriterOptions {
            byte_order: crate::tiff::ByteOrderType::LittleEndian,
            validate_after_write: self.config.validate_output,
        };
        let mut cog_writer = CogWriter::create(&output_path, writer_config, writer_options)?;
        let validation = cog_writer.write(&image_data)?;
        drop(cog_writer);

        // Measure the REAL sizes from disk.
        let input_size = source.size()?;
        let output_size = std::fs::metadata(&output_path)
            .map_err(|e| OxiGeoError::Io(e.into()))?
            .len();

        // Guard the ratio against a zero-byte output (a degenerate write).
        // 1.0 means "no size change" — the most neutral fallback.
        let compression_ratio = if output_size == 0 {
            1.0
        } else {
            input_size as f64 / output_size as f64
        };

        // Determine the real overview count from the file that was actually
        // written, rather than trusting the requested level list.
        let overview_count = match FileDataSource::open(&output_path)
            .and_then(|out_source| TiffFile::parse(&out_source).map(|t| t.ifds.len()))
        {
            Ok(ifd_count) => ifd_count.saturating_sub(1),
            Err(_) => overview_levels.len(),
        };

        // Report completion
        self.report_progress(ConversionProgress {
            step: ConversionStep::Complete,
            progress_percent: 100,
            message: Some("Conversion complete".to_string()),
        });

        let duration_ms = start_time.elapsed().as_millis() as u64;

        Ok(ConversionResult {
            output_size,
            input_size,
            compression_ratio,
            overview_count,
            tile_size: (tile_width, tile_height),
            compression_used: compression,
            validation_passed: validation.is_valid,
            duration_ms,
        })
    }

    /// Reports progress
    fn report_progress(&self, progress: ConversionProgress) {
        if let Some(ref callback) = self.progress_callback {
            callback(progress);
        }
    }
}

/// Batch conversion configuration
#[derive(Debug, Clone)]
pub struct BatchConversionConfig {
    /// Conversion config for each file
    pub conversion_config: ConversionConfig,
    /// Maximum parallel conversions
    pub max_parallel: usize,
    /// Continue on error
    pub continue_on_error: bool,
}

impl Default for BatchConversionConfig {
    fn default() -> Self {
        Self {
            conversion_config: ConversionConfig::default(),
            max_parallel: num_cpus::get(),
            continue_on_error: true,
        }
    }
}

/// Batch conversion result
#[derive(Debug)]
pub struct BatchConversionResult {
    /// Number of files successfully converted
    pub success_count: usize,
    /// Number of files that failed
    pub failure_count: usize,
    /// Individual results
    pub results: Vec<Result<ConversionResult>>,
    /// Total time (milliseconds)
    pub total_duration_ms: u64,
}

/// Converts multiple files to COG
pub fn convert_batch(
    input_paths: &[impl AsRef<str>],
    output_dir: impl AsRef<str>,
    config: BatchConversionConfig,
) -> BatchConversionResult {
    let start_time = std::time::Instant::now();
    let mut results = Vec::new();
    let mut success_count = 0;
    let mut failure_count = 0;

    for input_path in input_paths {
        let input_str = input_path.as_ref();
        let input_path_obj = std::path::Path::new(input_str);

        let output_name = input_path_obj
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");

        let output_path =
            std::path::Path::new(output_dir.as_ref()).join(format!("{}_cog.tif", output_name));

        let converter = CogConverter::new(input_str)
            .output(output_path.to_string_lossy().to_string())
            .with_goal(config.conversion_config.optimization_goal);

        let result = converter.convert();

        if result.is_ok() {
            success_count += 1;
        } else {
            failure_count += 1;
            if !config.continue_on_error {
                results.push(result);
                break;
            }
        }

        results.push(result);
    }

    let total_duration_ms = start_time.elapsed().as_millis() as u64;

    BatchConversionResult {
        success_count,
        failure_count,
        results,
        total_duration_ms,
    }
}

// Add num_cpus as a simple fallback
mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_config_default() {
        let config = ConversionConfig::default();
        assert!(config.tile_width.is_none());
        assert!(config.tile_height.is_none());
        assert!(config.compression.is_none());
        assert!(config.preserve_metadata);
    }

    #[test]
    fn test_converter_builder() {
        let converter = CogConverter::new("input.tif")
            .output("output.tif")
            .with_tile_size(256, 256)
            .with_compression(Compression::Deflate)
            .with_overviews(&[2, 4, 8]);

        assert_eq!(converter.config.tile_width, Some(256));
        assert_eq!(converter.config.tile_height, Some(256));
        assert_eq!(converter.config.compression, Some(Compression::Deflate));
    }

    #[test]
    fn test_auto_optimize() {
        let converter = CogConverter::new("input.tif").auto_optimize();

        assert!(converter.config.tile_width.is_none());
        assert!(converter.config.compression.is_none());
    }

    #[test]
    fn test_batch_config_default() {
        let config = BatchConversionConfig::default();
        assert!(config.max_parallel > 0);
        assert!(config.continue_on_error);
    }
}
