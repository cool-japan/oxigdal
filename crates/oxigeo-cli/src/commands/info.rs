//! Info command - Display file metadata and information

use crate::OutputFormat;
use crate::util;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use oxigeo_copc::CopcReader;
use oxigeo_core::{io::FileDataSource, types::RasterDataType};
use oxigeo_geojson::GeoJsonReader;
use oxigeo_geoparquet::GeoParquetReader;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_gpkg::{GeoPackage, multi_geom};
use oxigeo_jpeg2000::Jpeg2000Reader;
use oxigeo_mbtiles::MBTilesReader;
use oxigeo_pmtiles::PmTilesReader;
use oxigeo_shapefile::ShapefileReader;
use oxigeo_zarr::{FilesystemStore, Store, StoreKey};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;

/// Display information about a raster or vector file
#[derive(Args, Debug)]
pub struct InfoArgs {
    /// Input file path
    #[arg(value_name = "FILE")]
    input: PathBuf,

    /// Show detailed statistics
    #[arg(short, long)]
    stats: bool,

    /// Compute min/max values
    #[arg(long)]
    compute_minmax: bool,

    /// Show all metadata
    #[arg(short, long)]
    metadata: bool,

    /// Show coordinate reference system details
    #[arg(long)]
    crs: bool,

    /// Show band/layer information
    #[arg(short, long)]
    bands: bool,
}

#[derive(Serialize)]
struct FileInfo {
    file_path: String,
    file_size: String,
    format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    raster_info: Option<RasterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vector_info: Option<VectorInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    array_info: Option<ArrayInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpkg_info: Option<GpkgInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tile_info: Option<TileArchiveInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    point_cloud_info: Option<PointCloudInfo>,
}

#[derive(Serialize)]
struct RasterInfo {
    width: u64,
    height: u64,
    bands: u32,
    data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    geotransform: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
}

#[derive(Serialize)]
struct VectorInfo {
    layer_count: usize,
    feature_count: usize,
    geometry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crs: Option<String>,
}

/// Metadata for a chunked N-dimensional Zarr array.
#[derive(Serialize)]
struct ArrayInfo {
    shape: Vec<u64>,
    chunk_shape: Vec<u64>,
    data_type: String,
    zarr_format: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    crs: Option<String>,
}

/// Summary of a single table inside a `GeoPackage`.
#[derive(Serialize)]
struct GpkgLayerSummary {
    table_name: String,
    data_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    geometry_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feature_count: Option<u64>,
    bounds: Bounds,
    #[serde(skip_serializing_if = "Option::is_none")]
    crs: Option<String>,
}

/// All layers/tables discovered inside a `GeoPackage`.
#[derive(Serialize)]
struct GpkgInfo {
    layer_count: usize,
    layers: Vec<GpkgLayerSummary>,
}

/// Metadata for a tile archive (PMTiles / MBTiles).
#[derive(Serialize)]
struct TileArchiveInfo {
    tile_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_zoom: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_zoom: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tile_format: Option<String>,
}

/// Metadata for a COPC point cloud.
#[derive(Serialize)]
struct PointCloudInfo {
    point_count: u64,
    point_format: u8,
    bounds: Bounds3D,
    #[serde(skip_serializing_if = "Option::is_none")]
    crs: Option<String>,
}

#[derive(Serialize)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

