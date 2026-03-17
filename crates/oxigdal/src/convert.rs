//! Format conversion planning and detection utilities.
//!
//! This module provides tools for detecting geospatial dataset formats,
//! checking conversion feasibility between formats, and building conversion
//! plans.
//!
//! # Overview
//!
//! - `detect_format` — detect the format of a file from extension and/or magic bytes
//! - `can_convert` — check whether a conversion path exists between two formats
//! - `supported_conversions` — enumerate all supported format conversion pairs
//! - `ConvertOptions` — options that control a conversion (bbox filter, CRS, etc.)
//! - `ConversionPlan` — an ordered list of steps that will execute a conversion
//!
//! # Examples
//!
//! ```rust
//! use oxigdal::convert::{detect_format, can_convert};
//! use oxigdal::DatasetFormat;
//!
//! let fmt = detect_format("world.tif").expect("detect");
//! assert_eq!(fmt, DatasetFormat::GeoTiff);
//!
//! assert!(can_convert(DatasetFormat::GeoTiff, DatasetFormat::Zarr));
//! ```

use crate::{DatasetFormat, OxiGdalError, Result};

// ─── Format category helpers ─────────────────────────────────────────────────

/// Returns `true` if the given format is a raster format.
fn is_raster(fmt: DatasetFormat) -> bool {
    matches!(
        fmt,
        DatasetFormat::GeoTiff
            | DatasetFormat::Jpeg2000
            | DatasetFormat::NetCdf
            | DatasetFormat::Hdf5
            | DatasetFormat::Zarr
            | DatasetFormat::Grib
            | DatasetFormat::Vrt
            | DatasetFormat::PMTiles
            | DatasetFormat::MBTiles
            | DatasetFormat::Copc
    )
}

/// Returns `true` if the given format is a vector format.
fn is_vector(fmt: DatasetFormat) -> bool {
    matches!(
        fmt,
        DatasetFormat::GeoJson
            | DatasetFormat::Shapefile
            | DatasetFormat::GeoParquet
            | DatasetFormat::FlatGeobuf
            | DatasetFormat::GeoPackage
            | DatasetFormat::Stac
    )
}

/// Returns `true` if the given format supports both raster and vector data.
fn is_mixed(fmt: DatasetFormat) -> bool {
    matches!(fmt, DatasetFormat::GeoPackage)
}

// ─── ConvertOptions ──────────────────────────────────────────────────────────

/// Options controlling a format conversion.
///
/// These options are applied during conversion to filter, transform, or
/// constrain the output dataset.
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Optional bounding-box filter `(min_x, min_y, max_x, max_y)` in the
    /// source CRS. Only features/pixels inside this box are converted.
    pub bbox: Option<(f64, f64, f64, f64)>,

    /// Target coordinate reference system (e.g. `"EPSG:4326"`).
    ///
    /// When set, the output is reprojected to this CRS.
    pub target_crs: Option<String>,

    /// Source coordinate reference system override.
    ///
    /// When the source file lacks embedded CRS metadata, use this to declare it.
    pub source_crs: Option<String>,

    /// Maximum number of features to convert (vector datasets).
    pub feature_limit: Option<usize>,

    /// Raster overview level to read from (0 = full resolution).
    pub overview_level: Option<u32>,

    /// Target tile size for raster output (width, height) in pixels.
    pub tile_size: Option<(u32, u32)>,

    /// Compression hint for the output format (e.g. `"deflate"`, `"lz4"`).
    pub compression: Option<String>,

    /// If `true`, overwrite the target file when it already exists.
    pub overwrite: bool,
}

impl ConvertOptions {
    /// Create a new default (empty) options struct.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a bounding-box spatial filter.
    pub fn with_bbox(mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        self.bbox = Some((min_x, min_y, max_x, max_y));
        self
    }

