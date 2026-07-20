//! Stats command - Compute raster/vector statistics

use crate::OutputFormat;
use crate::util::raster::{read_band_region, read_raster_info};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigdal_copc::CopcReader;
use oxigdal_flatgeobuf::FlatGeobufReader;
use oxigdal_geojson::GeoJsonReader;
use oxigdal_geoparquet::{ColumnStatistics, GeoParquetReader, ScalarValue};
use oxigdal_gpkg::GeoPackage;
use oxigdal_jpeg2000::Jpeg2000Reader;
use oxigdal_mbtiles::MBTilesReader;
use oxigdal_pmtiles::PmTilesReader;
use oxigdal_shapefile::ShapefileReader;
use oxigdal_zarr::{FilesystemStore, ZarrV3Reader};
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
    /// Multi-table container statistics (`GeoPackage`)
    MultiLayer {
        /// Format identifier
        format: String,
        /// Number of tables/layers found
        layer_count: usize,
        /// Per-table summary
        layers: Vec<LayerStats>,
    },
    /// Tile archive statistics (PMTiles / MBTiles)
    TileArchive {
        /// Format identifier
        format: String,
        /// Total number of stored tiles
        tile_count: u64,
        /// Minimum zoom level present, when known
        #[serde(skip_serializing_if = "Option::is_none")]
        min_zoom: Option<u8>,
        /// Maximum zoom level present, when known
        #[serde(skip_serializing_if = "Option::is_none")]
        max_zoom: Option<u8>,
    },
    /// Point cloud statistics (COPC)
    PointCloud {
        /// Format identifier
        format: String,
        /// Total number of points declared in the LAS header
        point_count: u64,
        /// LAS point data record format id
        point_format: u8,
    },
}

/// Per-table summary used by [`DatasetStats::MultiLayer`]
#[derive(Debug, Clone, Serialize)]
pub struct LayerStats {
    /// Table name
    pub table_name: String,
    /// `GeoPackage` content type: `"features"`, `"tiles"`, or `"attributes"`
    pub data_type: String,
    /// Row count, when it could be determined
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_count: Option<u64>,
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
        "FlatGeobuf" => compute_flatgeobuf_stats(path, args),
        "GeoParquet" => compute_geoparquet_stats(path, args),
        "Zarr" => compute_zarr_stats(path, args),
        "GeoPackage" => compute_gpkg_stats(path, args),
        "JPEG2000" => compute_jp2_stats(path, args),
        "COPC" => compute_copc_stats(path, args),
        "PMTiles" => compute_pmtiles_stats(path, args),
        "MBTiles" => compute_mbtiles_stats(path, args),
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
// FlatGeobuf vector statistics
// ---------------------------------------------------------------------------

fn compute_flatgeobuf_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open FlatGeobuf file: {}", path.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = FlatGeobufReader::new(buf_reader)
        .with_context(|| format!("Failed to read FlatGeobuf header: {}", path.display()))?;

    let geometry_type = format!("{:?}", reader.header().geometry_type);

    let mut field_accumulator: HashMap<String, FieldAccumulator> = HashMap::new();
    let mut feature_count = 0usize;

    while let Some(feature) = reader
        .read_feature()
        .with_context(|| "Failed to read FlatGeobuf feature")?
    {
        feature_count += 1;
        for (key, val) in &feature.properties {
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
        format: "FlatGeobuf".to_string(),
        stats: VectorStats {
            feature_count,
            geometry_type,
            fields,
        },
    })
}

// ---------------------------------------------------------------------------
// GeoParquet vector statistics
// ---------------------------------------------------------------------------

/// Aggregate per-row-group Parquet footer statistics into a single summary
/// per column. Parquet's footer only carries min/max/null-count (no sum), so
/// `mean`/`distinct_count` are intentionally left unset here rather than
/// approximated from stale/partial data.
struct ColumnAggregate {
    min: Option<f64>,
    max: Option<f64>,
    null_count: u64,
    is_numeric: bool,
}

impl ColumnAggregate {
    fn new() -> Self {
        Self {
            min: None,
            max: None,
            null_count: 0,
            is_numeric: false,
        }
    }

