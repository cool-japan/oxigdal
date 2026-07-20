//! Metadata extraction from geospatial datasets.
//!
//! This module provides functionality to extract metadata from various
//! geospatial file formats including GeoTIFF, NetCDF, HDF5, and STAC.

use crate::common::{BoundingBox, Keyword, TemporalExtent};
use crate::error::{MetadataError, Result};
use crate::iso19115::{DataIdentification, Iso19115Metadata};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Extracted metadata from a dataset.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedMetadata {
    /// Dataset title
    pub title: Option<String>,
    /// Abstract/description
    pub abstract_text: Option<String>,
    /// Bounding box
    pub bbox: Option<BoundingBox>,
    /// Temporal extent
    pub temporal_extent: Option<TemporalExtent>,
    /// Coordinate reference system
    pub crs: Option<String>,
    /// Spatial resolution
    pub spatial_resolution: Option<f64>,
    /// Format
    pub format: Option<String>,
    /// Keywords
    pub keywords: Vec<String>,
    /// Additional attributes
    pub attributes: std::collections::HashMap<String, String>,
}

/// Extract metadata from a file path.
///
/// # Arguments
///
/// * `path` - Path to the geospatial file
///
/// # Returns
///
/// Extracted metadata or error.
pub fn extract_metadata<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    let path = path.as_ref();

    // Determine file type from extension
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| MetadataError::InvalidFormat("No file extension".to_string()))?;

    match extension.to_lowercase().as_str() {
        "tif" | "tiff" | "gtiff" => extract_from_geotiff(path),
        "nc" | "nc4" | "netcdf" => extract_from_netcdf(path),
        "h5" | "hdf5" | "he5" => extract_from_hdf5(path),
        "json" => extract_from_stac(path),
        _ => Err(MetadataError::Unsupported(format!(
            "File format not supported: {}",
            extension
        ))),
    }
}

