//! Stats command - Compute raster/vector statistics

use crate::OutputFormat;
use crate::util::raster::{read_band_region, read_raster_info};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_geojson::GeoJsonReader;
use oxigdal_shapefile::ShapefileReader;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Compute statistics for a raster or vector file
#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Input file path
    #[arg(value_name = "FILE")]
    pub input: String,

    /// Number of histogram bins for raster statistics
    #[arg(long, default_value = "256")]
    pub histogram_bins: usize,

    /// Band indices to compute statistics for (1-indexed, all bands if empty)
    #[arg(long = "band", value_name = "BAND")]
    pub band: Vec<u32>,

    /// Use approximate statistics (faster but less accurate)
    #[arg(long)]
    pub approx: bool,
}

/// Statistics for a single raster band
#[derive(Debug, Clone, Serialize)]
pub struct RasterBandStats {
    /// Band index (1-indexed)
    pub band: u32,
    /// Minimum pixel value
    pub min: f64,
    /// Maximum pixel value
    pub max: f64,
    /// Mean pixel value
    pub mean: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Number of valid (non-nodata) pixels
    pub valid_count: u64,
    /// Histogram bin counts (uniform spacing from min to max)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub histogram: Option<Vec<u64>>,
}

/// Statistics for a single vector field
#[derive(Debug, Clone, Serialize)]
pub struct FieldStats {
    /// Field name
    pub name: String,
    /// Number of non-null values
    pub count: u64,
    /// Number of null/missing values
    pub null_count: u64,
    /// Minimum numeric value (numeric fields only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum numeric value (numeric fields only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Mean numeric value (numeric fields only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    /// Number of distinct string values (string fields only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distinct_count: Option<usize>,
    /// Inferred type of the field
    pub field_type: String,
}

/// Statistics for a vector dataset
#[derive(Debug, Clone, Serialize)]
pub struct VectorStats {
    /// Total number of features
    pub feature_count: usize,
    /// Geometry type of features
    pub geometry_type: String,
    /// Per-field statistics
    pub fields: Vec<FieldStats>,
}