    fn merge(&mut self, stat: &ColumnStatistics) {
        self.null_count += stat.null_count;
        if let (Some(min_v), Some(max_v)) = (scalar_to_f64(&stat.min), scalar_to_f64(&stat.max)) {
            self.is_numeric = true;
            self.min = Some(self.min.map_or(min_v, |m| m.min(min_v)));
            self.max = Some(self.max.map_or(max_v, |m| m.max(max_v)));
        }
    }

    fn finalize(self, name: String, total_rows: u64) -> FieldStats {
        let count = total_rows.saturating_sub(self.null_count);
        FieldStats {
            name,
            count,
            null_count: self.null_count,
            min: self.min,
            max: self.max,
            mean: None,
            distinct_count: None,
            field_type: if self.is_numeric {
                "numeric".to_string()
            } else {
                "other".to_string()
            },
        }
    }
}

fn scalar_to_f64(v: &ScalarValue) -> Option<f64> {
    match v {
        ScalarValue::Int32(i) => Some(f64::from(*i)),
        ScalarValue::Int64(i) => Some(*i as f64),
        ScalarValue::Float32(f) => Some(f64::from(*f)),
        ScalarValue::Float64(f) => Some(*f),
        _ => None,
    }
}

fn compute_geoparquet_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let reader = GeoParquetReader::open(path)
        .with_context(|| format!("Failed to read GeoParquet: {}", path.display()))?;

    let feature_count = reader.num_rows().max(0) as u64;
    let geometry_column = reader.geometry_column_name().to_string();
    let geometry_type = reader
        .metadata()
        .get_column(&geometry_column)
        .and_then(|c| c.geometry_types.first().cloned())
        .unwrap_or_else(|| "Unknown".to_string());

    let mut aggregated: HashMap<String, ColumnAggregate> = HashMap::new();
    for row_group in reader.row_group_statistics() {
        for col_stat in row_group {
            if col_stat.name == geometry_column {
                continue;
            }
            aggregated
                .entry(col_stat.name.clone())
                .or_insert_with(ColumnAggregate::new)
                .merge(&col_stat);
        }
    }

    let fields: Vec<FieldStats> = {
        let mut sorted_names: Vec<String> = aggregated.keys().cloned().collect();
        sorted_names.sort();
        sorted_names
            .into_iter()
            .filter_map(|name| {
                aggregated
                    .remove(&name)
                    .map(|agg| agg.finalize(name, feature_count))
            })
            .collect()
    };

    Ok(DatasetStats::Vector {
        format: "GeoParquet".to_string(),
        stats: VectorStats {
            feature_count: feature_count as usize,
            geometry_type,
            fields,
        },
    })
}

// ---------------------------------------------------------------------------
// Zarr raster statistics
// ---------------------------------------------------------------------------

/// Decode a Zarr array's raw (already-uncompressed) bytes into `f64` samples
/// according to its declared dtype string.
fn decode_zarr_samples(raw: &[u8], dtype: &str) -> Result<Vec<f64>> {
    match dtype {
        "uint8" | "u1" => Ok(raw.iter().map(|&b| f64::from(b)).collect()),
        "int8" | "i1" => Ok(raw.iter().map(|&b| f64::from(b as i8)).collect()),
        "bool" => Ok(raw
            .iter()
            .map(|&b| if b != 0 { 1.0 } else { 0.0 })
            .collect()),
        "uint16" | "u2" => Ok(raw
            .chunks_exact(2)
            .map(|c| f64::from(u16::from_le_bytes([c[0], c[1]])))
            .collect()),
        "int16" | "i2" => Ok(raw
            .chunks_exact(2)
            .map(|c| f64::from(i16::from_le_bytes([c[0], c[1]])))
            .collect()),
        "uint32" | "u4" => Ok(raw
            .chunks_exact(4)
            .map(|c| f64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect()),
        "int32" | "i4" => Ok(raw
            .chunks_exact(4)
            .map(|c| f64::from(i32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect()),
        "float32" | "f4" => Ok(raw
            .chunks_exact(4)
            .map(|c| f64::from(f32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect()),
        "float64" | "f8" => Ok(raw
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap_or([0u8; 8])))
            .collect()),
        "uint64" | "u8" => Ok(raw
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap_or([0u8; 8])) as f64)
            .collect()),
        "int64" | "i8" => Ok(raw
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes(c.try_into().unwrap_or([0u8; 8])) as f64)
            .collect()),
        other => anyhow::bail!("Unsupported Zarr data type for stats: {}", other),
    }
}