#[derive(Serialize)]
struct Bounds3D {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

pub fn execute(args: InfoArgs, format: OutputFormat) -> Result<()> {
    // If the caller supplied a cloud URI as a PathBuf it won't have a filesystem
    // presence — detect and reject early with a helpful message.
    let input_str = args.input.to_str().unwrap_or_default();
    if crate::util::cloud::is_cloud_uri(input_str) {
        eprintln!("Note: cloud URI support is experimental; full metadata may not be available");
        anyhow::bail!(
            "cloud URI reading for raster info requires GeoTiffReader<DataSource>; \
             use a local file path for now (got: {})",
            input_str
        );
    }

    // Allow file:// URIs to be passed; strip the prefix for filesystem operations.
    let resolved_path = if let Some(stripped) = input_str.strip_prefix("file://") {
        std::path::PathBuf::from(stripped)
    } else {
        args.input.clone()
    };

    // Check if file exists
    if !resolved_path.exists() {
        anyhow::bail!("File not found: {}", resolved_path.display());
    }

    // Get file size
    let metadata = fs::metadata(&resolved_path)
        .with_context(|| format!("Failed to read file metadata: {}", resolved_path.display()))?;
    let file_size = util::format_size(metadata.len());

    // Detect format
    let detected_format = util::detect_format(&resolved_path)
        .ok_or_else(|| anyhow::anyhow!("Unknown file format"))?;

    // Build a resolved InfoArgs so sub-functions get the stripped path
    let resolved_args = InfoArgs {
        input: resolved_path.clone(),
        stats: args.stats,
        compute_minmax: args.compute_minmax,
        metadata: args.metadata,
        crs: args.crs,
        bands: args.bands,
    };

    // Try to read as raster or vector
    let mut file_info = FileInfo {
        file_path: resolved_path.display().to_string(),
        file_size,
        format: detected_format.to_string(),
        raster_info: None,
        vector_info: None,
        array_info: None,
        gpkg_info: None,
        tile_info: None,
        point_cloud_info: None,
    };

    match detected_format {
        "GeoTIFF" => {
            file_info.raster_info = Some(read_geotiff_info(&resolved_args)?);
        }
        "GeoJSON" => {
            file_info.vector_info = Some(read_geojson_info(&resolved_args)?);
        }
        "Shapefile" => {
            file_info.vector_info = Some(read_shapefile_info(&resolved_args)?);
        }
        "FlatGeobuf" => {
            file_info.vector_info = Some(read_flatgeobuf_info(&resolved_args)?);
        }
        "GeoParquet" => {
            file_info.vector_info = Some(read_geoparquet_info(&resolved_args)?);
        }
        "Zarr" => {
            file_info.array_info = Some(read_zarr_info(&resolved_args)?);
        }
        "GeoPackage" => {
            file_info.gpkg_info = Some(read_gpkg_info(&resolved_args)?);
        }
        "JPEG2000" => {
            file_info.raster_info = Some(read_jp2_info(&resolved_args)?);
        }
        "COPC" => {
            file_info.point_cloud_info = Some(read_copc_info(&resolved_args)?);
        }
        "PMTiles" => {
            file_info.tile_info = Some(read_pmtiles_info(&resolved_args)?);
        }
        "MBTiles" => {
            file_info.tile_info = Some(read_mbtiles_info(&resolved_args)?);
        }
        _ => {
            anyhow::bail!(
                "Format detected but info display not yet implemented for: {}",
                detected_format
            );
        }
    };

    // Output results
    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&file_info).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            print_text_info(&file_info, &args);
        }
    }

    Ok(())
}