/// Top-level dataset statistics — raster or vector
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DatasetStats {
    Raster {
        /// Format identifier
        format: String,
        /// Image width in pixels
        width: u64,
        /// Image height in pixels
        height: u64,
        /// Total number of bands in the file
        band_count: u32,
        /// Per-band statistics
        bands: Vec<RasterBandStats>,
    },
    Vector {
        /// Format identifier
        format: String,
        #[serde(flatten)]
        stats: VectorStats,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Execute the stats command
pub fn execute(args: StatsArgs, format: OutputFormat) -> Result<()> {
    let stats = compute_stats(&args)?;
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&stats)
                .context("Failed to serialize stats to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            print_stats_text(&stats);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core dispatcher
// ---------------------------------------------------------------------------

/// Compute statistics from the given args, dispatching on detected file format
pub fn compute_stats(args: &StatsArgs) -> Result<DatasetStats> {
    // Reject cloud URIs early
    if crate::util::cloud::is_cloud_uri(&args.input) || args.input.starts_with("file://") {
        anyhow::bail!(
            "cloud URI and file:// paths are not supported for stats; \
             use a local file path (got: {})",
            args.input
        );
    }

    let path = Path::new(&args.input);
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let detected = crate::util::detect_format(path)
        .ok_or_else(|| anyhow::anyhow!("Unknown file format: {}", path.display()))?;

    match detected {
        "GeoTIFF" => compute_raster_stats(path, args),
        "GeoJSON" => compute_geojson_stats(path, args),
        "Shapefile" => compute_shapefile_stats(path, args),
        other => anyhow::bail!(
            "Format detected but stats not yet implemented for: {}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// Raster statistics
// ---------------------------------------------------------------------------

fn compute_raster_stats(path: &Path, args: &StatsArgs) -> Result<DatasetStats> {
    let info = read_raster_info(path)
        .with_context(|| format!("Failed to read raster info: {}", path.display()))?;

    let total_bands = info.bands;
    let width = info.width;
    let height = info.height;

    // Resolve the set of 1-indexed bands to process
    let bands_to_process: Vec<u32> = if args.band.is_empty() {
        (1..=total_bands).collect()
    } else {
        for &b in &args.band {
            if b == 0 || b > total_bands {
                anyhow::bail!(
                    "Band {} is out of range; file has {} band(s) (1-indexed)",
                    b,
                    total_bands
                );
            }
        }
        args.band.clone()
    };

    let bin_count = if args.histogram_bins == 0 {
        1
    } else {
        args.histogram_bins
    };

    let mut band_stats_list = Vec::with_capacity(bands_to_process.len());

    for band_1indexed in bands_to_process {
        // Reader uses 0-indexed bands
        let band_0indexed = band_1indexed - 1;

        let buffer = read_band_region(path, band_0indexed, 0, 0, width, height)
            .with_context(|| format!("Failed to read band {} data", band_1indexed))?;

        let buf_stats = buffer
            .compute_statistics_with_histogram(bin_count)
            .with_context(|| format!("Failed to compute statistics for band {}", band_1indexed))?;

        band_stats_list.push(RasterBandStats {
            band: band_1indexed,
            min: buf_stats.min,
            max: buf_stats.max,
            mean: buf_stats.mean,
            std_dev: buf_stats.std_dev,
            valid_count: buf_stats.valid_count,
            histogram: buf_stats.histogram,
        });
    }

    Ok(DatasetStats::Raster {
        format: "GeoTIFF".to_string(),
        width,
        height,
        band_count: total_bands,
        bands: band_stats_list,
    })
}

// ---------------------------------------------------------------------------
// GeoJSON vector statistics
// ---------------------------------------------------------------------------

fn compute_geojson_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open GeoJSON file: {}", path.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let collection = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON feature collection")?;

    let feature_count = collection.features.len();

    let geometry_type = collection
        .features
        .first()
        .and_then(|f| f.geometry.as_ref())
        .map(|g| format!("{:?}", g))
        .unwrap_or_else(|| "Unknown".to_string());

    // Accumulate field data from all features using serde_json::Value properties
    // Each feature's properties is Option<serde_json::Value> (an Object).
    let mut field_accumulator: HashMap<String, FieldAccumulator> = HashMap::new();

    for feature in &collection.features {
        if let Some(props) = &feature.properties {
            for (key, val) in props {
                let acc = field_accumulator
                    .entry(key.clone())
                    .or_insert_with(FieldAccumulator::new);
                acc.push_json_value(val);
            }
        }
    }

    let fields: Vec<FieldStats> = {
        let mut sorted_keys: Vec<String> = field_accumulator.keys().cloned().collect();
        sorted_keys.sort();
        sorted_keys
            .into_iter()
            .filter_map(|k| field_accumulator.remove(&k).map(|acc| acc.finalize(k)))
            .collect()
    };

    Ok(DatasetStats::Vector {
        format: "GeoJSON".to_string(),
        stats: VectorStats {
            feature_count,
            geometry_type,
            fields,
        },
    })
}

// ---------------------------------------------------------------------------
// Shapefile vector statistics
// ---------------------------------------------------------------------------

fn compute_shapefile_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let reader = ShapefileReader::open(path)
        .with_context(|| format!("Failed to open Shapefile: {}", path.display()))?;

    let header = reader.header();
    let geometry_type = format!("{:?}", header.shape_type);

    let features = reader
        .read_features()
        .context("Failed to read Shapefile features")?;

    let feature_count = features.len();

    // Accumulate field data using FieldValue
    let mut field_accumulator: HashMap<String, FieldAccumulator> = HashMap::new();

    for feature in &features {
        for (key, val) in &feature.attributes {
            let acc = field_accumulator
                .entry(key.clone())
                .or_insert_with(FieldAccumulator::new);
            acc.push_field_value(val);
        }
    }

    let fields: Vec<FieldStats> = {
        let mut sorted_keys: Vec<String> = field_accumulator.keys().cloned().collect();
        sorted_keys.sort();
        sorted_keys
            .into_iter()
            .filter_map(|k| field_accumulator.remove(&k).map(|acc| acc.finalize(k)))
            .collect()
    };

    Ok(DatasetStats::Vector {
        format: "Shapefile".to_string(),
        stats: VectorStats {
            feature_count,
            geometry_type,
            fields,
        },
    })
}

// ---------------------------------------------------------------------------
// Field accumulator — collects typed values for a single field across features
// ---------------------------------------------------------------------------

/// Categorised field observations used to build [`FieldStats`]
#[derive(Debug)]
enum FieldKind {
    /// Numeric observations (f64 converted)
    Numeric(Vec<f64>),
    /// Boolean observations
    Bool(Vec<bool>),
    /// String observations (for distinct count)
    Text(Vec<String>),
    /// Unknown / heterogeneous
    Mixed,
}

/// Mutable accumulator for a single field
struct FieldAccumulator {
    kind: Option<FieldKind>,
    null_count: u64,
    total: u64,
}

impl FieldAccumulator {
    fn new() -> Self {
        Self {
            kind: None,
            null_count: 0,
            total: 0,
        }
    }

    /// Accept a serde_json value (GeoJSON path)
    fn push_json_value(&mut self, val: &JsonValue) {
        self.total += 1;
        match val {
            JsonValue::Null => {
                self.null_count += 1;
            }
            JsonValue::Bool(b) => {
                self.push_bool(*b);
            }
            JsonValue::Number(n) => {
                let v = n.as_f64().unwrap_or_else(|| {
                    // Fallback for integers that don't fit f64 exactly
                    n.as_i64().map(|i| i as f64).unwrap_or(0.0)
                });
                self.push_numeric(v);
            }
            JsonValue::String(s) => {
                self.push_string(s.clone());
            }
            _ => {
                // Arrays and objects become Mixed
                self.kind = Some(FieldKind::Mixed);
            }
        }
    }

    /// Accept a FieldValue (Shapefile feature attributes use oxigdal_core::vector::FieldValue)
    fn push_field_value(&mut self, val: &oxigdal_core::vector::FieldValue) {
        use oxigdal_core::vector::FieldValue;
        self.total += 1;
        match val {
            FieldValue::Null => {
                self.null_count += 1;
            }
            FieldValue::Bool(b) => {
                self.push_bool(*b);
            }
            FieldValue::Integer(i) => {
                self.push_numeric(*i as f64);
            }
            FieldValue::UInteger(u) => {
                self.push_numeric(*u as f64);
            }
            FieldValue::Float(f) => {
                self.push_numeric(*f);
            }
            FieldValue::String(s) => {
                self.push_string(s.clone());
            }
            FieldValue::Date(_) => {
                // Treat date values as mixed/opaque — no numeric summary
                self.kind = Some(FieldKind::Mixed);
            }
            FieldValue::Blob(_) | FieldValue::Array(_) | FieldValue::Object(_) => {
                self.kind = Some(FieldKind::Mixed);
            }
        }
    }

    fn push_numeric(&mut self, v: f64) {
        match &mut self.kind {
            None => {
                self.kind = Some(FieldKind::Numeric(vec![v]));
            }
            Some(FieldKind::Numeric(nums)) => {
                nums.push(v);
            }
            _ => {
                self.kind = Some(FieldKind::Mixed);
            }
        }
    }

    fn push_bool(&mut self, b: bool) {
        match &mut self.kind {
            None => {
                self.kind = Some(FieldKind::Bool(vec![b]));
            }
            Some(FieldKind::Bool(bools)) => {
                bools.push(b);
            }
            _ => {
                self.kind = Some(FieldKind::Mixed);
            }
        }
    }

    fn push_string(&mut self, s: String) {
        match &mut self.kind {
            None => {
                self.kind = Some(FieldKind::Text(vec![s]));
            }
            Some(FieldKind::Text(strings)) => {
                strings.push(s);
            }
            _ => {
                self.kind = Some(FieldKind::Mixed);
            }
        }
    }

    /// Consume this accumulator and produce a [`FieldStats`]
    fn finalize(self, name: String) -> FieldStats {
        let count = self.total - self.null_count;

        match self.kind {
            Some(FieldKind::Numeric(nums)) if !nums.is_empty() => {
                let min = nums
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(f64::MAX, f64::min);
                let max = nums
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(f64::MIN, f64::max);
                let sum: f64 = nums.iter().copied().filter(|v| v.is_finite()).sum();
                let finite_count = nums.iter().filter(|v| v.is_finite()).count() as f64;
                let mean = if finite_count > 0.0 {
                    sum / finite_count
                } else {
                    f64::NAN
                };

                FieldStats {
                    name,
                    count,
                    null_count: self.null_count,
                    min: if min == f64::MAX { None } else { Some(min) },
                    max: if max == f64::MIN { None } else { Some(max) },
                    mean: if mean.is_finite() { Some(mean) } else { None },
                    distinct_count: None,
                    field_type: "numeric".to_string(),
                }
            }
            Some(FieldKind::Bool(bools)) => {
                let true_count = bools.iter().filter(|&&b| b).count();
                FieldStats {
                    name,
                    count,
                    null_count: self.null_count,
                    min: Some(0.0),
                    max: Some(if bools.is_empty() { 0.0 } else { 1.0 }),
                    mean: if bools.is_empty() {
                        None
                    } else {
                        Some(true_count as f64 / bools.len() as f64)
                    },
                    distinct_count: Some(2),
                    field_type: "boolean".to_string(),
                }
            }
            Some(FieldKind::Text(strings)) => {
                let mut distinct: std::collections::HashSet<&str> =
                    std::collections::HashSet::new();
                for s in &strings {
                    distinct.insert(s.as_str());
                }
                FieldStats {
                    name,
                    count,
                    null_count: self.null_count,
                    min: None,
                    max: None,
                    mean: None,
                    distinct_count: Some(distinct.len()),
                    field_type: "string".to_string(),
                }
            }
            _ => {
                // Mixed, empty, or unknown
                FieldStats {
                    name,
                    count,
                    null_count: self.null_count,
                    min: None,
                    max: None,
                    mean: None,
                    distinct_count: None,
                    field_type: "mixed".to_string(),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Text output
// ---------------------------------------------------------------------------

/// Print statistics in human-readable text format
pub fn print_stats_text(stats: &DatasetStats) {
    match stats {
        DatasetStats::Raster {
            format,
            width,
            height,
            band_count,
            bands,
        } => {
            println!("{}", style("Raster Statistics").bold().cyan());
            println!("  Format:     {}", format);
            println!("  Dimensions: {} x {}", width, height);
            println!("  Bands:      {}", band_count);
            println!();

            for band_stat in bands {
                println!(
                    "{}",
                    style(format!("Band {}", band_stat.band)).bold().yellow()
                );
                if band_stat.min.is_nan() {
                    println!("  (no valid pixels)");
                } else {
                    println!("  Min:         {:.6}", band_stat.min);
                    println!("  Max:         {:.6}", band_stat.max);
                    println!("  Mean:        {:.6}", band_stat.mean);
                    println!("  Std Dev:     {:.6}", band_stat.std_dev);
                    println!("  Valid Count: {}", band_stat.valid_count);
                    if let Some(hist) = &band_stat.histogram {
                        let total: u64 = hist.iter().sum();
                        let non_zero_bins = hist.iter().filter(|&&c| c > 0).count();
                        println!(
                            "  Histogram:   {} bins, {} non-zero, {} total",
                            hist.len(),
                            non_zero_bins,
                            total
                        );
                    }
                }
                println!();
            }
        }
        DatasetStats::Vector { format, stats } => {
            println!("{}", style("Vector Statistics").bold().cyan());
            println!("  Format:   {}", format);
            println!("  Features: {}", stats.feature_count);
            println!("  Geometry: {}", stats.geometry_type);
            println!();

            if stats.fields.is_empty() {
                println!("  (no attribute fields)");
                return;
            }

            println!("{}", style("Fields").bold().cyan());
            for field in &stats.fields {
                println!(
                    "  {} [{}]  count={}, nulls={}",
                    style(&field.name).bold(),
                    field.field_type,
                    field.count,
                    field.null_count
                );
                if let (Some(min), Some(max), Some(mean)) = (field.min, field.max, field.mean) {
                    println!("    min={:.6}  max={:.6}  mean={:.6}", min, max, mean);
                }
                if let Some(distinct) = field.distinct_count {
                    println!("    distinct values: {}", distinct);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_args_defaults() {
        // Verify default histogram_bins is usable
        let args = StatsArgs {
            input: "test.tif".to_string(),
            histogram_bins: 256,
            band: vec![],
            approx: false,
        };
        assert_eq!(args.histogram_bins, 256);
        assert!(args.band.is_empty());
        assert!(!args.approx);
    }

    #[test]
    fn test_stats_nonexistent_file_returns_error() {
        let args = StatsArgs {
            input: "/nonexistent/path/totally/fake.tif".to_string(),
            histogram_bins: 256,
            band: vec![],
            approx: false,
        };
        let result = compute_stats(&args);
        assert!(result.is_err());
        let err = result.expect_err("should have errored");
        assert!(
            err.to_string().contains("not found") || err.to_string().contains("File not found")
        );
    }

    #[test]
    fn test_field_accumulator_numeric() {
        let mut acc = FieldAccumulator::new();
        for v in [1.0_f64, 2.0, 3.0] {
            acc.push_json_value(&JsonValue::Number(
                serde_json::Number::from_f64(v).expect("valid f64"),
            ));
        }
        let stats = acc.finalize("score".to_string());
        assert_eq!(stats.field_type, "numeric");
        assert_eq!(stats.count, 3);
        assert_eq!(stats.null_count, 0);
        assert!((stats.min.expect("min") - 1.0).abs() < 1e-9);
        assert!((stats.max.expect("max") - 3.0).abs() < 1e-9);
        assert!((stats.mean.expect("mean") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_field_accumulator_string_distinct() {
        let mut acc = FieldAccumulator::new();
        for s in ["a", "b", "a", "c"] {
            acc.push_json_value(&JsonValue::String(s.to_string()));
        }
        let stats = acc.finalize("label".to_string());
        assert_eq!(stats.field_type, "string");
        assert_eq!(stats.count, 4);
        assert_eq!(stats.distinct_count, Some(3));
    }

    #[test]
    fn test_field_accumulator_null_tracking() {
        let mut acc = FieldAccumulator::new();
        acc.push_json_value(&JsonValue::Null);
        acc.push_json_value(&JsonValue::Number(
            serde_json::Number::from_f64(5.0).expect("valid"),
        ));
        acc.push_json_value(&JsonValue::Null);
        let stats = acc.finalize("val".to_string());
        assert_eq!(stats.null_count, 2);
        assert_eq!(stats.count, 1);
    }
}