/// Compute `(min, max, mean, std_dev, valid_count)` over finite samples.
fn summarize_f64(values: &[f64]) -> (f64, f64, f64, f64, u64) {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return (f64::NAN, f64::NAN, f64::NAN, f64::NAN, 0);
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = finite.iter().sum();
    let count = finite.len() as f64;
    let mean = sum / count;
    let variance = finite.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
    let std_dev = if variance > 0.0 { variance.sqrt() } else { 0.0 };
    (min, max, mean, std_dev, finite.len() as u64)
}

/// Compute whole-array statistics for a Zarr v3 array by reading and
/// decoding every chunk. The array is flattened row-major into a single
/// "band": `width` is the innermost (fastest-varying) dimension and `height`
/// is the product of all outer dimensions.
///
/// Zarr v2 arrays are not supported here: `oxigdal_zarr::ZarrReaderV2` does
/// not yet implement chunk decoding, so there is no data path to read
/// sample values from a v2 store.
fn compute_zarr_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let store = FilesystemStore::open_readonly(path)
        .with_context(|| format!("Failed to open Zarr store: {}", path.display()))?;
    let reader = ZarrV3Reader::new(store, "").with_context(|| {
        format!(
            "Failed to read Zarr array metadata (only Zarr v3 stats are supported): {}",
            path.display()
        )
    })?;

    let shape = reader.shape().to_vec();
    if shape.is_empty() {
        anyhow::bail!("Zarr array has no dimensions: {}", path.display());
    }
    let dtype = reader.metadata().data_type.as_str().to_string();

    let ranges: Vec<std::ops::Range<usize>> = shape.iter().map(|&s| 0..s).collect();
    let raw = reader
        .read_slice(&ranges)
        .with_context(|| format!("Failed to read Zarr array data: {}", path.display()))?;

    let values = decode_zarr_samples(&raw, &dtype)?;
    let (min, max, mean, std_dev, valid_count) = summarize_f64(&values);

    let width = *shape.last().unwrap_or(&0) as u64;
    let height: usize = shape[..shape.len().saturating_sub(1)].iter().product();

    Ok(DatasetStats::Raster {
        format: "Zarr".to_string(),
        width,
        height: height as u64,
        band_count: 1,
        bands: vec![RasterBandStats {
            band: 1,
            min,
            max,
            mean,
            std_dev,
            valid_count,
            histogram: None,
        }],
    })
}

// ---------------------------------------------------------------------------
// GeoPackage multi-layer statistics
// ---------------------------------------------------------------------------

fn compute_gpkg_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let data =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    let mut gp = GeoPackage::from_bytes(data)
        .with_context(|| format!("Failed to parse GeoPackage: {}", path.display()))?;
    gp.load_contents()
        .with_context(|| "Failed to load gpkg_contents")?;

    let layers: Vec<LayerStats> = gp
        .contents
        .iter()
        .map(|content| LayerStats {
            table_name: content.table_name.clone(),
            data_type: content.data_type.as_str().to_string(),
            feature_count: gp.count_table_rows(&content.table_name).ok().flatten(),
        })
        .collect();

    Ok(DatasetStats::MultiLayer {
        format: "GeoPackage".to_string(),
        layer_count: layers.len(),
        layers,
    })
}

// ---------------------------------------------------------------------------
// JPEG2000 raster statistics (per-channel, via full RGB decode)
// ---------------------------------------------------------------------------