    /// Set the target CRS for reprojection.
    pub fn with_target_crs(mut self, crs: impl Into<String>) -> Self {
        self.target_crs = Some(crs.into());
        self
    }

    /// Set the source CRS override.
    pub fn with_source_crs(mut self, crs: impl Into<String>) -> Self {
        self.source_crs = Some(crs.into());
        self
    }

    /// Set maximum features to convert.
    pub fn with_feature_limit(mut self, limit: usize) -> Self {
        self.feature_limit = Some(limit);
        self
    }

    /// Set raster overview level.
    pub fn with_overview_level(mut self, level: u32) -> Self {
        self.overview_level = Some(level);
        self
    }

    /// Set tile size for raster output.
    pub fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.tile_size = Some((width, height));
        self
    }

    /// Set compression hint.
    pub fn with_compression(mut self, compression: impl Into<String>) -> Self {
        self.compression = Some(compression.into());
        self
    }

    /// Enable overwrite mode.
    pub fn with_overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }
}

// ─── ConversionStep ──────────────────────────────────────────────────────────

/// A single step in a conversion plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionStep {
    /// Read the source dataset in the given format.
    ReadSource(DatasetFormat),
    /// Reproject data from one CRS to another.
    Reproject {
        /// Source CRS identifier.
        from_crs: String,
        /// Target CRS identifier.
        to_crs: String,
    },
    /// Apply a spatial bounding-box filter.
    BboxFilter,
    /// Apply a feature-count limit.
    FeatureLimit(usize),
    /// Transcode raster data between formats.
    TranscodeRaster(DatasetFormat),
    /// Transcode vector data between formats.
    TranscodeVector(DatasetFormat),
    /// Write the output dataset in the given format.
    WriteTarget(DatasetFormat),
}

// ─── ConversionPlan ──────────────────────────────────────────────────────────

/// An ordered list of steps that will execute a format conversion.
///
/// Created via [`ConversionPlan::build`].
#[derive(Debug, Clone)]
pub struct ConversionPlan {
    /// Source format.
    pub source: DatasetFormat,
    /// Target format.
    pub target: DatasetFormat,
    /// Ordered steps.
    pub steps: Vec<ConversionStep>,
    /// Options that were used to build this plan.
    pub options: ConvertOptions,
}

impl ConversionPlan {
    /// Build a conversion plan for the given source/target pair and options.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::NotSupported`] if the conversion is not feasible.
    pub fn build(
        source: DatasetFormat,
        target: DatasetFormat,
        options: ConvertOptions,
    ) -> Result<Self> {
        if !can_convert(source, target) {
            return Err(OxiGdalError::NotSupported {
                operation: format!(
                    "conversion from {} to {} is not supported",
                    source.driver_name(),
                    target.driver_name()
                ),
            });
        }

        let mut steps = Vec::new();

        // Step 1: Read source
        steps.push(ConversionStep::ReadSource(source));

        // Step 2: Optional bbox filter
        if options.bbox.is_some() {
            steps.push(ConversionStep::BboxFilter);
        }

        // Step 3: Optional feature limit
        if let Some(limit) = options.feature_limit {
            steps.push(ConversionStep::FeatureLimit(limit));
        }

        // Step 4: Optional reprojection
        if let Some(ref to_crs) = options.target_crs {
            let from_crs = options
                .source_crs
                .clone()
                .unwrap_or_else(|| "auto".to_string());
            steps.push(ConversionStep::Reproject {
                from_crs,
                to_crs: to_crs.clone(),
            });
        }

        // Step 5: Transcode if needed
        if source != target {
            if is_raster(target) {
                steps.push(ConversionStep::TranscodeRaster(target));
            } else if is_vector(target) {
                steps.push(ConversionStep::TranscodeVector(target));
            }
        }

        // Step 6: Write target
        steps.push(ConversionStep::WriteTarget(target));

        Ok(Self {
            source,
            target,
            steps,
            options,
        })
    }