/// Extract metadata from GeoTIFF by reading TIFF IFD tags and GeoKeys.
fn extract_from_geotiff<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    use std::io::Read;

    let path = path.as_ref();
    let path_str = path.to_string_lossy().to_string();
    let mut attributes = std::collections::HashMap::new();
    attributes.insert("file_path".to_string(), path_str.clone());

    // Read the file header (up to 64KB to capture IFD + GeoKeys)
    let mut file = std::fs::File::open(path).map_err(|e| {
        MetadataError::ExtractionError(format!("Cannot open '{}': {}", path_str, e))
    })?;
    let mut header = vec![0u8; 65536];
    let bytes_read = file.read(&mut header).map_err(|e| {
        MetadataError::ExtractionError(format!("Cannot read '{}': {}", path_str, e))
    })?;
    header.truncate(bytes_read);

    if header.len() < 8 {
        return Err(MetadataError::InvalidFormat(
            "File too small for TIFF".to_string(),
        ));
    }

    // Detect byte order
    let is_le = header[0] == 0x49 && header[1] == 0x49;
    let is_be = header[0] == 0x4D && header[1] == 0x4D;
    if !is_le && !is_be {
        return Err(MetadataError::InvalidFormat(
            "Not a TIFF file (bad byte order mark)".to_string(),
        ));
    }

    let read_u16 = |buf: &[u8], off: usize| -> u16 {
        if is_le {
            u16::from_le_bytes([buf[off], buf[off + 1]])
        } else {
            u16::from_be_bytes([buf[off], buf[off + 1]])
        }
    };
    let read_u32 = |buf: &[u8], off: usize| -> u32 {
        if is_le {
            u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
        } else {
            u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
        }
    };
    let read_u64 = |buf: &[u8], off: usize| -> u64 {
        if is_le {
            u64::from_le_bytes([
                buf[off],
                buf[off + 1],
                buf[off + 2],
                buf[off + 3],
                buf[off + 4],
                buf[off + 5],
                buf[off + 6],
                buf[off + 7],
            ])
        } else {
            u64::from_be_bytes([
                buf[off],
                buf[off + 1],
                buf[off + 2],
                buf[off + 3],
                buf[off + 4],
                buf[off + 5],
                buf[off + 6],
                buf[off + 7],
            ])
        }
    };

    let version = read_u16(&header, 2);
    let is_bigtiff = version == 43;
    attributes.insert("tiff_version".to_string(), version.to_string());

    if is_bigtiff {
        attributes.insert("bigtiff".to_string(), "true".to_string());
    }

    // Read first IFD offset
    let ifd_offset = if is_bigtiff {
        // BigTIFF: bytes 8-15 contain the 8-byte IFD offset. Read the full
        // 8-byte value rather than truncating to a fixed 4-byte half — on
        // big-endian files the low-order bytes sit at the *end* of the
        // field (bytes 12-15), not the start, so slicing off the first 4
        // bytes silently returns zero for big-endian BigTIFFs.
        if header.len() < 16 {
            return Ok(ExtractedMetadata {
                format: Some("GeoTIFF".to_string()),
                attributes,
                ..Default::default()
            });
        }
        read_u64(&header, 8) as usize
    } else {
        read_u32(&header, 4) as usize
    };

    // BigTIFF's IFD entry-count field is 8 bytes (vs. 2 bytes for classic
    // TIFF), so the minimum header size needed to read it differs.
    let entry_count_field_size = if is_bigtiff { 8 } else { 2 };
    if ifd_offset >= header.len() || ifd_offset + entry_count_field_size > header.len() {
        return Ok(ExtractedMetadata {
            format: Some("GeoTIFF".to_string()),
            attributes,
            ..Default::default()
        });
    }

    let entry_count = if is_bigtiff {
        read_u64(&header, ifd_offset) as usize
    } else {
        read_u16(&header, ifd_offset) as usize
    };
    // BigTIFF entries are 20 bytes (tag2+type2+count8+value8) vs. classic
    // TIFF's 12 bytes (tag2+type2+count4+value4).
    let entry_size = if is_bigtiff { 20 } else { 12 };
    let entries_start = ifd_offset + entry_count_field_size;

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut compression: Option<u16> = None;
    let mut samples_per_pixel: Option<u16> = None;
    let mut model_tiepoint: Vec<f64> = Vec::new();
    let mut model_pixel_scale: Vec<f64> = Vec::new();
    let mut geo_key_directory: Vec<u16> = Vec::new();
    let mut geo_ascii_params: Option<String> = None;

    // TIFF tag IDs
    const TAG_IMAGE_WIDTH: u16 = 256;
    const TAG_IMAGE_LENGTH: u16 = 257;
    const TAG_BITS_PER_SAMPLE: u16 = 258;
    const TAG_COMPRESSION: u16 = 259;
    const TAG_SAMPLES_PER_PIXEL: u16 = 277;
    const TAG_MODEL_TIEPOINT: u16 = 33922;
    const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
    const TAG_GEO_KEY_DIRECTORY: u16 = 34735;
    const TAG_GEO_ASCII_PARAMS: u16 = 34737;
    const TAG_GDAL_METADATA: u16 = 42112;
    const TAG_GDAL_NODATA: u16 = 42113;

    let read_f64 = |buf: &[u8], off: usize| -> f64 {
        if is_le {
            f64::from_le_bytes([
                buf[off],
                buf[off + 1],
                buf[off + 2],
                buf[off + 3],
                buf[off + 4],
                buf[off + 5],
                buf[off + 6],
                buf[off + 7],
            ])
        } else {
            f64::from_be_bytes([
                buf[off],
                buf[off + 1],
                buf[off + 2],
                buf[off + 3],
                buf[off + 4],
                buf[off + 5],
                buf[off + 6],
                buf[off + 7],
            ])
        }
    };

    // Out-of-line data offsets are stored as a 4-byte LONG in classic TIFF
    // but an 8-byte LONG8 in BigTIFF; read the width matching the format.
    let read_data_offset = |buf: &[u8], off: usize| -> usize {
        if is_bigtiff {
            read_u64(buf, off) as usize
        } else {
            read_u32(buf, off) as usize
        }
    };
    // Number of bytes available for an inline (non-offset) value within the
    // entry's value/offset field: 4 bytes for classic TIFF, 8 for BigTIFF.
    let inline_value_bytes: usize = if is_bigtiff { 8 } else { 4 };

    for i in 0..entry_count {
        let entry_off = entries_start + i * entry_size;
        if entry_off + entry_size > header.len() {
            break;
        }

        let tag = read_u16(&header, entry_off);
        let type_id = read_u16(&header, entry_off + 2);
        // BigTIFF entry layout: tag u16 @0, type u16 @2, count u64 @4,
        // value/offset u64 @12. Classic TIFF: tag u16 @0, type u16 @2,
        // count u32 @4, value/offset u32 @8.
        let (count, value_offset_pos) = if is_bigtiff {
            (read_u64(&header, entry_off + 4) as usize, entry_off + 12)
        } else {
            (read_u32(&header, entry_off + 4) as usize, entry_off + 8)
        };

        // For SHORT/LONG values that fit in the value/offset field, the
        // value is stored inline; otherwise it is a byte offset elsewhere
        // in the file.
        match tag {
            TAG_IMAGE_WIDTH => {
                width = Some(if type_id == 3 {
                    // SHORT
                    u32::from(read_u16(&header, value_offset_pos))
                } else {
                    // LONG
                    read_u32(&header, value_offset_pos)
                });
            }
            TAG_IMAGE_LENGTH => {
                height = Some(if type_id == 3 {
                    u32::from(read_u16(&header, value_offset_pos))
                } else {
                    read_u32(&header, value_offset_pos)
                });
            }
            TAG_BITS_PER_SAMPLE => {
                bits_per_sample = Some(read_u16(&header, value_offset_pos));
            }
            TAG_COMPRESSION => {
                compression = Some(read_u16(&header, value_offset_pos));
            }
            TAG_SAMPLES_PER_PIXEL => {
                samples_per_pixel = Some(read_u16(&header, value_offset_pos));
            }
            TAG_MODEL_PIXEL_SCALE => {
                // DOUBLE values (type 12), count typically 3
                let data_off = read_data_offset(&header, value_offset_pos);
                if data_off + count * 8 <= header.len() {
                    for j in 0..count {
                        model_pixel_scale.push(read_f64(&header, data_off + j * 8));
                    }
                }
            }
            TAG_MODEL_TIEPOINT => {
                // DOUBLE values (type 12), count typically 6
                let data_off = read_data_offset(&header, value_offset_pos);
                if data_off + count * 8 <= header.len() {
                    for j in 0..count {
                        model_tiepoint.push(read_f64(&header, data_off + j * 8));
                    }
                }
            }
            TAG_GEO_KEY_DIRECTORY => {
                // SHORT values (type 3)
                if count * 2 <= inline_value_bytes {
                    // Inline
                    for j in 0..count {
                        geo_key_directory.push(read_u16(&header, value_offset_pos + j * 2));
                    }
                } else {
                    let data_off = read_data_offset(&header, value_offset_pos);
                    if data_off + count * 2 <= header.len() {
                        for j in 0..count {
                            geo_key_directory.push(read_u16(&header, data_off + j * 2));
                        }
                    }
                }
            }
            TAG_GEO_ASCII_PARAMS => {
                let data_off = read_data_offset(&header, value_offset_pos);
                if data_off + count <= header.len()
                    && let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count])
                {
                    geo_ascii_params = Some(s.trim_end_matches('\0').to_string());
                }
            }
            TAG_GDAL_METADATA => {
                let data_off = read_data_offset(&header, value_offset_pos);
                if data_off + count <= header.len()
                    && let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count])
                {
                    attributes.insert(
                        "gdal_metadata".to_string(),
                        s.trim_end_matches('\0').to_string(),
                    );
                }
            }
            TAG_GDAL_NODATA => {
                let data_off = read_data_offset(&header, value_offset_pos);
                if data_off + count <= header.len()
                    && let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count])
                {
                    attributes.insert("nodata".to_string(), s.trim_end_matches('\0').to_string());
                }
            }
            _ => {}
        }
    }

    // Populate attributes from parsed tags
    if let Some(w) = width {
        attributes.insert("width".to_string(), w.to_string());
    }
    if let Some(h) = height {
        attributes.insert("height".to_string(), h.to_string());
    }
    if let Some(bps) = bits_per_sample {
        attributes.insert("bits_per_sample".to_string(), bps.to_string());
    }
    if let Some(c) = compression {
        let comp_name = match c {
            1 => "None",
            5 => "LZW",
            6 => "OJPEG",
            7 => "JPEG",
            8 | 32946 => "Deflate",
            32773 => "PackBits",
            34887 => "LERC",
            50000 => "ZSTD",
            50001 => "WebP",
            _ => "Unknown",
        };
        attributes.insert("compression".to_string(), comp_name.to_string());
    }
    if let Some(spp) = samples_per_pixel {
        attributes.insert("samples_per_pixel".to_string(), spp.to_string());
    }

    // Compute bounding box from tiepoint + pixel scale
    let bbox = if model_tiepoint.len() >= 6 && model_pixel_scale.len() >= 2 {
        let origin_x = model_tiepoint[3];
        let origin_y = model_tiepoint[4];
        let pixel_x = model_pixel_scale[0];
        let pixel_y = model_pixel_scale[1];

        if let (Some(w), Some(h)) = (width, height) {
            let min_x = origin_x;
            let max_x = origin_x + pixel_x * f64::from(w);
            let max_y = origin_y;
            let min_y = origin_y - pixel_y * f64::from(h);
            // BoundingBox::new(west, east, south, north)
            Some(BoundingBox::new(min_x, max_x, min_y, max_y))
        } else {
            None
        }
    } else {
        None
    };

    // Compute spatial resolution
    let spatial_resolution = if model_pixel_scale.len() >= 2 {
        Some(model_pixel_scale[0])
    } else {
        None
    };

    // Parse CRS from GeoKeys
    let crs = parse_crs_from_geokeys(&geo_key_directory, &geo_ascii_params);

    // Store model tiepoint/scale as attributes for downstream use
    if !model_tiepoint.is_empty() {
        let tp: Vec<String> = model_tiepoint.iter().map(|v| format!("{v}")).collect();
        attributes.insert("model_tiepoint".to_string(), tp.join(","));
    }
    if !model_pixel_scale.is_empty() {
        let ps: Vec<String> = model_pixel_scale.iter().map(|v| format!("{v}")).collect();
        attributes.insert("model_pixel_scale".to_string(), ps.join(","));
    }

    Ok(ExtractedMetadata {
        title: Some(
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string(),
        ),
        format: Some("GeoTIFF".to_string()),
        bbox,
        spatial_resolution,
        crs,
        attributes,
        ..Default::default()
    })
}