fn compute_jp2_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let file =
        File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    let buf_reader = BufReader::new(file);
    let mut decoder = Jpeg2000Reader::new(buf_reader)
        .with_context(|| format!("Failed to read JPEG2000: {}", path.display()))?;
    decoder
        .parse_headers()
        .with_context(|| format!("Failed to parse JPEG2000 headers: {}", path.display()))?;

    let info = decoder
        .info()
        .map_err(|e| anyhow::anyhow!("Failed to read JPEG2000 image info: {}", e))?;

    let rgb = decoder
        .decode_rgb()
        .map_err(|e| anyhow::anyhow!("Failed to decode JPEG2000 image: {}", e))?;

    let mut mins = [f64::MAX; 3];
    let mut maxs = [f64::MIN; 3];
    let mut sums = [0f64; 3];
    let mut sums_sq = [0f64; 3];
    let mut count = 0u64;

    for pixel in rgb.chunks_exact(3) {
        for (channel, &sample) in pixel.iter().enumerate() {
            let v = f64::from(sample);
            mins[channel] = mins[channel].min(v);
            maxs[channel] = maxs[channel].max(v);
            sums[channel] += v;
            sums_sq[channel] += v * v;
        }
        count += 1;
    }

    let bands: Vec<RasterBandStats> = (0..3)
        .map(|channel| {
            let mean = if count > 0 {
                sums[channel] / count as f64
            } else {
                f64::NAN
            };
            let variance = if count > 0 {
                (sums_sq[channel] / count as f64) - mean * mean
            } else {
                f64::NAN
            };
            let std_dev = if variance.is_finite() && variance > 0.0 {
                variance.sqrt()
            } else {
                0.0
            };
            RasterBandStats {
                band: channel as u32 + 1,
                min: mins[channel],
                max: maxs[channel],
                mean,
                std_dev,
                valid_count: count,
                histogram: None,
            }
        })
        .collect();

    Ok(DatasetStats::Raster {
        format: "JPEG2000".to_string(),
        width: u64::from(info.width),
        height: u64::from(info.height),
        band_count: 3,
        bands,
    })
}

// ---------------------------------------------------------------------------
// COPC point cloud statistics
// ---------------------------------------------------------------------------

fn compute_copc_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let data =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    let reader = CopcReader::from_bytes(&data)
        .map_err(|e| anyhow::anyhow!("Failed to parse COPC: {}", e))?;
    let header = reader.header();

    Ok(DatasetStats::PointCloud {
        format: "COPC".to_string(),
        point_count: header.number_of_point_records,
        point_format: header.point_data_format_id,
    })
}

// ---------------------------------------------------------------------------
// PMTiles / MBTiles tile archive statistics
// ---------------------------------------------------------------------------

fn compute_pmtiles_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let data =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    let reader = PmTilesReader::from_bytes(data)
        .with_context(|| format!("Failed to parse PMTiles: {}", path.display()))?;

    Ok(DatasetStats::TileArchive {
        format: "PMTiles".to_string(),
        tile_count: reader.header.addressed_tiles,
        min_zoom: Some(reader.header.min_zoom),
        max_zoom: Some(reader.header.max_zoom),
    })
}

