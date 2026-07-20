//! Minimal file inspector — reports format / size / structure for `oxigeo inspect`.
//!
//! This is a self-contained reimplementation of the structural-summary feature
//! that previously lived in the (now disabled) `oxigeo_dev_tools` crate. It
//! drives the per-format readers already used by the CLI (`GeoTiffReader`,
//! `GeoJsonReader`) and reports only data that is actually reachable through
//! their public APIs — any field the readers do not expose is left `None`.

use anyhow::{Context, Result};
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geojson::GeoJsonReader;
use oxigeo_geotiff::GeoTiffReader;
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Structured result of inspecting a single geospatial file.
#[derive(Debug, Clone, Serialize)]
pub struct InspectionReport {
    /// The path (or URI) that was inspected.
    pub path: String,
    /// File size in bytes (0 for cloud URIs whose size is not locally known).
    pub file_size: u64,
    /// Detected format name (e.g. `GeoTIFF`, `GeoJSON`, or `Unknown`).
    pub format: String,
    /// Lower-cased file extension without the leading dot (empty if none).
    pub extension: String,
    /// True when the path looks like a cloud URI (`s3://`, `gs://`, `az://`).
    pub is_cloud: bool,
    /// Coordinate reference system description, if discoverable.
    pub crs: Option<String>,
    /// Raster structure summary, present only for raster formats.
    pub raster: Option<RasterSummary>,
    /// Vector structure summary, present only for vector formats.
    pub vector: Option<VectorSummary>,
}

/// Raster-specific structural summary.
#[derive(Debug, Clone, Serialize)]
pub struct RasterSummary {
    /// Raster width in pixels.
    pub width: u32,
    /// Raster height in pixels.
    pub height: u32,
    /// Number of bands.
    pub band_count: u32,
    /// Per-band data type names (one entry repeated per band).
    pub data_types: Vec<String>,
    /// Affine geo-transform `[origin_x, pixel_w, row_rot, origin_y, col_rot, pixel_h]`.
    pub geo_transform: Option<[f64; 6]>,
    /// NoData value, if defined.
    pub nodata: Option<f64>,
}

/// Vector-specific structural summary.
#[derive(Debug, Clone, Serialize)]
pub struct VectorSummary {
    /// Feature count, if the reader can determine it.
    pub feature_count: Option<u64>,
    /// Number of layers (GeoJSON FeatureCollections are always single-layer).
    pub layer_count: u32,
    /// Bounding box `[min_x, min_y, max_x, max_y]`, if present.
    pub bounds: Option<[f64; 4]>,
}