/// Parse CRS information from GeoKey directory.
///
/// GeoKey directory layout: [KeyDirectoryVersion, KeyRevision, MinorRevision, NumberOfKeys,
///   KeyID_1, TIFFTagLocation_1, Count_1, ValueOffset_1, ...]
fn parse_crs_from_geokeys(
    geo_key_directory: &[u16],
    geo_ascii_params: &Option<String>,
) -> Option<String> {
    if geo_key_directory.len() < 4 {
        return None;
    }

    let num_keys = geo_key_directory[3] as usize;

    // GeoKey IDs
    const GT_MODEL_TYPE: u16 = 1024;
    const GT_RASTER_TYPE: u16 = 1025;
    const GEOGRAPHIC_TYPE: u16 = 2048;
    const PROJECTED_CS_TYPE: u16 = 3072;
    const PROJ_CITATION: u16 = 3073;

    let mut model_type: Option<u16> = None;
    let mut geographic_type: Option<u16> = None;
    let mut projected_type: Option<u16> = None;
    let mut _raster_type: Option<u16> = None;
    let mut proj_citation: Option<String> = None;

    for k in 0..num_keys {
        let base = 4 + k * 4;
        if base + 3 >= geo_key_directory.len() {
            break;
        }
        let key_id = geo_key_directory[base];
        let tiff_tag_location = geo_key_directory[base + 1];
        let count = geo_key_directory[base + 2] as usize;
        let value_offset = geo_key_directory[base + 3];

        match key_id {
            GT_MODEL_TYPE if tiff_tag_location == 0 => {
                model_type = Some(value_offset);
            }
            GT_RASTER_TYPE if tiff_tag_location == 0 => {
                _raster_type = Some(value_offset);
            }
            GEOGRAPHIC_TYPE if tiff_tag_location == 0 => {
                geographic_type = Some(value_offset);
            }
            PROJECTED_CS_TYPE if tiff_tag_location == 0 => {
                projected_type = Some(value_offset);
            }
            PROJ_CITATION if tiff_tag_location == 34737 => {
                // Citation is stored in GeoAsciiParams
                if let Some(ascii) = geo_ascii_params {
                    let offset = value_offset as usize;
                    if offset + count <= ascii.len() {
                        proj_citation = Some(
                            ascii[offset..offset + count]
                                .trim_end_matches('|')
                                .to_string(),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // Build CRS string
    if let Some(epsg) = projected_type
        && epsg != 0
        && epsg != 32767
    {
        return Some(format!("EPSG:{epsg}"));
    }
    if let Some(epsg) = geographic_type
        && epsg != 0
        && epsg != 32767
    {
        return Some(format!("EPSG:{epsg}"));
    }
    if let Some(citation) = proj_citation {
        return Some(citation);
    }
    if let Some(mt) = model_type {
        return Some(
            match mt {
                1 => "Projected CRS (user-defined)",
                2 => "Geographic CRS (user-defined)",
                3 => "Geocentric CRS",
                _ => "Unknown CRS",
            }
            .to_string(),
        );
    }

    None
}

/// Extract metadata from NetCDF.
fn extract_from_netcdf<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    #[cfg(feature = "netcdf")]
    {
        crate::extractors::netcdf_cf::NetCdfCfExtractor::extract(path)
    }
    #[cfg(not(feature = "netcdf"))]
    {
        let path_str = path.as_ref().to_string_lossy().to_string();
        Err(MetadataError::Unsupported(format!(
            "NetCDF extraction not available for '{}': enable the 'netcdf' feature",
            path_str
        )))
    }
}

/// Extract metadata from HDF5.
fn extract_from_hdf5<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    #[cfg(feature = "hdf5")]
    {
        extract_from_hdf5_impl(path)
    }
    #[cfg(not(feature = "hdf5"))]
    {
        let path_str = path.as_ref().to_string_lossy().to_string();
        Err(MetadataError::Unsupported(format!(
            "HDF5 extraction not available for '{}': enable the 'hdf5' feature",
            path_str
        )))
    }
}

/// Convert an `oxigdal_hdf5::Hdf5Error` into a `MetadataError`.
#[cfg(feature = "hdf5")]
fn hdf5_err(e: oxigdal_hdf5::Hdf5Error) -> MetadataError {
    MetadataError::ExtractionError(e.to_string())
}

/// Stringify an `AttributeValue` for storage in the attributes map.
#[cfg(feature = "hdf5")]
fn attribute_value_to_string(val: &oxigdal_hdf5::AttributeValue) -> String {
    use oxigdal_hdf5::AttributeValue;
    match val {
        AttributeValue::String(s) => s.clone(),
        AttributeValue::Int8(v) => v.to_string(),
        AttributeValue::UInt8(v) => v.to_string(),
        AttributeValue::Int16(v) => v.to_string(),
        AttributeValue::UInt16(v) => v.to_string(),
        AttributeValue::Int32(v) => v.to_string(),
        AttributeValue::UInt32(v) => v.to_string(),
        AttributeValue::Int64(v) => v.to_string(),
        AttributeValue::UInt64(v) => v.to_string(),
        AttributeValue::Float32(v) => v.to_string(),
        AttributeValue::Float64(v) => v.to_string(),
        AttributeValue::Int8Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::UInt8Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Int16Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::UInt16Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Int32Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::UInt32Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Int64Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::UInt64Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Float32Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Float64Array(arr) => arr
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::StringArray(arr) => arr.join(","),
    }
}

/// Collect all attributes from a group's `Attributes` into the output map,
/// using the given prefix for the key (e.g. `"hdf5_attr_"` for root attributes).
#[cfg(feature = "hdf5")]
fn collect_group_attributes(
    attrs: &oxigdal_hdf5::Attributes,
    prefix: &str,
    out: &mut std::collections::HashMap<String, String>,
) {
    for attr in attrs.iter() {
        let key = format!("{}{}", prefix, attr.name());
        let val = attribute_value_to_string(attr.value());
        out.insert(key, val);
    }
}

/// Walk every group known to the reader (up to depth 3 below root) and collect:
/// - group names into `hdf5_groups`
/// - group attributes with prefix `hdf5_grp_<group_path>_attr_<name>`
#[cfg(feature = "hdf5")]
fn walk_groups(
    reader: &oxigdal_hdf5::Hdf5Reader,
    out: &mut std::collections::HashMap<String, String>,
) {
    let mut group_names: Vec<String> = Vec::new();
    for path in reader.list_groups() {
        // Skip root itself from the listing
        if path == "/" {
            continue;
        }
        // Depth = number of '/' minus 1 (root slash doesn't count)
        let depth = path.chars().filter(|&c| c == '/').count();
        if depth > 3 {
            continue;
        }
        group_names.push(path.to_string());
        if let Ok(grp) = reader.group(path) {
            let prefix = format!(
                "hdf5_grp_{}_attr_",
                path.trim_start_matches('/').replace('/', "_")
            );
            collect_group_attributes(grp.attributes(), &prefix, out);
        }
    }
    group_names.sort();
    if !group_names.is_empty() {
        out.insert("hdf5_groups".to_string(), group_names.join(";"));
    }
}

/// Walk every dataset known to the reader and record name, shape, and datatype.
#[cfg(feature = "hdf5")]
fn walk_datasets(
    reader: &oxigdal_hdf5::Hdf5Reader,
    out: &mut std::collections::HashMap<String, String>,
) {
    let mut dataset_names: Vec<String> = Vec::new();
    for path in reader.list_datasets() {
        dataset_names.push(path.to_string());
        if let Ok(ds) = reader.dataset(path) {
            let safe_key = path.trim_start_matches('/').replace('/', "_");
            let shape_str: Vec<String> = ds.dims().iter().map(|d| d.to_string()).collect();
            out.insert(format!("hdf5_ds_{}_shape", safe_key), shape_str.join("x"));
            out.insert(
                format!("hdf5_ds_{}_dtype", safe_key),
                ds.datatype().name().to_string(),
            );
        }
    }
    dataset_names.sort();
    if !dataset_names.is_empty() {
        out.insert("hdf5_datasets".to_string(), dataset_names.join(";"));
    }
}

/// Map well-known CF/NetCDF-style or geospatial HDF5 attributes from the flat
/// `attributes` map onto the structured fields of `ExtractedMetadata`.
///
/// Keys already contain the `hdf5_attr_` prefix at this stage.
#[cfg(feature = "hdf5")]
#[allow(clippy::type_complexity)]
fn map_well_known_attributes(
    attributes: &std::collections::HashMap<String, String>,
) -> (
    Option<String>, // title
    Option<String>, // abstract_text
    Option<String>, // crs
    Option<BoundingBox>,
    Vec<String>, // keywords
) {
    let lookup = |name: &str| attributes.get(&format!("hdf5_attr_{}", name)).cloned();

    // Title
    let title = lookup("title")
        .or_else(|| lookup("long_name"))
        .or_else(|| lookup("Name"));

    // Abstract / description
    let abstract_text = lookup("comment")
        .or_else(|| lookup("description"))
        .or_else(|| lookup("summary"))
        .or_else(|| lookup("abstract"));

    // CRS
    let crs = lookup("crs_wkt")
        .or_else(|| lookup("projection"))
        .or_else(|| lookup("CoordinateProjection"))
        .or_else(|| {
            // CF convention: EPSG code in attribute "grid_mapping_name" is less
            // common, but "EPSG" is sometimes stored directly.
            lookup("EPSG").map(|v| format!("EPSG:{v}"))
        });

    // Bounding box: various common conventions
    let bbox = {
        let try_f64 = |s: &str| -> Option<f64> { s.parse().ok() };
        // CF convention attributes
        let west = lookup("westernmost_longitude")
            .or_else(|| lookup("geospatial_lon_min"))
            .or_else(|| lookup("WEST_LONGITUDE"))
            .and_then(|v| try_f64(&v));
        let east = lookup("easternmost_longitude")
            .or_else(|| lookup("geospatial_lon_max"))
            .or_else(|| lookup("EAST_LONGITUDE"))
            .and_then(|v| try_f64(&v));
        let south = lookup("southernmost_latitude")
            .or_else(|| lookup("geospatial_lat_min"))
            .or_else(|| lookup("SOUTH_LATITUDE"))
            .and_then(|v| try_f64(&v));
        let north = lookup("northernmost_latitude")
            .or_else(|| lookup("geospatial_lat_max"))
            .or_else(|| lookup("NORTH_LATITUDE"))
            .and_then(|v| try_f64(&v));
        match (west, east, south, north) {
            (Some(w), Some(e), Some(s), Some(n)) if w <= e && s <= n => {
                Some(BoundingBox::new(w, e, s, n))
            }
            _ => None,
        }
    };

    // Keywords: CF "keywords" attribute, comma-separated
    let keywords = lookup("keywords")
        .map(|v| {
            v.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();

    (title, abstract_text, crs, bbox, keywords)
}

/// Real HDF5 metadata extraction implementation (requires `hdf5` feature).
#[cfg(feature = "hdf5")]
fn extract_from_hdf5_impl<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    let path_ref = path.as_ref();
    let path_str = path_ref.to_string_lossy().to_string();

    // Open the HDF5 file — error maps to ExtractionError
    let reader = oxigdal_hdf5::Hdf5Reader::open(path_ref).map_err(|e| {
        MetadataError::ExtractionError(format!("Cannot open HDF5 file '{}': {}", path_str, e))
    })?;

    let mut attributes: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    attributes.insert("file_path".to_string(), path_str.clone());

    // Record superblock version
    let superblock_ver = format!("{:?}", reader.superblock_version());
    attributes.insert("hdf5_superblock_version".to_string(), superblock_ver);

    // Collect root group attributes with prefix `hdf5_attr_`
    let root = reader.root().map_err(hdf5_err)?;
    collect_group_attributes(root.attributes(), "hdf5_attr_", &mut attributes);

    // Walk sub-groups (depth ≤ 3) and collect their attributes
    walk_groups(&reader, &mut attributes);

    // Walk datasets and record shape + dtype
    walk_datasets(&reader, &mut attributes);

    // Check for well-known geospatial metadata groups and promote their
    // attributes to the root-level `hdf5_attr_` namespace if not already present.
    for meta_group in &["/METADATA", "/metadata", "/HDF_METADATA"] {
        if reader.is_group(meta_group)
            && let Ok(grp) = reader.group(meta_group)
        {
            for attr in grp.attributes().iter() {
                let key = format!("hdf5_attr_{}", attr.name());
                attributes
                    .entry(key)
                    .or_insert_with(|| attribute_value_to_string(attr.value()));
            }
        }
    }

    // Set file title from filename if not overridden by attributes
    let file_title = path_ref
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());

    // Map well-known attributes onto structured fields
    let (attr_title, abstract_text, crs, bbox, keywords) = map_well_known_attributes(&attributes);

    let title = attr_title.or(file_title);

    Ok(ExtractedMetadata {
        title,
        abstract_text,
        bbox,
        crs,
        format: Some("HDF5".to_string()),
        keywords,
        attributes,
        ..Default::default()
    })
}

/// Extract metadata from STAC.
#[cfg(feature = "stac")]
fn extract_from_stac<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy().to_string();

    let content = std::fs::read_to_string(path).map_err(|e| {
        MetadataError::ExtractionError(format!("Cannot read '{}': {}", path_str, e))
    })?;

    extract_from_stac_json(&content)
}

/// Extract metadata from STAC JSON not available without stac feature.
#[cfg(not(feature = "stac"))]
fn extract_from_stac<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    Err(MetadataError::Unsupported(format!(
        "STAC extraction not available for '{}': enable the 'stac' feature",
        path_str
    )))
}

/// Extract metadata from a STAC Item JSON string.
///
/// Parses the JSON as a STAC Item and extracts bbox, datetime, CRS (from
/// projection extension), title, description, keywords, and additional
/// attributes.
#[cfg(feature = "stac")]
pub fn extract_from_stac_json(json: &str) -> Result<ExtractedMetadata> {
    let item: oxigdal_stac::Item =
        serde_json::from_str(json).map_err(|e| MetadataError::JsonError(e.to_string()))?;

    let mut attributes = std::collections::HashMap::new();
    attributes.insert("stac_version".to_string(), item.stac_version.clone());
    attributes.insert("id".to_string(), item.id.clone());

    if let Some(ref collection) = item.collection {
        attributes.insert("collection".to_string(), collection.clone());
    }

    if let Some(ref extensions) = item.stac_extensions
        && !extensions.is_empty()
    {
        attributes.insert("stac_extensions".to_string(), extensions.join(", "));
    }

    // Extract bbox
    let bbox = item.bbox.as_ref().and_then(|b| {
        if b.len() >= 4 {
            Some(BoundingBox::new(b[0], b[2], b[1], b[3]))
        } else {
            None
        }
    });

    // Extract temporal extent from properties
    let temporal_extent = {
        let start = item.properties.start_datetime.or(item.properties.datetime);
        let end = item.properties.end_datetime.or(item.properties.datetime);
        if start.is_some() || end.is_some() {
            Some(TemporalExtent { start, end })
        } else {
            None
        }
    };

    // Store datetime as attribute
    if let Some(dt) = item.properties.datetime {
        attributes.insert("datetime".to_string(), dt.to_rfc3339());
    }

    // Extract CRS from projection extension fields in additional_fields
    let crs = extract_crs_from_properties(&item.properties.additional_fields);

    // Extract title and description
    let title = item.properties.title.clone();
    let abstract_text = item.properties.description.clone();

    // Collect keywords from additional_fields if present
    let keywords = item
        .properties
        .additional_fields
        .get("keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Extract spatial resolution from projection extension (gsd)
    let spatial_resolution = item
        .properties
        .additional_fields
        .get("gsd")
        .and_then(|v| v.as_f64());

    // Populate additional attributes from well-known properties
    for (key, value) in &item.properties.additional_fields {
        // Skip complex objects; store scalars as string attributes
        match value {
            serde_json::Value::String(s) => {
                attributes.insert(key.clone(), s.clone());
            }
            serde_json::Value::Number(n) => {
                attributes.insert(key.clone(), n.to_string());
            }
            serde_json::Value::Bool(b) => {
                attributes.insert(key.clone(), b.to_string());
            }
            _ => {}
        }
    }

    // Record asset keys
    if !item.assets.is_empty() {
        let asset_keys: Vec<String> = item.assets.keys().cloned().collect();
        attributes.insert("asset_keys".to_string(), asset_keys.join(", "));
    }

    Ok(ExtractedMetadata {
        title,
        abstract_text,
        bbox,
        temporal_extent,
        crs,
        spatial_resolution,
        format: Some("STAC".to_string()),
        keywords,
        attributes,
    })
}

/// Extract CRS information from STAC projection extension properties.
#[cfg(feature = "stac")]
fn extract_crs_from_properties(
    fields: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<String> {
    // proj:epsg is the most common
    if let Some(epsg) = fields.get("proj:epsg").and_then(|v| v.as_i64()) {
        return Some(format!("EPSG:{}", epsg));
    }
    // proj:code (newer projection extension)
    if let Some(code) = fields.get("proj:code").and_then(|v| v.as_str()) {
        return Some(code.to_string());
    }
    // proj:wkt2
    if let Some(wkt2) = fields.get("proj:wkt2").and_then(|v| v.as_str()) {
        // Return a truncated indicator rather than the full WKT2. Truncate by
        // character count (not byte offset) so a multi-byte UTF-8 character
        // straddling the cut point can never split-panic.
        let truncated: String = wkt2.chars().take(64).collect();
        return Some(format!("WKT2:{truncated}"));
    }
    None
}

/// Auto-populate ISO 19115 metadata from extracted metadata.
///
/// # Arguments
///
/// * `extracted` - Extracted metadata
///
/// # Returns
///
/// ISO 19115 metadata populated with extracted information.
pub fn to_iso19115(extracted: &ExtractedMetadata) -> Result<Iso19115Metadata> {
    let mut iso = Iso19115Metadata::default();

    // Create data identification
    let mut ident = DataIdentification::default();

    // Set title
    if let Some(ref title) = extracted.title {
        ident.citation.title = title.clone();
    } else {
        ident.citation.title = "Untitled Dataset".to_string();
    }

    // Set abstract
    if let Some(ref abstract_text) = extracted.abstract_text {
        ident.abstract_text = abstract_text.clone();
    }

    // Set extent
    if let Some(bbox) = extracted.bbox {
        ident.extent.geographic_extent = Some(bbox);
    }

    if let Some(ref temporal) = extracted.temporal_extent {
        ident.extent.temporal_extent = Some(temporal.clone());
    }

    // Set keywords
    if !extracted.keywords.is_empty() {
        ident.keywords.push(
            extracted
                .keywords
                .iter()
                .map(|k| Keyword {
                    keyword: k.clone(),
                    thesaurus: None,
                })
                .collect(),
        );
    }

    iso.identification_info.push(ident);

    // Set reference system
    if let Some(ref crs) = extracted.crs {
        use crate::iso19115::reference_system::{Identifier, ReferenceSystem};
        iso.reference_system_info.push(ReferenceSystem {
            reference_system_identifier: Some(Identifier::new(crs)),
            reference_system_type: None,
        });
    }

    Ok(iso)
}

/// Auto-populate FGDC metadata from extracted metadata.
///
/// # Arguments
///
/// * `extracted` - Extracted metadata
///
/// # Returns
///
/// FGDC metadata populated with extracted information.
pub fn to_fgdc(extracted: &ExtractedMetadata) -> Result<crate::fgdc::FgdcMetadata> {
    use crate::fgdc::*;

    let mut fgdc = FgdcMetadata::default();

    // Set title
    if let Some(ref title) = extracted.title {
        fgdc.idinfo.citation.citeinfo.title = title.clone();
    } else {
        fgdc.idinfo.citation.citeinfo.title = "Untitled Dataset".to_string();
    }

    // Set abstract
    if let Some(ref abstract_text) = extracted.abstract_text {
        fgdc.idinfo.descript.abstract_text = abstract_text.clone();
    }

    // Set bounding box
    if let Some(bbox) = extracted.bbox {
        fgdc.idinfo.spdom.bounding = bbox;
    }

    // Set keywords
    if !extracted.keywords.is_empty() {
        fgdc.idinfo.keywords.push(Keywords {
            theme: Some("General".to_string()),
            theme_key: extracted.keywords.clone(),
            place: Vec::new(),
            temporal: Vec::new(),
        });
    }

    Ok(fgdc)
}

/// Metadata extractor with configurable options.
pub struct MetadataExtractor {
    /// Whether to extract spatial metadata
    pub extract_spatial: bool,
    /// Whether to extract temporal metadata
    pub extract_temporal: bool,
    /// Whether to extract attributes
    pub extract_attributes: bool,
    /// Maximum number of keywords to extract
    pub max_keywords: usize,
}

impl Default for MetadataExtractor {
    fn default() -> Self {
        Self {
            extract_spatial: true,
            extract_temporal: true,
            extract_attributes: true,
            max_keywords: 20,
        }
    }
}

impl MetadataExtractor {
    /// Create a new extractor with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to extract spatial metadata.
    pub fn with_spatial(mut self, extract: bool) -> Self {
        self.extract_spatial = extract;
        self
    }

    /// Set whether to extract temporal metadata.
    pub fn with_temporal(mut self, extract: bool) -> Self {
        self.extract_temporal = extract;
        self
    }

    /// Set whether to extract attributes.
    pub fn with_attributes(mut self, extract: bool) -> Self {
        self.extract_attributes = extract;
        self
    }

    /// Set maximum number of keywords.
    pub fn with_max_keywords(mut self, max: usize) -> Self {
        self.max_keywords = max;
        self
    }

    /// Extract metadata from file.
    pub fn extract<P: AsRef<Path>>(&self, path: P) -> Result<ExtractedMetadata> {
        let mut metadata = extract_metadata(path)?;

        // Apply extractor options
        if !self.extract_spatial {
            metadata.bbox = None;
            metadata.crs = None;
            metadata.spatial_resolution = None;
        }

        if !self.extract_temporal {
            metadata.temporal_extent = None;
        }

        if !self.extract_attributes {
            metadata.attributes.clear();
        }

        // Limit keywords
        if metadata.keywords.len() > self.max_keywords {
            metadata.keywords.truncate(self.max_keywords);
        }

        Ok(metadata)
    }
}

/// Extract metadata from multiple files in batch.
///
/// # Arguments
///
/// * `paths` - Iterator of file paths
///
/// # Returns
///
/// Vector of extracted metadata results.
pub fn batch_extract<I, P>(paths: I) -> Vec<Result<ExtractedMetadata>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    paths.into_iter().map(extract_metadata).collect()
}

// Tests live in `extract_tests.rs` (rather than an inline `mod tests { ... }`)
// to keep this file under the workspace's 2000-line-per-file limit.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "extract_tests.rs"]
mod tests;