fn read_geotiff_info(args: &InfoArgs) -> Result<RasterInfo> {
    let source = FileDataSource::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", args.input.display()))?;

    let width = reader.width();
    let height = reader.height();
    let bands = reader.band_count();
    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;

    let geotransform = reader.geo_transform().map(|gt| {
        vec![
            gt.origin_x,
            gt.pixel_width,
            gt.row_rotation,
            gt.origin_y,
            gt.col_rotation,
            gt.pixel_height,
        ]
    });

    let projection = reader.epsg_code().map(|code| format!("EPSG:{}", code));

    // Calculate bounds from geotransform
    let bounds = geotransform.as_ref().map(|gt| {
        let min_x = gt[0];
        let max_y = gt[3];
        let max_x = min_x + gt[1] * width as f64;
        let min_y = max_y + gt[5] * height as f64;

        Bounds {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    });

    Ok(RasterInfo {
        width,
        height,
        bands,
        data_type: format_data_type(data_type),
        geotransform,
        projection,
        bounds,
    })
}

fn read_geojson_info(args: &InfoArgs) -> Result<VectorInfo> {
    let file = File::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut reader = GeoJsonReader::new(buf_reader);

    let feature_collection = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON")?;

    let feature_count = feature_collection.features.len();

    // Determine geometry type from first feature
    let geometry_type = if let Some(first_feature) = feature_collection.features.first() {
        if let Some(ref geom) = first_feature.geometry {
            format!("{:?}", geom)
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    };

    // Get bounds from feature collection
    let bounds = feature_collection.bbox.as_ref().and_then(|bbox| {
        if bbox.len() >= 4 {
            Some(Bounds {
                min_x: bbox[0],
                min_y: bbox[1],
                max_x: bbox[2],
                max_y: bbox[3],
            })
        } else {
            None
        }
    });

    let crs = feature_collection
        .crs
        .as_ref()
        .map(|crs| format!("{:?}", crs));

    Ok(VectorInfo {
        layer_count: 1,
        feature_count,
        geometry_type,
        bounds,
        crs,
    })
}

fn read_shapefile_info(args: &InfoArgs) -> Result<VectorInfo> {
    let reader = ShapefileReader::open(&args.input)
        .with_context(|| format!("Failed to open Shapefile: {}", args.input.display()))?;

    let header = reader.header();

    // Get geometry type from shapefile header
    let geometry_type = format!("{:?}", header.shape_type);

    // Get bounding box from header
    let bbox = &header.bbox;
    let bounds = Some(Bounds {
        min_x: bbox.x_min,
        min_y: bbox.y_min,
        max_x: bbox.x_max,
        max_y: bbox.y_max,
    });

    // Get feature count from index entries or by reading features
    let feature_count = if let Some(entries) = reader.index_entries() {
        entries.len()
    } else {
        // Fall back to reading features to count them
        reader
            .read_features()
            .map(|f| f.len())
            .with_context(|| "Failed to read Shapefile features for counting")?
    };

    // Get field information
    let fields = reader.field_descriptors();
    let field_names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();

    // Check for .prj file for CRS info
    let prj_path = args.input.with_extension("prj");
    let crs = if prj_path.exists() {
        fs::read_to_string(&prj_path)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    // Log field info if metadata requested (displayed in text output)
    if args.metadata && !field_names.is_empty() {
        println!("\n{}", console::style("Attribute Fields").bold().cyan());
        for field in fields {
            println!(
                "  {} ({:?}, length: {}, decimals: {})",
                field.name, field.field_type, field.length, field.decimal_count
            );
        }
    }

    Ok(VectorInfo {
        layer_count: 1,
        feature_count,
        geometry_type,
        bounds,
        crs,
    })
}

/// Read summary metadata for a `FlatGeobuf` file: the header declares the
/// dataset-wide geometry type, feature count and extent up front, so no
/// feature scan is required.
fn read_flatgeobuf_info(args: &InfoArgs) -> Result<VectorInfo> {
    let file = File::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;
    let buf_reader = BufReader::new(file);
    let reader = oxigeo_flatgeobuf::FlatGeobufReader::new(buf_reader)
        .with_context(|| format!("Failed to read FlatGeobuf: {}", args.input.display()))?;

    let header = reader.header();
    let feature_count = header.features_count.unwrap_or(0) as usize;
    let geometry_type = format!("{:?}", header.geometry_type);

    let bounds = header.extent.map(|e| Bounds {
        min_x: e[0],
        min_y: e[1],
        max_x: e[2],
        max_y: e[3],
    });

    let crs = header.crs.as_ref().map(flatgeobuf_crs_label);

    Ok(VectorInfo {
        layer_count: 1,
        feature_count,
        geometry_type,
        bounds,
        crs,
    })
}

/// Format a `FlatGeobuf` CRS record as `"ORG:CODE"` when possible, otherwise
/// fall back to embedded WKT or a debug dump.
fn flatgeobuf_crs_label(crs: &oxigeo_flatgeobuf::CrsInfo) -> String {
    match (&crs.organization, crs.organization_code) {
        (Some(org), Some(code)) => format!("{org}:{code}"),
        _ => crs
            .wkt
            .clone()
            .or_else(|| crs.name.clone())
            .unwrap_or_else(|| format!("{:?}", crs)),
    }
}

/// Read summary metadata for a GeoParquet file from its `"geo"` file
/// metadata — no row data is decoded.
fn read_geoparquet_info(args: &InfoArgs) -> Result<VectorInfo> {
    let reader = GeoParquetReader::open(&args.input)
        .with_context(|| format!("Failed to read GeoParquet: {}", args.input.display()))?;

    let feature_count = reader.num_rows().max(0) as usize;
    let metadata = reader.metadata();
    let primary = metadata.primary_column_metadata().ok();

    let geometry_type = primary
        .and_then(|c| c.geometry_types.first().cloned())
        .unwrap_or_else(|| "Unknown".to_string());

    let bounds = primary.and_then(|c| c.bbox.as_ref()).and_then(|b| {
        if b.len() >= 4 {
            Some(Bounds {
                min_x: b[0],
                min_y: b[1],
                max_x: b[2],
                max_y: b[3],
            })
        } else {
            None
        }
    });

    let crs = primary
        .and_then(|c| c.crs.as_ref())
        .map(|crs| format!("{:?}", crs));

    Ok(VectorInfo {
        layer_count: 1,
        feature_count,
        geometry_type,
        bounds,
        crs,
    })
}

/// Read shape / chunking / dtype metadata for a Zarr array store.
///
/// Tries the Zarr v3 layout (`zarr.json` at the store root) first, then
/// falls back to Zarr v2 (`.zarray` + `.zattrs`). This reads only the
/// metadata documents, never chunk data.
fn read_zarr_info(args: &InfoArgs) -> Result<ArrayInfo> {
    let store = FilesystemStore::open_readonly(&args.input)
        .with_context(|| format!("Failed to open Zarr store: {}", args.input.display()))?;

    let v3_key = StoreKey::new("zarr.json".to_string());
    if store.exists(&v3_key).unwrap_or(false) {
        let bytes = store
            .get(&v3_key)
            .with_context(|| "Failed to read zarr.json")?;
        let meta: oxigeo_zarr::metadata::v3::ArrayMetadataV3 = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse zarr.json: {}", args.input.display()))?;

        let chunk_shape = meta
            .chunk_grid
            .regular_chunk_shape()
            .map(|s| s.iter().map(|&v| v as u64).collect())
            .unwrap_or_default();

        let crs = meta
            .attributes
            .as_ref()
            .and_then(|a| a.get("crs"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        return Ok(ArrayInfo {
            shape: meta.shape.iter().map(|&v| v as u64).collect(),
            chunk_shape,
            data_type: meta.data_type.as_str().to_string(),
            zarr_format: meta.zarr_format,
            crs,
        });
    }

    let v2_key = StoreKey::new(".zarray".to_string());
    if store.exists(&v2_key).unwrap_or(false) {
        let bytes = store
            .get(&v2_key)
            .with_context(|| "Failed to read .zarray")?;
        let meta: oxigeo_zarr::metadata::v2::ArrayMetadataV2 = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse .zarray: {}", args.input.display()))?;

        let crs = read_zarr_v2_crs(&store);

        return Ok(ArrayInfo {
            shape: meta.shape.iter().map(|&v| v as u64).collect(),
            chunk_shape: meta.chunks.iter().map(|&v| v as u64).collect(),
            data_type: meta.dtype,
            zarr_format: meta.zarr_format,
            crs,
        });
    }

    anyhow::bail!(
        "Not a recognized Zarr array (missing zarr.json or .zarray): {}",
        args.input.display()
    );
}

/// Best-effort lookup of a `"crs"` attribute inside a Zarr v2 `.zattrs`
/// document. Returns `None` on any missing/malformed data rather than
/// failing the whole info request.
fn read_zarr_v2_crs(store: &FilesystemStore) -> Option<String> {
    let bytes = store.get(&StoreKey::new(".zattrs".to_string())).ok()?;
    let attrs: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    attrs.get("crs")?.as_str().map(str::to_string)
}

/// Read `gpkg_contents` / `gpkg_geometry_columns` / `gpkg_spatial_ref_sys`
/// to build a summary of every table in a `GeoPackage`.
fn read_gpkg_info(args: &InfoArgs) -> Result<GpkgInfo> {
    let data = fs::read(&args.input)
        .with_context(|| format!("Failed to read file: {}", args.input.display()))?;
    let mut gp = GeoPackage::from_bytes(data)
        .with_context(|| format!("Failed to parse GeoPackage: {}", args.input.display()))?;
    gp.load_contents()
        .with_context(|| "Failed to load gpkg_contents")?;

    let srs_map = read_gpkg_srs_map(&gp);
    // Propagate a genuine gpkg_geometry_columns scan failure (e.g. a corrupt
    // B-tree) instead of silently reporting every layer as geometry-less.
    let geom_columns = multi_geom::load_all_geometry_columns(&gp)
        .with_context(|| "Failed to load gpkg_geometry_columns")?;

    let mut layers = Vec::with_capacity(gp.contents.len());
    for content in &gp.contents {
        let geometry_type = geom_columns
            .iter()
            .find(|g| g.table_name == content.table_name)
            .and_then(|g| g.columns.first())
            .map(|c| c.geometry_type_name.clone());

        let feature_count = gp.count_table_rows(&content.table_name).ok().flatten();

        let crs = srs_map.get(&content.srs_id).cloned();

        layers.push(GpkgLayerSummary {
            table_name: content.table_name.clone(),
            data_type: content.data_type.as_str().to_string(),
            geometry_type,
            feature_count,
            bounds: Bounds {
                min_x: content.min_x,
                min_y: content.min_y,
                max_x: content.max_x,
                max_y: content.max_y,
            },
            crs,
        });
    }

    Ok(GpkgInfo {
        layer_count: layers.len(),
        layers,
    })
}

/// Scan `gpkg_spatial_ref_sys` and build an `srs_id -> "ORG:CODE"` lookup
/// table. Missing or malformed rows are skipped; an empty map is returned
/// when the system table itself is absent.
fn read_gpkg_srs_map(gp: &GeoPackage) -> HashMap<i32, String> {
    let mut map = HashMap::new();
    let rows = match gp.scan_table_by_name("gpkg_spatial_ref_sys") {
        Ok(Some(rows)) => rows,
        _ => return map,
    };

    for (_rowid, values) in &rows {
        if values.len() < 4 {
            continue;
        }
        let srs_id = match &values[1] {
            oxigeo_gpkg::CellValue::Integer(i) => *i as i32,
            _ => continue,
        };
        let organization = match &values[2] {
            oxigeo_gpkg::CellValue::Text(s) => s.clone(),
            _ => continue,
        };
        let org_code = match &values[3] {
            oxigeo_gpkg::CellValue::Integer(i) => *i,
            _ => continue,
        };
        map.insert(srs_id, format!("{organization}:{org_code}"));
    }

    map
}

/// Decode JP2/J2K headers via [`Jpeg2000Reader::info`] and report
/// dims/bands/bit-depth. This driver does not extract georeferencing
/// (no GMLJP2 support), so `geotransform`/`projection`/`bounds` are `None`.
fn read_jp2_info(args: &InfoArgs) -> Result<RasterInfo> {
    let file = File::open(&args.input)
        .with_context(|| format!("Failed to open file: {}", args.input.display()))?;
    let buf_reader = BufReader::new(file);
    let mut decoder = Jpeg2000Reader::new(buf_reader)
        .with_context(|| format!("Failed to read JPEG2000: {}", args.input.display()))?;
    decoder
        .parse_headers()
        .with_context(|| format!("Failed to parse JPEG2000 headers: {}", args.input.display()))?;

    let info = decoder
        .info()
        .map_err(|e| anyhow::anyhow!("Failed to read JPEG2000 image info: {}", e))?;

    let data_type = decoder
        .image_size_info()
        .and_then(|s| s.components.first())
        .map(|c| jp2_data_type_label(c.precision, c.is_signed))
        .unwrap_or_else(|| "Unknown".to_string());

    Ok(RasterInfo {
        width: u64::from(info.width),
        height: u64::from(info.height),
        bands: u32::from(info.num_components),
        data_type,
        geotransform: None,
        projection: None,
        bounds: None,
    })
}

/// Format a JP2 component's precision/signedness as a human-readable label,
/// e.g. `"8-bit unsigned"`.
fn jp2_data_type_label(precision: u8, is_signed: bool) -> String {
    format!(
        "{precision}-bit {}",
        if is_signed { "signed" } else { "unsigned" }
    )
}

/// Read the LAS public header + COPC info VLR. Point counts and bounds come
/// directly from the header; no octree traversal is required.
fn read_copc_info(args: &InfoArgs) -> Result<PointCloudInfo> {
    let data = fs::read(&args.input)
        .with_context(|| format!("Failed to read file: {}", args.input.display()))?;
    let reader = CopcReader::from_bytes(&data)
        .map_err(|e| anyhow::anyhow!("Failed to parse COPC: {}", e))?;

    let header = reader.header();
    let (min, max) = header.bounds();

    let crs_info = reader.crs();
    let crs = if crs_info.is_empty() {
        None
    } else {
        crs_info.wkt.clone().or_else(|| crs_info.geo_ascii.clone())
    };

    Ok(PointCloudInfo {
        point_count: header.number_of_point_records,
        point_format: header.point_data_format_id,
        bounds: Bounds3D {
            min_x: min[0],
            min_y: min[1],
            min_z: min[2],
            max_x: max[0],
            max_y: max[1],
            max_z: max[2],
        },
        crs,
    })
}

/// Read the fixed 127-byte header of a PMTiles v3 archive.
fn read_pmtiles_info(args: &InfoArgs) -> Result<TileArchiveInfo> {
    let data = fs::read(&args.input)
        .with_context(|| format!("Failed to read file: {}", args.input.display()))?;
    let reader = PmTilesReader::from_bytes(data)
        .with_context(|| format!("Failed to parse PMTiles: {}", args.input.display()))?;

    let header = &reader.header;
    let bounds = Bounds {
        min_x: header.min_lon(),
        min_y: header.min_lat(),
        max_x: header.max_lon(),
        max_y: header.max_lat(),
    };

    Ok(TileArchiveInfo {
        tile_count: header.addressed_tiles,
        min_zoom: Some(header.min_zoom),
        max_zoom: Some(header.max_zoom),
        bounds: Some(bounds),
        tile_format: Some(format!("{:?}", header.tile_type)),
    })
}

/// Open an on-disk MBTiles (SQLite) archive and read its `metadata` table.
fn read_mbtiles_info(args: &InfoArgs) -> Result<TileArchiveInfo> {
    let reader = MBTilesReader::open(&args.input)
        .with_context(|| format!("Failed to open MBTiles: {}", args.input.display()))?;

    let tile_count = reader
        .tile_count()
        .with_context(|| "Failed to count tiles")? as u64;

    let meta = reader.metadata();
    let bounds = meta.bounds.map(|b| Bounds {
        min_x: b[0],
        min_y: b[1],
        max_x: b[2],
        max_y: b[3],
    });

    Ok(TileArchiveInfo {
        tile_count,
        min_zoom: meta.minzoom,
        max_zoom: meta.maxzoom,
        bounds,
        tile_format: meta.format.as_ref().map(|f| format!("{:?}", f)),
    })
}

fn format_data_type(dt: RasterDataType) -> String {
    match dt {
        RasterDataType::UInt8 => "UInt8".to_string(),
        RasterDataType::UInt16 => "UInt16".to_string(),
        RasterDataType::UInt32 => "UInt32".to_string(),
        RasterDataType::UInt64 => "UInt64".to_string(),
        RasterDataType::Int8 => "Int8".to_string(),
        RasterDataType::Int16 => "Int16".to_string(),
        RasterDataType::Int32 => "Int32".to_string(),
        RasterDataType::Int64 => "Int64".to_string(),
        RasterDataType::Float32 => "Float32".to_string(),
        RasterDataType::Float64 => "Float64".to_string(),
        RasterDataType::CFloat32 => "CFloat32".to_string(),
        RasterDataType::CFloat64 => "CFloat64".to_string(),
    }
}

fn print_text_info(info: &FileInfo, args: &InfoArgs) {
    println!("{}", style("File Information").bold().cyan());
    println!("  Path:   {}", info.file_path);
    println!("  Size:   {}", info.file_size);
    println!("  Format: {}", info.format);
    println!();

    if let Some(ref raster) = info.raster_info {
        println!("{}", style("Raster Information").bold().cyan());
        println!("  Dimensions: {} x {}", raster.width, raster.height);
        println!("  Bands:      {}", raster.bands);
        println!("  Data Type:  {}", raster.data_type);

        if (args.crs || args.metadata)
            && let Some(ref proj) = raster.projection
        {
            println!("\n{}", style("Coordinate Reference System").bold().cyan());
            println!("  {}", proj);
        }

        if let Some(ref gt) = raster.geotransform {
            println!("\n{}", style("Geotransform").bold().cyan());
            println!("  Origin:    ({}, {})", gt[0], gt[3]);
            println!("  Pixel Size: ({}, {})", gt[1], gt[5]);
        }

        if let Some(ref bounds) = raster.bounds {
            println!("\n{}", style("Bounds").bold().cyan());
            println!("  Min X: {}", bounds.min_x);
            println!("  Min Y: {}", bounds.min_y);
            println!("  Max X: {}", bounds.max_x);
            println!("  Max Y: {}", bounds.max_y);
        }
    }

    if let Some(ref vector) = info.vector_info {
        println!("{}", style("Vector Information").bold().cyan());
        println!("  Layers:   {}", vector.layer_count);
        println!("  Features: {}", vector.feature_count);
        println!("  Geometry: {}", vector.geometry_type);

        if (args.crs || args.metadata)
            && let Some(ref crs) = vector.crs
        {
            println!("\n{}", style("Coordinate Reference System").bold().cyan());
            println!("  {}", crs);
        }

        if let Some(ref bounds) = vector.bounds {
            println!("\n{}", style("Bounds").bold().cyan());
            println!("  Min X: {}", bounds.min_x);
            println!("  Min Y: {}", bounds.min_y);
            println!("  Max X: {}", bounds.max_x);
            println!("  Max Y: {}", bounds.max_y);
        }
    }

    if let Some(ref array) = info.array_info {
        println!("{}", style("Array Information").bold().cyan());
        println!("  Shape:       {:?}", array.shape);
        println!("  Chunk Shape: {:?}", array.chunk_shape);
        println!("  Data Type:   {}", array.data_type);
        println!("  Zarr Format: v{}", array.zarr_format);

        if (args.crs || args.metadata)
            && let Some(ref crs) = array.crs
        {
            println!("\n{}", style("Coordinate Reference System").bold().cyan());
            println!("  {}", crs);
        }
    }

    if let Some(ref gpkg) = info.gpkg_info {
        println!("{}", style("GeoPackage Layers").bold().cyan());
        println!("  Layer Count: {}", gpkg.layer_count);
        for layer in &gpkg.layers {
            println!();
            println!("{}", style(&layer.table_name).bold().yellow());
            println!("  Type: {}", layer.data_type);
            if let Some(ref geom_type) = layer.geometry_type {
                println!("  Geometry: {}", geom_type);
            }
            if let Some(count) = layer.feature_count {
                println!("  Rows: {}", count);
            }
            println!(
                "  Bounds: ({}, {}) - ({}, {})",
                layer.bounds.min_x, layer.bounds.min_y, layer.bounds.max_x, layer.bounds.max_y
            );
            if (args.crs || args.metadata)
                && let Some(ref crs) = layer.crs
            {
                println!("  CRS: {}", crs);
            }
        }
    }

    if let Some(ref tiles) = info.tile_info {
        println!("{}", style("Tile Archive Information").bold().cyan());
        println!("  Tile Count: {}", tiles.tile_count);
        if let (Some(min_zoom), Some(max_zoom)) = (tiles.min_zoom, tiles.max_zoom) {
            println!("  Zoom Range: {} - {}", min_zoom, max_zoom);
        }
        if let Some(ref tile_format) = tiles.tile_format {
            println!("  Tile Format: {}", tile_format);
        }
        if let Some(ref bounds) = tiles.bounds {
            println!("\n{}", style("Bounds").bold().cyan());
            println!("  Min X: {}", bounds.min_x);
            println!("  Min Y: {}", bounds.min_y);
            println!("  Max X: {}", bounds.max_x);
            println!("  Max Y: {}", bounds.max_y);
        }
    }

    if let Some(ref pc) = info.point_cloud_info {
        println!("{}", style("Point Cloud Information").bold().cyan());
        println!("  Points:       {}", pc.point_count);
        println!("  Point Format: {}", pc.point_format);
        println!("\n{}", style("Bounds").bold().cyan());
        println!(
            "  Min: ({}, {}, {})",
            pc.bounds.min_x, pc.bounds.min_y, pc.bounds.min_z
        );
        println!(
            "  Max: ({}, {}, {})",
            pc.bounds.max_x, pc.bounds.max_y, pc.bounds.max_z
        );
        if (args.crs || args.metadata)
            && let Some(ref crs) = pc.crs
        {
            println!("\n{}", style("Coordinate Reference System").bold().cyan());
            println!("  {}", crs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_data_type() {
        assert_eq!(format_data_type(RasterDataType::UInt8), "UInt8");
        assert_eq!(format_data_type(RasterDataType::Float32), "Float32");
        assert_eq!(format_data_type(RasterDataType::CFloat32), "CFloat32");
    }

    #[test]
    fn test_jp2_data_type_label() {
        assert_eq!(jp2_data_type_label(8, false), "8-bit unsigned");
        assert_eq!(jp2_data_type_label(16, true), "16-bit signed");
    }

    /// FlatGeobuf CRS formatting prefers "ORG:CODE" over a raw WKT/debug dump.
    #[test]
    fn test_flatgeobuf_crs_label_org_code() {
        let crs = oxigeo_flatgeobuf::CrsInfo::from_epsg(4326);
        assert_eq!(flatgeobuf_crs_label(&crs), "EPSG:4326");
    }

    #[test]
    fn test_flatgeobuf_crs_label_wkt_fallback() {
        let crs = oxigeo_flatgeobuf::CrsInfo::from_wkt("GEOGCS[\"WGS 84\"]");
        assert_eq!(flatgeobuf_crs_label(&crs), "GEOGCS[\"WGS 84\"]");
    }

    fn args_for(path: PathBuf) -> InfoArgs {
        InfoArgs {
            input: path,
            stats: false,
            compute_minmax: false,
            metadata: true,
            crs: true,
            bands: false,
        }
    }

    #[test]
    fn test_read_flatgeobuf_info_demo_fixture() {
        // `demo/cog-viewer/iron-belt.fgb` is generated by the
        // `create_test_flatgeobuf_samples` example and is `.gitignore`-d, so it
        // may be absent on a clean checkout. Prefer the real demo fixture when
        // present (assertions unchanged); otherwise fall back to an equivalent
        // in-process synthesized `GeometryCollection` fixture so this test is
        // self-contained rather than silently depending on external setup.
        let demo = crate::commands::test_fixtures::demo_fixture("demo/cog-viewer/iron-belt.fgb");
        let path = if demo.exists() {
            demo
        } else {
            crate::commands::test_fixtures::geometrycollection_fgb_fixture_path()
        };
        let info = read_flatgeobuf_info(&args_for(path)).expect("read FlatGeobuf info");
        assert_eq!(info.layer_count, 1);
        assert!(info.feature_count > 0, "expected at least one feature");
    }

    #[test]
    fn test_read_zarr_info_demo_fixture() {
        let path = crate::commands::test_fixtures::demo_fixture("demo/cog-viewer/iron-belt.zarr");
        let info = read_zarr_info(&args_for(path)).expect("read Zarr info");
        assert_eq!(info.shape, vec![512, 512]);
        assert_eq!(info.chunk_shape, vec![64, 64]);
        assert_eq!(info.data_type, "float32");
        assert_eq!(info.zarr_format, 3);
        assert_eq!(info.crs.as_deref(), Some("EPSG:4326"));
    }

    #[test]
    fn test_read_geoparquet_info_synthesized_fixture() {
        let path = crate::commands::test_fixtures::geoparquet_fixture_path();
        let info = read_geoparquet_info(&args_for(path.clone())).expect("read GeoParquet info");
        assert_eq!(info.feature_count, 4);
        assert!(
            info.crs.as_deref().is_some_and(|c| c.contains("4326")),
            "expected CRS to mention EPSG:4326, got {:?}",
            info.crs
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_gpkg_info_synthesized_fixture() {
        let path = crate::commands::test_fixtures::gpkg_fixture_path();
        let info = read_gpkg_info(&args_for(path.to_path_buf())).expect("read GeoPackage info");
        assert_eq!(info.layer_count, 1);
        let layer = &info.layers[0];
        assert_eq!(layer.table_name, "cities");
        assert_eq!(layer.data_type, "features");
        assert_eq!(layer.geometry_type.as_deref(), Some("POINT"));
        assert_eq!(layer.feature_count, Some(3));
        assert_eq!(layer.crs.as_deref(), Some("EPSG:4326"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_mbtiles_info_synthesized_fixture() {
        let path = crate::commands::test_fixtures::mbtiles_fixture_path();
        let info = read_mbtiles_info(&args_for(path.to_path_buf())).expect("read MBTiles info");
        assert_eq!(info.tile_count, 3);
        assert_eq!(info.min_zoom, Some(0));
        assert_eq!(info.max_zoom, Some(1));
        assert!(info.bounds.is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_pmtiles_info_synthesized_fixture() {
        let path = crate::commands::test_fixtures::pmtiles_fixture_path();
        let info = read_pmtiles_info(&args_for(path.clone())).expect("read PMTiles info");
        assert_eq!(info.tile_count, 3);
        assert_eq!(info.min_zoom, Some(0));
        assert_eq!(info.max_zoom, Some(1));
        assert_eq!(info.tile_format.as_deref(), Some("Png"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_jp2_info_synthesized_fixture() {
        let path = crate::commands::test_fixtures::j2k_fixture_path();
        let info = read_jp2_info(&args_for(path.clone())).expect("read JPEG2000 info");
        assert_eq!(info.width, 4);
        assert_eq!(info.height, 4);
        assert_eq!(info.bands, 1);
        assert_eq!(info.data_type, "8-bit unsigned");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_copc_info_synthesized_fixture() {
        let bounds = ([1.0, 2.0, 0.5], [10.0, 20.0, 5.0]);
        let path = crate::commands::test_fixtures::copc_fixture_path(1000, bounds);
        let info = read_copc_info(&args_for(path.clone())).expect("read COPC info");
        assert_eq!(info.point_count, 1000);
        assert_eq!(info.point_format, 6);
        assert!((info.bounds.min_x - 1.0).abs() < f64::EPSILON);
        assert!((info.bounds.max_z - 5.0).abs() < f64::EPSILON);
        let _ = std::fs::remove_file(&path);
    }
}