fn compute_mbtiles_stats(path: &Path, _args: &StatsArgs) -> Result<DatasetStats> {
    let reader = MBTilesReader::open(path)
        .with_context(|| format!("Failed to open MBTiles: {}", path.display()))?;
    let tile_count = reader
        .tile_count()
        .with_context(|| "Failed to count tiles")? as u64;
    let meta = reader.metadata();

    Ok(DatasetStats::TileArchive {
        format: "MBTiles".to_string(),
        tile_count,
        min_zoom: meta.minzoom,
        max_zoom: meta.maxzoom,
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
                if let (Some(min), Some(max)) = (field.min, field.max) {
                    match field.mean {
                        Some(mean) => {
                            println!("    min={:.6}  max={:.6}  mean={:.6}", min, max, mean)
                        }
                        None => println!("    min={:.6}  max={:.6}", min, max),
                    }
                }
                if let Some(distinct) = field.distinct_count {
                    println!("    distinct values: {}", distinct);
                }
            }
        }
        DatasetStats::MultiLayer {
            format,
            layer_count,
            layers,
        } => {
            println!("{}", style("GeoPackage Statistics").bold().cyan());
            println!("  Format: {}", format);
            println!("  Layers: {}", layer_count);
            println!();

            for layer in layers {
                println!("{}", style(&layer.table_name).bold().yellow());
                println!("  Type: {}", layer.data_type);
                if let Some(count) = layer.feature_count {
                    println!("  Rows: {}", count);
                }
                println!();
            }
        }
        DatasetStats::TileArchive {
            format,
            tile_count,
            min_zoom,
            max_zoom,
        } => {
            println!("{}", style("Tile Archive Statistics").bold().cyan());
            println!("  Format: {}", format);
            println!("  Tiles:  {}", tile_count);
            if let (Some(min_zoom), Some(max_zoom)) = (min_zoom, max_zoom) {
                println!("  Zoom:   {}-{}", min_zoom, max_zoom);
            }
        }
        DatasetStats::PointCloud {
            format,
            point_count,
            point_format,
        } => {
            println!("{}", style("Point Cloud Statistics").bold().cyan());
            println!("  Format:       {}", format);
            println!("  Points:       {}", point_count);
            println!("  Point Format: {}", point_format);
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

    fn dummy_args() -> StatsArgs {
        StatsArgs {
            input: String::new(),
            histogram_bins: 32,
            band: vec![],
            approx: false,
        }
    }

    #[test]
    fn test_compute_flatgeobuf_stats_synthesized_fixture() {
        // Uses a synthesized homogeneous-Point fixture rather than the demo
        // `iron-belt.fgb` (GeometryCollection header + heterogeneous features)
        // — see `test_fixtures::flatgeobuf_fixture_path` for why.
        let path = crate::commands::test_fixtures::flatgeobuf_fixture_path();
        let stats = compute_flatgeobuf_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::Vector { format, stats } => {
                assert_eq!(format, "FlatGeobuf");
                assert_eq!(stats.feature_count, 3);
            }
            other => panic!("expected Vector stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_compute_geoparquet_stats_synthesized_fixture() {
        let path = crate::commands::test_fixtures::geoparquet_fixture_path();
        let stats = compute_geoparquet_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::Vector { format, stats } => {
                assert_eq!(format, "GeoParquet");
                assert_eq!(stats.feature_count, 4);
            }
            other => panic!("expected Vector stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    // Ignored pending an upstream fix in the `oxiarc-zstd` dependency.
    //
    // `compute_zarr_stats` decodes the fixture's chunks, which invokes
    // `oxiarc_zstd::decode_all`. oxiarc-zstd 0.3.5 (the published version this
    // workspace depends on) and 0.3.6 (local) both panic in `fse.rs`
    // (`index out of bounds: the len is 32 but the index is 65526`) while
    // FSE-decoding the zstd "sequences" section: the reconstructed FSE
    // normalized counts do not sum to `table_size`, which leaves an unassigned
    // decode-table cell whose `baseline` underflows and produces an
    // out-of-range state.
    //
    // The fixture itself is VALID zstd — the reference `zstd` CLI decodes each
    // 64x64 float32 chunk to the expected 16384 bytes. This is therefore a
    // decoder bug in oxiarc-zstd, not in OxiGDAL. It cannot be fixed from this
    // workspace: the fix (and a subsequent crates.io release of oxiarc-zstd)
    // lives in the `oxiarc` project. The sibling metadata-only test
    // `commands::info::tests::test_read_zarr_info_demo_fixture` still exercises
    // this fixture and passes. Remove `#[ignore]` once the workspace depends on
    // a fixed oxiarc-zstd.
    #[test]
    #[ignore = "upstream oxiarc-zstd FSE decoder panics on valid zstd frames (fse.rs OOB, state 65526); requires an oxiarc-zstd release"]
    fn test_compute_zarr_stats_demo_fixture() {
        let path = crate::commands::test_fixtures::demo_fixture("demo/cog-viewer/iron-belt.zarr");
        let stats = compute_zarr_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::Raster {
                format,
                width,
                height,
                band_count,
                bands,
            } => {
                assert_eq!(format, "Zarr");
                assert_eq!(width, 512);
                assert_eq!(height, 512);
                assert_eq!(band_count, 1);
                assert_eq!(bands.len(), 1);
                assert!(bands[0].valid_count > 0);
                assert!(bands[0].min.is_finite());
                assert!(bands[0].max.is_finite());
                assert!(bands[0].max >= bands[0].min);
            }
            other => panic!("expected Raster stats, got {other:?}"),
        }
    }

    #[test]
    fn test_compute_gpkg_stats_synthesized_fixture() {
        let path = crate::commands::test_fixtures::gpkg_fixture_path();
        let stats = compute_gpkg_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::MultiLayer {
                format,
                layer_count,
                layers,
            } => {
                assert_eq!(format, "GeoPackage");
                assert_eq!(layer_count, 1);
                assert_eq!(layers[0].table_name, "cities");
                assert_eq!(layers[0].feature_count, Some(3));
            }
            other => panic!("expected MultiLayer stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_compute_mbtiles_stats_synthesized_fixture() {
        let path = crate::commands::test_fixtures::mbtiles_fixture_path();
        let stats = compute_mbtiles_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::TileArchive {
                format,
                tile_count,
                min_zoom,
                max_zoom,
            } => {
                assert_eq!(format, "MBTiles");
                assert_eq!(tile_count, 3);
                assert_eq!(min_zoom, Some(0));
                assert_eq!(max_zoom, Some(1));
            }
            other => panic!("expected TileArchive stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_compute_pmtiles_stats_synthesized_fixture() {
        let path = crate::commands::test_fixtures::pmtiles_fixture_path();
        let stats = compute_pmtiles_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::TileArchive {
                format,
                tile_count,
                min_zoom,
                max_zoom,
            } => {
                assert_eq!(format, "PMTiles");
                assert_eq!(tile_count, 3);
                assert_eq!(min_zoom, Some(0));
                assert_eq!(max_zoom, Some(1));
            }
            other => panic!("expected TileArchive stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_compute_copc_stats_synthesized_fixture() {
        let bounds = ([0.0, 0.0, 0.0], [100.0, 100.0, 50.0]);
        let path = crate::commands::test_fixtures::copc_fixture_path(4242, bounds);
        let stats = compute_copc_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::PointCloud {
                format,
                point_count,
                point_format,
            } => {
                assert_eq!(format, "COPC");
                assert_eq!(point_count, 4242);
                assert_eq!(point_format, 6);
            }
            other => panic!("expected PointCloud stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_compute_jp2_stats_synthesized_fixture() {
        let path = crate::commands::test_fixtures::j2k_fixture_path();
        let stats = compute_jp2_stats(&path, &dummy_args()).expect("compute stats");
        match stats {
            DatasetStats::Raster {
                format,
                width,
                height,
                band_count,
                bands,
            } => {
                assert_eq!(format, "JPEG2000");
                assert_eq!(width, 4);
                assert_eq!(height, 4);
                assert_eq!(band_count, 3);
                assert_eq!(bands.len(), 3);
                for band in &bands {
                    assert_eq!(band.valid_count, 16);
                }
            }
            other => panic!("expected Raster stats, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_scalar_to_f64() {
        assert_eq!(scalar_to_f64(&ScalarValue::Int32(7)), Some(7.0));
        assert_eq!(scalar_to_f64(&ScalarValue::Int64(-3)), Some(-3.0));
        assert_eq!(scalar_to_f64(&ScalarValue::Float64(1.5)), Some(1.5));
        assert_eq!(scalar_to_f64(&ScalarValue::Utf8("x".to_string())), None);
    }

    #[test]
    fn test_decode_zarr_samples_float32() {
        let bytes = 1.5f32.to_le_bytes();
        let values = decode_zarr_samples(&bytes, "float32").expect("decode");
        assert_eq!(values.len(), 1);
        assert!((values[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn test_decode_zarr_samples_unsupported_dtype() {
        let result = decode_zarr_samples(&[0u8; 4], "complex128");
        assert!(result.is_err());
    }

    #[test]
    fn test_summarize_f64_basic() {
        let (min, max, mean, std_dev, count) = summarize_f64(&[1.0, 2.0, 3.0]);
        assert!((min - 1.0).abs() < 1e-9);
        assert!((max - 3.0).abs() < 1e-9);
        assert!((mean - 2.0).abs() < 1e-9);
        assert!(std_dev > 0.0);
        assert_eq!(count, 3);
    }
}
