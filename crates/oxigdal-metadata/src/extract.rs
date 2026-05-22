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

    let version = read_u16(&header, 2);
    let is_bigtiff = version == 43;
    attributes.insert("tiff_version".to_string(), version.to_string());

    if is_bigtiff {
        attributes.insert("bigtiff".to_string(), "true".to_string());
    }

    // Read first IFD offset
    let ifd_offset = if is_bigtiff {
        // BigTIFF: bytes 8-15 contain the 8-byte IFD offset
        if header.len() < 16 {
            return Ok(ExtractedMetadata {
                format: Some("GeoTIFF".to_string()),
                attributes,
                ..Default::default()
            });
        }
        // Read 8-byte offset (take lower 32 bits for our 64KB buffer)
        read_u32(&header, 8) as usize
    } else {
        read_u32(&header, 4) as usize
    };

    if ifd_offset >= header.len() || ifd_offset + 2 > header.len() {
        return Ok(ExtractedMetadata {
            format: Some("GeoTIFF".to_string()),
            attributes,
            ..Default::default()
        });
    }

    let entry_count = read_u16(&header, ifd_offset) as usize;
    let entry_size = if is_bigtiff { 20 } else { 12 };
    let entries_start = ifd_offset + 2;

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

    for i in 0..entry_count {
        let entry_off = entries_start + i * entry_size;
        if entry_off + entry_size > header.len() {
            break;
        }

        let tag = read_u16(&header, entry_off);
        let type_id = read_u16(&header, entry_off + 2);
        let count = read_u32(&header, entry_off + 4) as usize;
        let value_offset_pos = entry_off + 8;

        // For SHORT/LONG values that fit in 4 bytes, value is inline
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
                let data_off = read_u32(&header, value_offset_pos) as usize;
                if data_off + count * 8 <= header.len() {
                    for j in 0..count {
                        model_pixel_scale.push(read_f64(&header, data_off + j * 8));
                    }
                }
            }
            TAG_MODEL_TIEPOINT => {
                // DOUBLE values (type 12), count typically 6
                let data_off = read_u32(&header, value_offset_pos) as usize;
                if data_off + count * 8 <= header.len() {
                    for j in 0..count {
                        model_tiepoint.push(read_f64(&header, data_off + j * 8));
                    }
                }
            }
            TAG_GEO_KEY_DIRECTORY => {
                // SHORT values (type 3)
                if count * 2 <= 4 {
                    // Inline
                    for j in 0..count {
                        geo_key_directory.push(read_u16(&header, value_offset_pos + j * 2));
                    }
                } else {
                    let data_off = read_u32(&header, value_offset_pos) as usize;
                    if data_off + count * 2 <= header.len() {
                        for j in 0..count {
                            geo_key_directory.push(read_u16(&header, data_off + j * 2));
                        }
                    }
                }
            }
            TAG_GEO_ASCII_PARAMS => {
                let data_off = read_u32(&header, value_offset_pos) as usize;
                if data_off + count <= header.len() {
                    if let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count]) {
                        geo_ascii_params = Some(s.trim_end_matches('\0').to_string());
                    }
                }
            }
            TAG_GDAL_METADATA => {
                let data_off = read_u32(&header, value_offset_pos) as usize;
                if data_off + count <= header.len() {
                    if let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count]) {
                        attributes.insert(
                            "gdal_metadata".to_string(),
                            s.trim_end_matches('\0').to_string(),
                        );
                    }
                }
            }
            TAG_GDAL_NODATA => {
                let data_off = read_u32(&header, value_offset_pos) as usize;
                if data_off + count <= header.len() {
                    if let Ok(s) = std::str::from_utf8(&header[data_off..data_off + count]) {
                        attributes
                            .insert("nodata".to_string(), s.trim_end_matches('\0').to_string());
                    }
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
    if let Some(epsg) = projected_type {
        if epsg != 0 && epsg != 32767 {
            return Some(format!("EPSG:{epsg}"));
        }
    }
    if let Some(epsg) = geographic_type {
        if epsg != 0 && epsg != 32767 {
            return Some(format!("EPSG:{epsg}"));
        }
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
        let mut attributes = std::collections::HashMap::new();
        attributes.insert("file_path".to_string(), path_str);
        Ok(ExtractedMetadata {
            format: Some("NetCDF".to_string()),
            attributes,
            ..Default::default()
        })
    }
}

/// Extract metadata from HDF5.
fn extract_from_hdf5<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    let mut attributes = std::collections::HashMap::new();
    attributes.insert("file_path".to_string(), path_str);

    let metadata = ExtractedMetadata {
        format: Some("HDF5".to_string()),
        attributes,
        ..Default::default()
    };

    // In a real implementation, we would use the oxigdal-hdf5 crate

    // Placeholder for HDF5-specific extraction
    // This would include:
    // - Reading file attributes
    // - Extracting dataset metadata
    // - Reading group structure
    // - Extracting embedded metadata standards

    Ok(metadata)
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
    let mut attributes = std::collections::HashMap::new();
    attributes.insert("file_path".to_string(), path_str);

    Ok(ExtractedMetadata {
        format: Some("STAC".to_string()),
        attributes,
        ..Default::default()
    })
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

    if let Some(ref extensions) = item.stac_extensions {
        if !extensions.is_empty() {
            attributes.insert("stac_extensions".to_string(), extensions.join(", "));
        }
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
        // Return a truncated indicator rather than the full WKT2
        return Some(format!("WKT2:{}", &wkt2[..wkt2.len().min(64)]));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extractor_builder() {
        let extractor = MetadataExtractor::new()
            .with_spatial(true)
            .with_temporal(false)
            .with_max_keywords(10);

        assert!(extractor.extract_spatial);
        assert!(!extractor.extract_temporal);
        assert_eq!(extractor.max_keywords, 10);
    }

    #[test]
    fn test_extracted_metadata_default() {
        let metadata = ExtractedMetadata::default();
        assert!(metadata.title.is_none());
        assert!(metadata.bbox.is_none());
        assert!(metadata.keywords.is_empty());
    }

    #[test]
    fn test_unsupported_extension() {
        let path = std::env::temp_dir().join("oxigdal_nonexistent_test_bx9f.xyz");
        let result = extract_metadata(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_extension() {
        let path = std::env::temp_dir().join("oxigdal_nonexistent_somefile_bx9f");
        let result = extract_metadata(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_geotiff_extraction_with_real_tiff() {
        // Create a minimal valid TIFF file with GeoTIFF tags
        let tiff_bytes = build_test_geotiff(
            256,
            256,
            8,
            1,
            Some(&[0.0, 0.0, 0.0, -120.0, 40.0, 0.0]), // tiepoint
            Some(&[0.01, 0.01, 0.0]),                  // pixel scale
            Some(&[
                1, 1, 0,
                3, // GeoKey directory header: version=1, revision=1, minor=0, numKeys=3
                1024, 0, 1, 1, // GTModelTypeGeoKey = ModelTypeProjected
                1025, 0, 1, 1, // GTRasterTypeGeoKey = RasterPixelIsArea
                2048, 0, 1, 4326, // GeographicTypeGeoKey = EPSG:4326
            ]),
        );

        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("test.tif");
        std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

        let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
        assert_eq!(metadata.format.as_deref(), Some("GeoTIFF"));
        assert!(metadata.title.is_some());

        // Check CRS from GeoKeys
        assert_eq!(metadata.crs.as_deref(), Some("EPSG:4326"));

        // Check bounding box computed from tiepoint + pixel scale + dimensions
        let bbox = metadata.bbox.expect("should have bbox");
        assert!((bbox.west - (-120.0)).abs() < 1e-6);
        assert!((bbox.north - 40.0).abs() < 1e-6);
        assert!((bbox.east - (-120.0 + 0.01 * 256.0)).abs() < 1e-6);
        assert!((bbox.south - (40.0 - 0.01 * 256.0)).abs() < 1e-6);

        // Check spatial resolution
        assert!(
            (metadata.spatial_resolution.expect("should have resolution") - 0.01).abs() < 1e-10
        );

        // Check attributes
        assert_eq!(
            metadata.attributes.get("width").map(|s| s.as_str()),
            Some("256")
        );
        assert_eq!(
            metadata.attributes.get("height").map(|s| s.as_str()),
            Some("256")
        );
        assert_eq!(
            metadata
                .attributes
                .get("bits_per_sample")
                .map(|s| s.as_str()),
            Some("8")
        );
        assert_eq!(
            metadata.attributes.get("compression").map(|s| s.as_str()),
            Some("None")
        );
    }

    #[test]
    fn test_geotiff_extraction_projected_crs() {
        let tiff_bytes = build_test_geotiff(
            512,
            512,
            16,
            1,
            Some(&[0.0, 0.0, 0.0, 500000.0, 4500000.0, 0.0]),
            Some(&[10.0, 10.0, 0.0]),
            Some(&[
                1, 1, 0, 2, 1024, 0, 1, 1, // ModelTypeProjected
                3072, 0, 1, 32632, // ProjectedCSTypeGeoKey = EPSG:32632 (UTM 32N)
            ]),
        );

        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("projected.tif");
        std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

        let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
        assert_eq!(metadata.crs.as_deref(), Some("EPSG:32632"));
        assert_eq!(
            metadata
                .attributes
                .get("bits_per_sample")
                .map(|s| s.as_str()),
            Some("16")
        );
    }

    #[test]
    fn test_geotiff_extraction_no_geokeys() {
        let tiff_bytes = build_test_geotiff(100, 100, 8, 3, None, None, None);

        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("plain.tif");
        std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

        let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
        assert_eq!(metadata.format.as_deref(), Some("GeoTIFF"));
        assert!(metadata.crs.is_none());
        assert!(metadata.bbox.is_none());
        assert_eq!(
            metadata
                .attributes
                .get("samples_per_pixel")
                .map(|s| s.as_str()),
            Some("3")
        );
    }

    #[test]
    fn test_geotiff_not_a_tiff() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("bad.tif");
        std::fs::write(&path, b"not a tiff file").expect("failed to write");

        let result = extract_from_geotiff(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_geotiff_too_small() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("tiny.tif");
        std::fs::write(&path, b"II").expect("failed to write");

        let result = extract_from_geotiff(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_geokeys_user_defined_projected() {
        let keys = vec![
            1u16, 1, 0, 1, 1024, 0, 1, 1, // ModelTypeProjected = user-defined
        ];
        let crs = parse_crs_from_geokeys(&keys, &None);
        assert_eq!(crs.as_deref(), Some("Projected CRS (user-defined)"));
    }

    #[test]
    fn test_geokeys_user_defined_geographic() {
        let keys = vec![
            1u16, 1, 0, 1, 1024, 0, 1, 2, // ModelTypeGeographic
        ];
        let crs = parse_crs_from_geokeys(&keys, &None);
        assert_eq!(crs.as_deref(), Some("Geographic CRS (user-defined)"));
    }

    #[test]
    fn test_geokeys_empty() {
        let crs = parse_crs_from_geokeys(&[], &None);
        assert!(crs.is_none());
    }

    #[test]
    fn test_to_iso19115_conversion() {
        let metadata = ExtractedMetadata {
            title: Some("Test Dataset".to_string()),
            abstract_text: Some("A test dataset".to_string()),
            bbox: Some(BoundingBox::new(-180.0, -90.0, 180.0, 90.0)),
            crs: Some("EPSG:4326".to_string()),
            keywords: vec!["geospatial".to_string(), "test".to_string()],
            ..Default::default()
        };

        let iso = to_iso19115(&metadata).expect("conversion should succeed");
        assert_eq!(iso.identification_info.len(), 1);
        assert_eq!(iso.identification_info[0].citation.title, "Test Dataset");
        assert_eq!(iso.reference_system_info.len(), 1);
    }

    #[test]
    fn test_to_fgdc_conversion() {
        let metadata = ExtractedMetadata {
            title: Some("FGDC Test".to_string()),
            abstract_text: Some("FGDC test dataset".to_string()),
            bbox: Some(BoundingBox::new(-100.0, 30.0, -90.0, 40.0)),
            ..Default::default()
        };

        let fgdc = to_fgdc(&metadata).expect("conversion should succeed");
        assert_eq!(fgdc.idinfo.citation.citeinfo.title, "FGDC Test");
    }

    #[test]
    fn test_batch_extract_mixed() {
        let results = batch_extract(vec![
            std::path::PathBuf::from("/nonexistent/file.tif"),
            std::path::PathBuf::from("/nonexistent/file.xyz"),
        ]);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_err()); // file doesn't exist
        assert!(results[1].is_err()); // unsupported format
    }

    #[test]
    fn test_extractor_filters_spatial() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("filter_test.tif");

        let tiff_bytes = build_test_geotiff(
            64,
            64,
            8,
            1,
            Some(&[0.0, 0.0, 0.0, 10.0, 50.0, 0.0]),
            Some(&[0.1, 0.1, 0.0]),
            Some(&[1, 1, 0, 1, 2048, 0, 1, 4326]),
        );
        std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

        let extractor = MetadataExtractor::new().with_spatial(false);
        let metadata = extractor.extract(&path).expect("extraction should succeed");
        assert!(metadata.bbox.is_none());
        assert!(metadata.crs.is_none());
        assert!(metadata.spatial_resolution.is_none());
    }

    /// Build a minimal valid TIFF file with optional GeoTIFF tags for testing.
    fn build_test_geotiff(
        width: u32,
        height: u32,
        bits_per_sample: u16,
        samples_per_pixel: u16,
        tiepoint: Option<&[f64]>,
        pixel_scale: Option<&[f64]>,
        geokeys: Option<&[u16]>,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4096);

        // TIFF header: little-endian, magic 42
        buf.extend_from_slice(b"II");
        buf.extend_from_slice(&42u16.to_le_bytes());

        // IFD offset at byte 8
        let ifd_offset = 8u32;
        buf.extend_from_slice(&ifd_offset.to_le_bytes());

        // Count how many IFD entries we need
        let mut entry_count: u16 = 5; // width, height, bits_per_sample, compression, samples_per_pixel
        if tiepoint.is_some() {
            entry_count += 1;
        }
        if pixel_scale.is_some() {
            entry_count += 1;
        }
        if geokeys.is_some() {
            entry_count += 1;
        }

        // IFD entry count
        buf.extend_from_slice(&entry_count.to_le_bytes());

        // Helper: write an IFD entry (tag, type, count, value/offset)
        let write_entry_long = |buf: &mut Vec<u8>, tag: u16, value: u32| {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type (for width/height we use LONG)
            buf.extend_from_slice(&1u32.to_le_bytes()); // count
            // For LONG values that should be tag type 4:
            // Actually let's use SHORT for small values, LONG for larger
            if value <= u32::from(u16::MAX) {
                buf.extend_from_slice(&(value as u16).to_le_bytes());
                buf.extend_from_slice(&0u16.to_le_bytes()); // pad
            } else {
                // Re-write type as LONG
                let type_off = buf.len() - 8;
                buf[type_off] = 4; // LONG type
                buf[type_off + 1] = 0;
                buf.extend_from_slice(&value.to_le_bytes());
            }
        };

        let write_entry_short = |buf: &mut Vec<u8>, tag: u16, value: u16| {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
            buf.extend_from_slice(&1u32.to_le_bytes()); // count
            buf.extend_from_slice(&value.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // pad
        };

        // IFD entries (must be sorted by tag)
        write_entry_long(&mut buf, 256, width); // ImageWidth
        write_entry_long(&mut buf, 257, height); // ImageLength
        write_entry_short(&mut buf, 258, bits_per_sample); // BitsPerSample
        write_entry_short(&mut buf, 259, 1); // Compression = None
        write_entry_short(&mut buf, 277, samples_per_pixel); // SamplesPerPixel

        // Compute offset for external data (after all IFD entries + next IFD offset)
        let entries_so_far = 5;
        let remaining_entries = entry_count - entries_so_far as u16;
        let _ifd_end_offset = buf.len() + (remaining_entries as usize * 12) + 4; // +4 for next IFD pointer

        // We need to plan offsets for data sections that come after the IFD
        struct DeferredData {
            entry_offset: usize, // where the offset field is in the IFD entry
            data: Vec<u8>,
        }
        let mut deferred: Vec<DeferredData> = Vec::new();

        // Write GeoTIFF tag entries with placeholder offsets
        if let Some(ps) = pixel_scale {
            let entry_offset = buf.len() + 8; // offset of the value/offset field
            buf.extend_from_slice(&33550u16.to_le_bytes()); // ModelPixelScaleTag
            buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
            buf.extend_from_slice(&(ps.len() as u32).to_le_bytes()); // count
            buf.extend_from_slice(&0u32.to_le_bytes()); // placeholder offset
            let data: Vec<u8> = ps.iter().flat_map(|v| v.to_le_bytes()).collect();
            deferred.push(DeferredData { entry_offset, data });
        }

        if let Some(tp) = tiepoint {
            let entry_offset = buf.len() + 8;
            buf.extend_from_slice(&33922u16.to_le_bytes()); // ModelTiepointTag
            buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
            buf.extend_from_slice(&(tp.len() as u32).to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            let data: Vec<u8> = tp.iter().flat_map(|v| v.to_le_bytes()).collect();
            deferred.push(DeferredData { entry_offset, data });
        }

        if let Some(gk) = geokeys {
            let entry_offset = buf.len() + 8;
            buf.extend_from_slice(&34735u16.to_le_bytes()); // GeoKeyDirectoryTag
            buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
            buf.extend_from_slice(&(gk.len() as u32).to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            let data: Vec<u8> = gk.iter().flat_map(|v| v.to_le_bytes()).collect();
            deferred.push(DeferredData { entry_offset, data });
        }

        // Next IFD offset = 0 (no more IFDs)
        buf.extend_from_slice(&0u32.to_le_bytes());

        // Now write deferred data and patch offsets
        for def in &deferred {
            let data_offset = buf.len() as u32;
            // Patch the offset in the IFD entry
            buf[def.entry_offset..def.entry_offset + 4].copy_from_slice(&data_offset.to_le_bytes());
            buf.extend_from_slice(&def.data);
        }

        buf
    }

    #[cfg(feature = "stac")]
    #[test]
    fn test_stac_extraction_full_item() {
        let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "stac_extensions": [
                "https://stac-extensions.github.io/projection/v1.1.0/schema.json"
            ],
            "id": "test-item-001",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[-122.5, 37.5], [-122.0, 37.5], [-122.0, 38.0], [-122.5, 38.0], [-122.5, 37.5]]]
            },
            "bbox": [-122.5, 37.5, -122.0, 38.0],
            "properties": {
                "datetime": "2024-01-15T10:30:00Z",
                "title": "San Francisco Bay Area",
                "description": "Sentinel-2 L2A imagery over SF Bay",
                "proj:epsg": 32610,
                "gsd": 10.0,
                "platform": "sentinel-2a",
                "constellation": "sentinel-2",
                "keywords": ["sentinel", "optical", "bay-area"]
            },
            "links": [],
            "assets": {
                "visual": {
                    "href": "https://example.com/visual.tif",
                    "type": "image/tiff; application=geotiff"
                },
                "thumbnail": {
                    "href": "https://example.com/thumb.png",
                    "type": "image/png"
                }
            },
            "collection": "sentinel-2-l2a"
        }"#;

        let metadata = extract_from_stac_json(json).expect("extraction should succeed");

        assert_eq!(metadata.title.as_deref(), Some("San Francisco Bay Area"));
        assert_eq!(
            metadata.abstract_text.as_deref(),
            Some("Sentinel-2 L2A imagery over SF Bay")
        );
        assert_eq!(metadata.format.as_deref(), Some("STAC"));
        assert_eq!(metadata.crs.as_deref(), Some("EPSG:32610"));
        assert!((metadata.spatial_resolution.expect("should have gsd") - 10.0).abs() < 1e-10);

        let bbox = metadata.bbox.expect("should have bbox");
        assert!((bbox.west - (-122.5)).abs() < 1e-6);
        assert!((bbox.south - 37.5).abs() < 1e-6);
        assert!((bbox.east - (-122.0)).abs() < 1e-6);
        assert!((bbox.north - 38.0).abs() < 1e-6);

        assert!(
            metadata
                .temporal_extent
                .as_ref()
                .expect("should have temporal")
                .start
                .is_some()
        );

        assert_eq!(metadata.keywords, vec!["sentinel", "optical", "bay-area"]);
        assert_eq!(
            metadata.attributes.get("id").map(|s| s.as_str()),
            Some("test-item-001")
        );
        assert_eq!(
            metadata.attributes.get("collection").map(|s| s.as_str()),
            Some("sentinel-2-l2a")
        );
        assert_eq!(
            metadata.attributes.get("platform").map(|s| s.as_str()),
            Some("sentinel-2a")
        );
        assert!(metadata.attributes.contains_key("asset_keys"));
    }

    #[cfg(feature = "stac")]
    #[test]
    fn test_stac_extraction_minimal_item() {
        let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "minimal-item",
            "geometry": null,
            "bbox": null,
            "properties": {
                "datetime": null
            },
            "links": [],
            "assets": {}
        }"#;

        let metadata = extract_from_stac_json(json).expect("extraction should succeed");

        assert!(metadata.title.is_none());
        assert!(metadata.abstract_text.is_none());
        assert!(metadata.bbox.is_none());
        assert!(metadata.temporal_extent.is_none());
        assert!(metadata.crs.is_none());
        assert_eq!(metadata.format.as_deref(), Some("STAC"));
        assert_eq!(
            metadata.attributes.get("id").map(|s| s.as_str()),
            Some("minimal-item")
        );
    }

    #[cfg(feature = "stac")]
    #[test]
    fn test_stac_extraction_with_temporal_range() {
        let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "temporal-range",
            "geometry": null,
            "bbox": null,
            "properties": {
                "datetime": null,
                "start_datetime": "2024-01-01T00:00:00Z",
                "end_datetime": "2024-01-31T23:59:59Z"
            },
            "links": [],
            "assets": {}
        }"#;

        let metadata = extract_from_stac_json(json).expect("extraction should succeed");
        let extent = metadata
            .temporal_extent
            .expect("should have temporal extent");
        assert!(extent.start.is_some());
        assert!(extent.end.is_some());
    }

    #[cfg(feature = "stac")]
    #[test]
    fn test_stac_extraction_invalid_json() {
        let result = extract_from_stac_json("not valid json");
        assert!(result.is_err());
    }

    #[cfg(feature = "stac")]
    #[test]
    fn test_stac_extraction_from_file() {
        let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "file-test",
            "geometry": null,
            "bbox": [-10.0, -20.0, 10.0, 20.0],
            "properties": {
                "datetime": "2024-06-01T12:00:00Z",
                "title": "File Test Item"
            },
            "links": [],
            "assets": {}
        }"#;

        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let path = dir.path().join("item.json");
        std::fs::write(&path, json).expect("failed to write");

        let metadata = extract_metadata(&path).expect("extraction should succeed");
        assert_eq!(metadata.title.as_deref(), Some("File Test Item"));
        assert_eq!(metadata.format.as_deref(), Some("STAC"));
        let bbox = metadata.bbox.expect("should have bbox");
        assert!((bbox.west - (-10.0)).abs() < 1e-6);
    }
}