    /// Return the number of steps in this plan.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Return `true` if this plan includes a reprojection step.
    pub fn has_reprojection(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, ConversionStep::Reproject { .. }))
    }

    /// Return `true` if this plan includes a spatial filter step.
    pub fn has_bbox_filter(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, ConversionStep::BboxFilter))
    }
}

// ─── Public conversion API ───────────────────────────────────────────────────

/// All dataset format variants (excluding `Unknown`).
const ALL_FORMATS: &[DatasetFormat] = &[
    DatasetFormat::GeoTiff,
    DatasetFormat::GeoJson,
    DatasetFormat::Shapefile,
    DatasetFormat::GeoParquet,
    DatasetFormat::NetCdf,
    DatasetFormat::Hdf5,
    DatasetFormat::Zarr,
    DatasetFormat::Grib,
    DatasetFormat::Stac,
    DatasetFormat::Terrain,
    DatasetFormat::Vrt,
    DatasetFormat::FlatGeobuf,
    DatasetFormat::Jpeg2000,
    DatasetFormat::GeoPackage,
    DatasetFormat::PMTiles,
    DatasetFormat::MBTiles,
    DatasetFormat::Copc,
];

/// Check whether a conversion from `from` to `to` is supported.
///
/// Conversions are supported when:
/// - Same format (identity conversion, always true)
/// - Both raster: raster-to-raster transcoding
/// - Both vector: vector-to-vector transcoding
/// - Mixed-format targets (GeoPackage) accept both raster and vector sources
///
/// Unknown or unsupported format pairs return `false`.
pub fn can_convert(from: DatasetFormat, to: DatasetFormat) -> bool {
    // Identity is always allowed
    if from == to {
        return true;
    }

    // Unknown cannot be converted
    if from == DatasetFormat::Unknown || to == DatasetFormat::Unknown {
        return false;
    }

    // Raster → raster
    if is_raster(from) && is_raster(to) {
        return true;
    }

    // Vector → vector
    if is_vector(from) && is_vector(to) {
        return true;
    }

    // Mixed target (GeoPackage) accepts both raster and vector
    if is_mixed(to) && (is_raster(from) || is_vector(from)) {
        return true;
    }

    // Mixed source can go to either raster or vector targets
    if is_mixed(from) && (is_raster(to) || is_vector(to)) {
        return true;
    }

    false
}

/// Return a list of all supported conversion pairs `(source, target)`.
///
/// This enumerates every pair `(A, B)` where [`can_convert(A, B)`] is `true`,
/// excluding identity conversions (A == A).
pub fn supported_conversions() -> Vec<(DatasetFormat, DatasetFormat)> {
    let mut pairs = Vec::new();
    for &from in ALL_FORMATS {
        for &to in ALL_FORMATS {
            if from != to && can_convert(from, to) {
                pairs.push((from, to));
            }
        }
    }
    pairs
}