/// Inspect a file and produce a structured report.
///
/// When `detailed` is `true` the report additionally fills `geo_transform`,
/// `nodata`, full per-band `data_types`, and `layer_count`. When `false` only a
/// lightweight summary is produced (raster `data_types` is left empty and
/// `geo_transform`/`nodata` are `None`).
///
/// # Errors
///
/// Returns an error when the path does not exist (for local paths) or when an
/// existing file cannot be parsed by the matching format reader. Files with an
/// unrecognised extension do not error — they yield a report with format
/// `Unknown` and neither a raster nor vector summary.
pub fn inspect_file(path: &str, detailed: bool) -> Result<InspectionReport> {
    let is_cloud = crate::util::cloud::is_cloud_uri(path);

    // Resolve `file://` URIs to a plain filesystem path; cloud URIs stay as-is.
    let resolved: &str = path.strip_prefix("file://").unwrap_or(path);
    let resolved_path = Path::new(resolved);

    // File size: real metadata for local files, 0 for cloud URIs.
    let file_size = if is_cloud {
        0
    } else {
        if !resolved_path.exists() {
            anyhow::bail!("File not found: {}", resolved);
        }
        std::fs::metadata(resolved_path)
            .with_context(|| format!("Failed to read file metadata: {}", resolved))?
            .len()
    };

    // Extension: lower-cased, without the leading dot.
    let extension = resolved_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    // Format detection reuses the shared CLI helper.
    let format = crate::util::detect_format(resolved_path)
        .map(|f| f.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let mut report = InspectionReport {
        path: path.to_string(),
        file_size,
        format: format.clone(),
        extension,
        is_cloud,
        crs: None,
        raster: None,
        vector: None,
    };

    // Cloud URIs and unknown formats stop here: opening the per-format readers
    // requires a local file, and there is no honest structure data to report.
    if is_cloud || format == "Unknown" {
        return Ok(report);
    }

    match format.as_str() {
        "GeoTIFF" => {
            let (summary, crs) = inspect_geotiff(resolved_path, detailed)?;
            report.crs = crs;
            report.raster = Some(summary);
        }
        "GeoJSON" => {
            let (summary, crs) = inspect_geojson(resolved_path, detailed)?;
            report.crs = crs;
            report.vector = Some(summary);
        }
        // Other detected formats (Shapefile, FlatGeobuf, GeoParquet, Zarr, ...)
        // are recognised by extension but a structural reader is not wired here.
        // The report still carries an accurate format / size / extension.
        _ => {}
    }

    Ok(report)
}

/// Builds a [`RasterSummary`] for a GeoTIFF using `GeoTiffReader`.
fn inspect_geotiff(path: &Path, detailed: bool) -> Result<(RasterSummary, Option<String>)> {
    let source = FileDataSource::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open file {}: {e}", path.display()))?;
    let reader = GeoTiffReader::open(source)
        .map_err(|e| anyhow::anyhow!("Failed to read GeoTIFF {}: {e}", path.display()))?;

    let width = u32::try_from(reader.width()).unwrap_or(u32::MAX);
    let height = u32::try_from(reader.height()).unwrap_or(u32::MAX);
    let band_count = reader.band_count();

    // data_types: one entry per band. Only populated in detailed mode.
    let data_types = if detailed {
        let type_name = reader
            .data_type()
            .map_or_else(|| "Unknown".to_string(), data_type_name);
        vec![type_name; band_count as usize]
    } else {
        Vec::new()
    };

    // geo_transform / nodata only in detailed mode.
    let geo_transform = if detailed {
        reader.geo_transform().map(|gt| {
            [
                gt.origin_x,
                gt.pixel_width,
                gt.row_rotation,
                gt.origin_y,
                gt.col_rotation,
                gt.pixel_height,
            ]
        })
    } else {
        None
    };

    let nodata = if detailed {
        reader.nodata().as_f64()
    } else {
        None
    };

    // CRS is always reported when available — it is cheap and useful.
    let crs = reader.epsg_code().map(|code| format!("EPSG:{code}"));

    Ok((
        RasterSummary {
            width,
            height,
            band_count,
            data_types,
            geo_transform,
            nodata,
        },
        crs,
    ))
}

/// Builds a [`VectorSummary`] for a GeoJSON file using `GeoJsonReader`.
fn inspect_geojson(path: &Path, detailed: bool) -> Result<(VectorSummary, Option<String>)> {
    let file =
        File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    let mut reader = GeoJsonReader::new(BufReader::new(file));
    let collection = reader
        .read_feature_collection()
        .map_err(|e| anyhow::anyhow!("Failed to read GeoJSON {}: {e}", path.display()))?;

    let feature_count = Some(collection.features.len() as u64);

    // GeoJSON FeatureCollections are a single layer by definition. The
    // `layer_count` field exists for parity with multi-layer formats; we only
    // surface it explicitly in detailed mode but the value is the same (1).
    let layer_count = 1;

    // bbox is `Vec<f64>`; only a 4-element bbox maps to a 2D bounds array.
    let bounds = collection.bbox.as_ref().and_then(|bbox| {
        if bbox.len() >= 4 {
            Some([bbox[0], bbox[1], bbox[2], bbox[3]])
        } else {
            None
        }
    });

    // CRS: prefer the named CRS; in non-detailed mode still report it if cheap.
    let _ = detailed;
    let crs = collection.crs.as_ref().and_then(|c| c.name());

    Ok((
        VectorSummary {
            feature_count,
            layer_count,
            bounds,
        },
        crs,
    ))
}

/// Maps a [`RasterDataType`] to its display name.
fn data_type_name(dt: RasterDataType) -> String {
    match dt {
        RasterDataType::UInt8 => "UInt8",
        RasterDataType::UInt16 => "UInt16",
        RasterDataType::UInt32 => "UInt32",
        RasterDataType::UInt64 => "UInt64",
        RasterDataType::Int8 => "Int8",
        RasterDataType::Int16 => "Int16",
        RasterDataType::Int32 => "Int32",
        RasterDataType::Int64 => "Int64",
        RasterDataType::Float32 => "Float32",
        RasterDataType::Float64 => "Float64",
        RasterDataType::CFloat32 => "CFloat32",
        RasterDataType::CFloat64 => "CFloat64",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_name() {
        assert_eq!(data_type_name(RasterDataType::UInt8), "UInt8");
        assert_eq!(data_type_name(RasterDataType::Float64), "Float64");
        assert_eq!(data_type_name(RasterDataType::CFloat64), "CFloat64");
    }

    #[test]
    fn test_inspect_unknown_extension_no_summary() -> Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "oxigeo_inspector_unit_{}_{}.xyz",
            std::process::id(),
            "unknown"
        ));
        std::fs::write(&path, b"not a geospatial file")?;

        let report = inspect_file(path.to_string_lossy().as_ref(), false)?;
        assert_eq!(report.format, "Unknown");
        assert!(report.raster.is_none());
        assert!(report.vector.is_none());
        assert!(!report.is_cloud);

        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}