/// Detect the format of a file from its path (extension and compound extension).
///
/// This function checks:
/// 1. Compound extensions (e.g., `.copc.laz`)
/// 2. Simple extensions (`.tif`, `.geojson`, `.gpkg`, etc.)
///
/// # Errors
///
/// Returns [`OxiGdalError::NotSupported`] if the extension is not recognized
/// or the path is empty.
pub fn detect_format(path: &str) -> Result<DatasetFormat> {
    if path.is_empty() {
        return Err(OxiGdalError::NotSupported {
            operation: "cannot detect format from empty path".to_string(),
        });
    }

    let fmt = DatasetFormat::from_extension(path);
    if fmt == DatasetFormat::Unknown {
        return Err(OxiGdalError::NotSupported {
            operation: format!("unrecognised file extension in '{path}'"),
        });
    }

    Ok(fmt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_format ────────────────────────────────────────────────────────

    #[test]
    fn test_detect_geotiff() {
        assert_eq!(
            detect_format("elevation.tif").ok(),
            Some(DatasetFormat::GeoTiff)
        );
        assert_eq!(
            detect_format("elevation.tiff").ok(),
            Some(DatasetFormat::GeoTiff)
        );
    }

    #[test]
    fn test_detect_geojson() {
        assert_eq!(
            detect_format("world.geojson").ok(),
            Some(DatasetFormat::GeoJson)
        );
    }

    #[test]
    fn test_detect_gpkg() {
        assert_eq!(
            detect_format("layer.gpkg").ok(),
            Some(DatasetFormat::GeoPackage)
        );
    }

    #[test]
    fn test_detect_pmtiles() {
        assert_eq!(
            detect_format("tiles.pmtiles").ok(),
            Some(DatasetFormat::PMTiles)
        );
    }

    #[test]
    fn test_detect_mbtiles() {
        assert_eq!(
            detect_format("map.mbtiles").ok(),
            Some(DatasetFormat::MBTiles)
        );
    }

    #[test]
    fn test_detect_shapefile() {
        assert_eq!(
            detect_format("roads.shp").ok(),
            Some(DatasetFormat::Shapefile)
        );
    }

    #[test]
    fn test_detect_flatgeobuf() {
        assert_eq!(
            detect_format("buildings.fgb").ok(),
            Some(DatasetFormat::FlatGeobuf)
        );
    }

    #[test]
    fn test_detect_geoparquet() {
        assert_eq!(
            detect_format("census.parquet").ok(),
            Some(DatasetFormat::GeoParquet)
        );
    }

    #[test]
    fn test_detect_zarr() {
        assert_eq!(
            detect_format("climate.zarr").ok(),
            Some(DatasetFormat::Zarr)
        );
    }

    #[test]
    fn test_detect_copc_compound_ext() {
        assert_eq!(
            detect_format("cloud.copc.laz").ok(),
            Some(DatasetFormat::Copc)
        );
    }

    #[test]
    fn test_detect_copc_laz_ext() {
        assert_eq!(detect_format("data.laz").ok(), Some(DatasetFormat::Copc));
    }

    #[test]
    fn test_detect_copc_las_ext() {
        assert_eq!(detect_format("data.las").ok(), Some(DatasetFormat::Copc));
    }

    #[test]
    fn test_detect_empty_path_error() {
        assert!(detect_format("").is_err());
    }

    #[test]
    fn test_detect_unknown_extension_error() {
        assert!(detect_format("readme.txt").is_err());
    }

    #[test]
    fn test_detect_no_extension_error() {
        assert!(detect_format("Makefile").is_err());
    }

    // ── can_convert ──────────────────────────────────────────────────────────

    #[test]
    fn test_identity_always_true() {
        for &fmt in ALL_FORMATS {
            assert!(
                can_convert(fmt, fmt),
                "identity conversion should always be true for {:?}",
                fmt
            );
        }
    }

    #[test]
    fn test_raster_to_raster() {
        assert!(can_convert(DatasetFormat::GeoTiff, DatasetFormat::Jpeg2000));
        assert!(can_convert(DatasetFormat::GeoTiff, DatasetFormat::Zarr));
        assert!(can_convert(DatasetFormat::NetCdf, DatasetFormat::Hdf5));
        assert!(can_convert(DatasetFormat::GeoTiff, DatasetFormat::PMTiles));
        assert!(can_convert(DatasetFormat::GeoTiff, DatasetFormat::MBTiles));
    }

    #[test]
    fn test_vector_to_vector() {
        assert!(can_convert(
            DatasetFormat::GeoJson,
            DatasetFormat::Shapefile
        ));
        assert!(can_convert(
            DatasetFormat::GeoJson,
            DatasetFormat::GeoParquet
        ));
        assert!(can_convert(
            DatasetFormat::Shapefile,
            DatasetFormat::FlatGeobuf
        ));
        assert!(can_convert(
            DatasetFormat::GeoJson,
            DatasetFormat::GeoPackage
        ));
    }

    #[test]
    fn test_raster_to_vector_fails() {
        assert!(!can_convert(DatasetFormat::GeoTiff, DatasetFormat::GeoJson));
        assert!(!can_convert(
            DatasetFormat::Jpeg2000,
            DatasetFormat::Shapefile
        ));
    }

    #[test]
    fn test_vector_to_raster_fails() {
        assert!(!can_convert(DatasetFormat::GeoJson, DatasetFormat::GeoTiff));
        assert!(!can_convert(DatasetFormat::Shapefile, DatasetFormat::Zarr));
    }

    #[test]
    fn test_unknown_never_converts() {
        assert!(!can_convert(DatasetFormat::Unknown, DatasetFormat::GeoTiff));
        assert!(!can_convert(DatasetFormat::GeoTiff, DatasetFormat::Unknown));
    }

    #[test]
    fn test_mixed_target_geopackage_accepts_raster() {
        assert!(can_convert(
            DatasetFormat::GeoTiff,
            DatasetFormat::GeoPackage
        ));
    }

    #[test]
    fn test_mixed_target_geopackage_accepts_vector() {
        assert!(can_convert(
            DatasetFormat::GeoJson,
            DatasetFormat::GeoPackage
        ));
    }

    #[test]
    fn test_mixed_source_geopackage_to_raster() {
        assert!(can_convert(
            DatasetFormat::GeoPackage,
            DatasetFormat::GeoTiff
        ));
    }

    #[test]
    fn test_mixed_source_geopackage_to_vector() {
        assert!(can_convert(
            DatasetFormat::GeoPackage,
            DatasetFormat::GeoJson
        ));
    }

    // ── supported_conversions ────────────────────────────────────────────────

    #[test]
    fn test_supported_conversions_non_empty() {
        let pairs = supported_conversions();
        assert!(
            pairs.len() > 20,
            "should have many supported pairs, got {}",
            pairs.len()
        );
    }

    #[test]
    fn test_supported_conversions_no_identity() {
        let pairs = supported_conversions();
        for (from, to) in &pairs {
            assert_ne!(from, to, "identity conversions should be excluded");
        }
    }

    #[test]
    fn test_supported_conversions_all_valid() {
        let pairs = supported_conversions();
        for (from, to) in &pairs {
            assert!(
                can_convert(*from, *to),
                "reported pair ({:?}, {:?}) should be convertible",
                from,
                to
            );
        }
    }

    // ── ConvertOptions ───────────────────────────────────────────────────────

    #[test]
    fn test_convert_options_default() {
        let opts = ConvertOptions::new();
        assert!(opts.bbox.is_none());
        assert!(opts.target_crs.is_none());
        assert!(opts.source_crs.is_none());
        assert!(opts.feature_limit.is_none());
        assert!(opts.overview_level.is_none());
        assert!(opts.tile_size.is_none());
        assert!(opts.compression.is_none());
        assert!(!opts.overwrite);
    }

    #[test]
    fn test_convert_options_builder() {
        let opts = ConvertOptions::new()
            .with_bbox(-180.0, -90.0, 180.0, 90.0)
            .with_target_crs("EPSG:3857")
            .with_source_crs("EPSG:4326")
            .with_feature_limit(1000)
            .with_overview_level(2)
            .with_tile_size(512, 512)
            .with_compression("deflate")
            .with_overwrite();

        assert_eq!(opts.bbox, Some((-180.0, -90.0, 180.0, 90.0)));
        assert_eq!(opts.target_crs.as_deref(), Some("EPSG:3857"));
        assert_eq!(opts.source_crs.as_deref(), Some("EPSG:4326"));
        assert_eq!(opts.feature_limit, Some(1000));
        assert_eq!(opts.overview_level, Some(2));
        assert_eq!(opts.tile_size, Some((512, 512)));
        assert_eq!(opts.compression.as_deref(), Some("deflate"));
        assert!(opts.overwrite);
    }

    // ── ConversionPlan ───────────────────────────────────────────────────────

    #[test]
    fn test_plan_simple_raster_to_raster() {
        let plan = ConversionPlan::build(
            DatasetFormat::GeoTiff,
            DatasetFormat::Zarr,
            ConvertOptions::new(),
        )
        .expect("plan");
        assert_eq!(plan.source, DatasetFormat::GeoTiff);
        assert_eq!(plan.target, DatasetFormat::Zarr);
        assert!(plan.step_count() >= 3); // read + transcode + write
        assert!(!plan.has_reprojection());
        assert!(!plan.has_bbox_filter());
    }

    #[test]
    fn test_plan_vector_to_vector_with_options() {
        let opts = ConvertOptions::new()
            .with_bbox(0.0, 0.0, 10.0, 10.0)
            .with_target_crs("EPSG:3857")
            .with_feature_limit(500);

        let plan = ConversionPlan::build(DatasetFormat::GeoJson, DatasetFormat::Shapefile, opts)
            .expect("plan");

        assert!(plan.has_reprojection());
        assert!(plan.has_bbox_filter());
        // read + bbox + limit + reproject + transcode + write = 6 steps
        assert_eq!(plan.step_count(), 6);
    }

    #[test]
    fn test_plan_unsupported_conversion_error() {
        let result = ConversionPlan::build(
            DatasetFormat::GeoTiff,
            DatasetFormat::GeoJson,
            ConvertOptions::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_identity_minimal_steps() {
        let plan = ConversionPlan::build(
            DatasetFormat::GeoTiff,
            DatasetFormat::GeoTiff,
            ConvertOptions::new(),
        )
        .expect("plan");
        // read + write (no transcode for identity)
        assert_eq!(plan.step_count(), 2);
    }

    #[test]
    fn test_plan_steps_order() {
        let opts = ConvertOptions::new()
            .with_bbox(0.0, 0.0, 1.0, 1.0)
            .with_target_crs("EPSG:3857");

        let plan = ConversionPlan::build(DatasetFormat::GeoJson, DatasetFormat::FlatGeobuf, opts)
            .expect("plan");

        // Verify ordering: ReadSource first, WriteTarget last
        assert!(
            matches!(plan.steps.first(), Some(ConversionStep::ReadSource(_))),
            "first step should be ReadSource"
        );
        assert!(
            matches!(plan.steps.last(), Some(ConversionStep::WriteTarget(_))),
            "last step should be WriteTarget"
        );
    }

    #[test]
    fn test_plan_reproject_step_contains_crs() {
        let opts = ConvertOptions::new()
            .with_source_crs("EPSG:4326")
            .with_target_crs("EPSG:32632");

        let plan = ConversionPlan::build(DatasetFormat::GeoJson, DatasetFormat::Shapefile, opts)
            .expect("plan");

        let reproject_step = plan
            .steps
            .iter()
            .find(|s| matches!(s, ConversionStep::Reproject { .. }))
            .expect("should have reproject step");

        if let ConversionStep::Reproject { from_crs, to_crs } = reproject_step {
            assert_eq!(from_crs, "EPSG:4326");
            assert_eq!(to_crs, "EPSG:32632");
        }
    }

    #[test]
    fn test_plan_auto_source_crs_when_not_set() {
        let opts = ConvertOptions::new().with_target_crs("EPSG:3857");

        let plan = ConversionPlan::build(DatasetFormat::Shapefile, DatasetFormat::GeoJson, opts)
            .expect("plan");

        let reproject_step = plan
            .steps
            .iter()
            .find(|s| matches!(s, ConversionStep::Reproject { .. }))
            .expect("should have reproject step");

        if let ConversionStep::Reproject { from_crs, .. } = reproject_step {
            assert_eq!(from_crs, "auto");
        }
    }
}
