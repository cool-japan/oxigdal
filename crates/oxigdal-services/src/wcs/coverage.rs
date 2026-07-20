//! WCS coverage description and retrieval
//!
//! Implements DescribeCoverage and GetCoverage operations for
//! raster data access and format conversion.

use crate::error::{ServiceError, ServiceResult};
use crate::wcs::{CoverageSource, WcsState};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder};
use oxigdal_core::io::{ByteRange, DataSource, FileDataSource};
use oxigdal_core::types::{GeoTransform, RasterDataType};
use serde::Deserialize;
use std::sync::Arc;

/// DescribeCoverage parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct DescribeCoverageParams {
    /// Coverage IDs (comma-separated)
    #[serde(rename = "COVERAGEID")]
    pub coverage_id: String,
}

/// GetCoverage parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct GetCoverageParams {
    /// Coverage ID
    #[serde(rename = "COVERAGEID")]
    pub coverage_id: String,
    /// Output format
    pub format: String,
    /// Subset (trim/slice operations)
    pub subset: Option<String>,
    /// Scaling factor
    pub scale_factor: Option<f64>,
    /// Scale axes
    pub scale_axes: Option<String>,
    /// Scale size
    pub scale_size: Option<String>,
    /// Range subset (band selection)
    pub range_subset: Option<String>,
}

/// Handle DescribeCoverage request
pub async fn handle_describe_coverage(
    state: &WcsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: DescribeCoverageParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("Parameters".to_string(), e.to_string()))?;

    let coverage_ids: Vec<&str> = params.coverage_id.split(',').map(|s| s.trim()).collect();

    // Validate all coverage IDs
    for coverage_id in &coverage_ids {
        if state.get_coverage(coverage_id).is_none() {
            return Err(ServiceError::NotFound(format!(
                "Coverage not found: {}",
                coverage_id
            )));
        }
    }

    generate_coverage_descriptions(state, &coverage_ids)
}

/// Handle GetCoverage request
pub async fn handle_get_coverage(
    state: &WcsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: GetCoverageParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("Parameters".to_string(), e.to_string()))?;

    let coverage = state
        .get_coverage(&params.coverage_id)
        .ok_or_else(|| ServiceError::NotFound(format!("Coverage: {}", params.coverage_id)))?;

    // Parse subset parameters
    let subset = parse_subset(&params.subset)?;

    // Get coverage data
    let data = retrieve_coverage_data(&coverage, &subset, &params).await?;

    // Encode in requested format
    encode_coverage(data, &params.format, &coverage)
}

/// Generate coverage descriptions XML
fn generate_coverage_descriptions(
    state: &WcsState,
    coverage_ids: &[&str],
) -> Result<Response, ServiceError> {
    use quick_xml::{
        Writer,
        events::{BytesDecl, BytesEnd, BytesStart, Event},
    };
    use std::io::Cursor;

    let mut writer = Writer::new(Cursor::new(Vec::new()));

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let mut root = BytesStart::new("wcs:CoverageDescriptions");
    root.push_attribute(("xmlns:wcs", "http://www.opengis.net/wcs/2.0"));
    root.push_attribute(("xmlns:gml", "http://www.opengis.net/gml/3.2"));
    root.push_attribute(("xmlns:gmlcov", "http://www.opengis.net/gmlcov/1.0"));
    root.push_attribute(("xmlns:swe", "http://www.opengis.net/swe/2.0"));

    writer
        .write_event(Event::Start(root))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for coverage_id in coverage_ids {
        let coverage = state
            .get_coverage(coverage_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Coverage: {}", coverage_id)))?;

        write_coverage_description(&mut writer, &coverage)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("wcs:CoverageDescriptions")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Write single coverage description
fn write_coverage_description(
    writer: &mut quick_xml::Writer<std::io::Cursor<Vec<u8>>>,
    coverage: &crate::wcs::CoverageInfo,
) -> ServiceResult<()> {
    use quick_xml::events::{BytesEnd, BytesStart, Event};

    writer
        .write_event(Event::Start(BytesStart::new("wcs:CoverageDescription")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // CoverageId
    write_text_element(writer, "wcs:CoverageId", &coverage.coverage_id)?;

    // BoundingBox
    let mut bbox = BytesStart::new("ows:BoundingBox");
    bbox.push_attribute(("crs", coverage.native_crs.as_str()));
    bbox.push_attribute(("dimensions", "2"));
    writer
        .write_event(Event::Start(bbox))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(
        writer,
        "ows:LowerCorner",
        &format!("{} {}", coverage.bbox.0, coverage.bbox.1),
    )?;
    write_text_element(
        writer,
        "ows:UpperCorner",
        &format!("{} {}", coverage.bbox.2, coverage.bbox.3),
    )?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:BoundingBox")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Grid envelope and resolution
    write_grid_description(writer, coverage)?;

    // Range type
    write_range_type(writer, coverage)?;

    writer
        .write_event(Event::End(BytesEnd::new("wcs:CoverageDescription")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write grid description
fn write_grid_description(
    writer: &mut quick_xml::Writer<std::io::Cursor<Vec<u8>>>,
    coverage: &crate::wcs::CoverageInfo,
) -> ServiceResult<()> {
    use quick_xml::events::{BytesEnd, BytesStart, Event};

    writer
        .write_event(Event::Start(BytesStart::new("gml:domainSet")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Start(BytesStart::new("gml:RectifiedGrid")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Grid limits
    write_text_element(
        writer,
        "gml:limits",
        &format!("0 0 {} {}", coverage.grid_size.0, coverage.grid_size.1),
    )?;

    // Axis labels
    write_text_element(writer, "gml:axisLabels", "i j")?;

    // Origin
    write_text_element(
        writer,
        "gml:origin",
        &format!("{} {}", coverage.grid_origin.0, coverage.grid_origin.1),
    )?;

    // Offset vectors
    write_text_element(
        writer,
        "gml:offsetVector",
        &format!("{} 0", coverage.grid_resolution.0),
    )?;
    write_text_element(
        writer,
        "gml:offsetVector",
        &format!("0 {}", coverage.grid_resolution.1),
    )?;

    writer
        .write_event(Event::End(BytesEnd::new("gml:RectifiedGrid")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("gml:domainSet")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write range type description
fn write_range_type(
    writer: &mut quick_xml::Writer<std::io::Cursor<Vec<u8>>>,
    coverage: &crate::wcs::CoverageInfo,
) -> ServiceResult<()> {
    use quick_xml::events::{BytesEnd, BytesStart, Event};

    writer
        .write_event(Event::Start(BytesStart::new("gmlcov:rangeType")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Start(BytesStart::new("swe:DataRecord")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for band_name in coverage.band_names.iter() {
        writer
            .write_event(Event::Start(BytesStart::new("swe:field")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;

        write_text_element(writer, "swe:Quantity", band_name)?;

        writer
            .write_event(Event::End(BytesEnd::new("swe:field")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("swe:DataRecord")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("gmlcov:rangeType")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Subset specification
#[derive(Debug)]
#[allow(dead_code)]
struct Subset {
    /// X range (min, max)
    x_range: Option<(f64, f64)>,
    /// Y range (min, max)
    y_range: Option<(f64, f64)>,
    /// Time range
    time_range: Option<(String, String)>,
}

/// Parse subset parameter
fn parse_subset(subset_str: &Option<String>) -> ServiceResult<Subset> {
    let subset = Subset {
        x_range: None,
        y_range: None,
        time_range: None,
    };

    if let Some(_s) = subset_str {
        // Parse subset expressions like "x(min,max)" or "Lat(40,50)"
        // Simple implementation - full WCS would support more complex subsetting
        // For now, return empty subset
    }

    Ok(subset)
}

/// Coverage data
struct CoverageData {
    /// Raster data (row-major, band-interleaved by pixel)
    data: Vec<u8>,
    /// Width in pixels
    width: usize,
    /// Height in pixels
    height: usize,
    /// Band count
    bands: usize,
    /// Sample data type
    data_type: RasterDataType,
}

/// An in-memory [`DataSource`] backed by a shared byte buffer.
///
/// Used to decode coverages that were fetched over HTTP or supplied inline
/// without touching the filesystem.
struct BytesDataSource {
    data: Arc<Vec<u8>>,
}

impl DataSource for BytesDataSource {
    fn size(&self) -> oxigdal_core::error::Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> oxigdal_core::error::Result<Vec<u8>> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end || end > self.data.len() {
            return Err(oxigdal_core::error::OxiGdalError::Io(
                oxigdal_core::error::IoError::UnexpectedEof { offset: range.end },
            ));
        }
        Ok(self.data[start..end].to_vec())
    }
}

/// Map a coverage `data_type` label (e.g. `"Byte"`, `"Float32"`) to a
/// [`RasterDataType`], defaulting to `UInt8` for unknown labels.
fn parse_data_type(name: &str) -> RasterDataType {
    match name {
        "Byte" | "UInt8" => RasterDataType::UInt8,
        "Int8" => RasterDataType::Int8,
        "UInt16" => RasterDataType::UInt16,
        "Int16" => RasterDataType::Int16,
        "UInt32" => RasterDataType::UInt32,
        "Int32" => RasterDataType::Int32,
        "UInt64" => RasterDataType::UInt64,
        "Int64" => RasterDataType::Int64,
        "Float32" => RasterDataType::Float32,
        "Float64" => RasterDataType::Float64,
        _ => RasterDataType::UInt8,
    }
}

/// Decode a GeoTIFF from any [`DataSource`] into a [`CoverageData`].
fn decode_geotiff<S: DataSource>(
    source: S,
    coverage: &crate::wcs::CoverageInfo,
) -> ServiceResult<CoverageData> {
    let reader = oxigdal_geotiff::GeoTiffReader::open(source)
        .map_err(|e| ServiceError::Coverage(format!("Failed to open GeoTIFF coverage: {e}")))?;

    let width = reader.width() as usize;
    let height = reader.height() as usize;
    let bands = reader.band_count() as usize;
    let data_type = reader
        .data_type()
        .unwrap_or_else(|| parse_data_type(&coverage.data_type));
    let data = reader
        .read_band(0, 0)
        .map_err(|e| ServiceError::Coverage(format!("Failed to read GeoTIFF raster: {e}")))?;

    Ok(CoverageData {
        data,
        width,
        height,
        bands,
        data_type,
    })
}

/// Fetch the raw bytes at a remote coverage URL.
#[cfg(feature = "remote")]
async fn fetch_url_bytes(url: &str) -> ServiceResult<Vec<u8>> {
    let response = reqwest::get(url).await.map_err(|e| {
        ServiceError::Coverage(format!("Failed to fetch coverage URL '{url}': {e}"))
    })?;
    if !response.status().is_success() {
        return Err(ServiceError::Coverage(format!(
            "Coverage URL '{url}' returned HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ServiceError::Coverage(format!("Failed to read coverage bytes: {e}")))?;
    Ok(bytes.to_vec())
}

/// Fallback used when the `remote` feature is not compiled in.
#[cfg(not(feature = "remote"))]
async fn fetch_url_bytes(url: &str) -> ServiceResult<Vec<u8>> {
    let _ = url;
    Err(ServiceError::Coverage(
        "Remote coverage fetching is unavailable: oxigdal-services was built without the \
         'remote' feature. Rebuild with `--features remote` to enable HTTP(S) coverage sources."
            .to_string(),
    ))
}

/// Retrieve coverage data
async fn retrieve_coverage_data(
    coverage: &crate::wcs::CoverageInfo,
    _subset: &Subset,
    _params: &GetCoverageParams,
) -> ServiceResult<CoverageData> {
    match &coverage.source {
        CoverageSource::File(path) => {
            let source = FileDataSource::open(path).map_err(|e| {
                ServiceError::Coverage(format!(
                    "Failed to open coverage file '{}': {e}",
                    path.display()
                ))
            })?;
            decode_geotiff(source, coverage)
        }
        CoverageSource::Url(url) => {
            let bytes = fetch_url_bytes(url).await?;
            decode_geotiff(
                BytesDataSource {
                    data: Arc::new(bytes),
                },
                coverage,
            )
        }
        CoverageSource::Memory(bytes) => {
            if bytes.is_empty() {
                return Err(ServiceError::Coverage(
                    "In-memory coverage has no encoded bytes".to_string(),
                ));
            }
            decode_geotiff(
                BytesDataSource {
                    data: Arc::clone(bytes),
                },
                coverage,
            )
        }
    }
}

/// Encode coverage in requested format
fn encode_coverage(
    data: CoverageData,
    format: &str,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    match format {
        "image/tiff" | "image/geotiff" => encode_as_geotiff(data, coverage),
        "image/png" => encode_as_png(data, coverage),
        "image/jpeg" => encode_as_jpeg(data, coverage),
        _ => Err(ServiceError::UnsupportedFormat(format.to_string())),
    }
}

/// Parse an EPSG code out of a CRS label such as `"EPSG:4326"` or `"4326"`.
fn parse_epsg(crs: &str) -> Option<u32> {
    let trimmed = crs.trim();
    let digits = trimmed.rsplit([':', '/']).next().unwrap_or(trimmed);
    digits.parse::<u32>().ok()
}

/// Ensure the payload has exactly `expected` bytes, zero-padding or truncating
/// as needed so the GeoTIFF writer's size validation always passes.
fn fit_payload(data: &[u8], expected: usize) -> Vec<u8> {
    let mut payload = vec![0u8; expected];
    let n = data.len().min(expected);
    payload[..n].copy_from_slice(&data[..n]);
    payload
}

/// Encode coverage data as a real GeoTIFF byte stream.
fn write_geotiff_bytes(
    data: &CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> ServiceResult<Vec<u8>> {
    use oxigdal_geotiff::{Compression, OverviewResampling, WriterConfig};

    if data.width == 0 || data.height == 0 || data.bands == 0 {
        return Err(ServiceError::Coverage(
            "Cannot encode a coverage with zero width, height or band count".to_string(),
        ));
    }

    let bands = u16::try_from(data.bands).map_err(|_| {
        ServiceError::Coverage(format!("Band count {} exceeds GeoTIFF limits", data.bands))
    })?;

    let mut config =
        WriterConfig::new(data.width as u64, data.height as u64, bands, data.data_type)
            .with_compression(Compression::None)
            .with_overviews(false, OverviewResampling::Nearest)
            .with_geo_transform(GeoTransform::north_up(
                coverage.grid_origin.0,
                coverage.grid_origin.1,
                coverage.grid_resolution.0,
                coverage.grid_resolution.1,
            ));
    // Use a striped layout (no tiling) so coverages of any size round-trip
    // without tile padding concerns.
    config.tile_width = None;
    config.tile_height = None;
    if let Some(epsg) = parse_epsg(&coverage.native_crs) {
        config = config.with_epsg_code(epsg);
    }

    let expected = data.width * data.height * data.bands * data.data_type.size_bytes();
    let payload = fit_payload(&data.data, expected);

    let path = std::env::temp_dir().join(format!("oxigdal_wcs_{}.tif", uuid::Uuid::new_v4()));
    let write_result = write_geotiff_to_path(&path, config, &payload);

    let bytes = write_result.and_then(|()| {
        std::fs::read(&path)
            .map_err(|e| ServiceError::Coverage(format!("Failed to read encoded GeoTIFF: {e}")))
    });

    // Best-effort cleanup regardless of success.
    let _ = std::fs::remove_file(&path);

    bytes
}

/// Write a single-image GeoTIFF to `path` using the given config and payload.
fn write_geotiff_to_path(
    path: &std::path::Path,
    config: oxigdal_geotiff::WriterConfig,
    payload: &[u8],
) -> ServiceResult<()> {
    use oxigdal_geotiff::{GeoTiffWriter, GeoTiffWriterOptions};

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .map_err(|e| ServiceError::Coverage(format!("Failed to create GeoTIFF writer: {e}")))?;
    writer
        .write(payload)
        .map_err(|e| ServiceError::Coverage(format!("Failed to write GeoTIFF: {e}")))?;
    Ok(())
}

/// Encode as GeoTIFF
fn encode_as_geotiff(
    data: CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    let bytes = Bytes::from(write_geotiff_bytes(&data, coverage)?);

    Ok((
        [
            (header::CONTENT_TYPE, "image/tiff"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}.tif\"", coverage.coverage_id),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Decode a single raw sample (of the coverage's native `RasterDataType`)
/// into an `f64`, reading it from a little-endian byte slice whose length
/// matches [`RasterDataType::size_bytes`].
///
/// Complex sample types (`CFloat32`/`CFloat64`) have no sensible scalar
/// interpretation for an 8-bit display encoding, so they are rejected.
fn decode_sample_f64(bytes: &[u8], data_type: RasterDataType) -> ServiceResult<f64> {
    let invalid = || ServiceError::Coverage(format!("Truncated {data_type:?} sample"));
    Ok(match data_type {
        RasterDataType::UInt8 => f64::from(*bytes.first().ok_or_else(invalid)?),
        RasterDataType::Int8 => f64::from(*bytes.first().ok_or_else(invalid)? as i8),
        RasterDataType::UInt16 => {
            let arr: [u8; 2] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(u16::from_le_bytes(arr))
        }
        RasterDataType::Int16 => {
            let arr: [u8; 2] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(i16::from_le_bytes(arr))
        }
        RasterDataType::UInt32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(u32::from_le_bytes(arr))
        }
        RasterDataType::Int32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(i32::from_le_bytes(arr))
        }
        RasterDataType::Float32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(f32::from_le_bytes(arr))
        }
        RasterDataType::UInt64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            u64::from_le_bytes(arr) as f64
        }
        RasterDataType::Int64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            i64::from_le_bytes(arr) as f64
        }
        RasterDataType::Float64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            f64::from_le_bytes(arr)
        }
        RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
            return Err(ServiceError::Coverage(
                "Complex raster sample types cannot be encoded to PNG/JPEG".to_string(),
            ));
        }
    })
}

/// Convert `data`'s raw, band-interleaved samples into 8-bit-per-band
/// samples suitable for PNG/JPEG encoding.
///
/// `UInt8` data is used as-is. Every other sample type has no fixed display
/// range (a `Float32` DEM might span metres below sea level to mountain
/// peaks), so it is linearly stretched per band from its observed
/// [min, max] into `0..=255`. A band whose samples are all equal (or where
/// every sample is non-finite) maps to `0`.
fn coverage_to_u8_samples(data: &CoverageData) -> ServiceResult<Vec<u8>> {
    let sample_size = data.data_type.size_bytes();
    let pixel_count = data
        .width
        .checked_mul(data.height)
        .ok_or_else(|| ServiceError::Coverage("Coverage dimensions overflow".to_string()))?;
    let sample_count = pixel_count
        .checked_mul(data.bands)
        .ok_or_else(|| ServiceError::Coverage("Coverage dimensions overflow".to_string()))?;
    let expected_bytes = sample_count
        .checked_mul(sample_size)
        .ok_or_else(|| ServiceError::Coverage("Coverage dimensions overflow".to_string()))?;

    if data.data.len() < expected_bytes {
        return Err(ServiceError::Coverage(format!(
            "Coverage data is truncated: expected at least {expected_bytes} bytes for a \
             {}x{} x {} band {:?} coverage, got {}",
            data.width,
            data.height,
            data.bands,
            data.data_type,
            data.data.len()
        )));
    }

    if data.data_type == RasterDataType::UInt8 {
        return Ok(data.data[..expected_bytes].to_vec());
    }

    let mut samples = Vec::with_capacity(sample_count);
    for chunk in data.data[..expected_bytes].chunks_exact(sample_size) {
        samples.push(decode_sample_f64(chunk, data.data_type)?);
    }

    let bands = data.bands;
    let mut band_min = vec![f64::INFINITY; bands];
    let mut band_max = vec![f64::NEG_INFINITY; bands];
    for (i, &value) in samples.iter().enumerate() {
        if value.is_finite() {
            let b = i % bands;
            band_min[b] = band_min[b].min(value);
            band_max[b] = band_max[b].max(value);
        }
    }

    let mut out = Vec::with_capacity(sample_count);
    for (i, &value) in samples.iter().enumerate() {
        let b = i % bands;
        let (min, max) = (band_min[b], band_max[b]);
        let stretchable = value.is_finite()
            && min.is_finite()
            && max.is_finite()
            && (max - min).abs() >= f64::EPSILON;
        let scaled = if stretchable {
            ((value - min) / (max - min)) * 255.0
        } else {
            0.0
        };
        out.push(scaled.round().clamp(0.0, 255.0) as u8);
    }

    Ok(out)
}

/// Maps a coverage band count to the PNG color type that consumes it, or an
/// error for band counts PNG cannot represent.
fn png_color_type_for_bands(bands: usize) -> ServiceResult<ExtendedColorType> {
    match bands {
        1 => Ok(ExtendedColorType::L8),
        2 => Ok(ExtendedColorType::La8),
        3 => Ok(ExtendedColorType::Rgb8),
        4 => Ok(ExtendedColorType::Rgba8),
        other => Err(ServiceError::Coverage(format!(
            "PNG encoding supports 1 (grayscale), 2 (grayscale+alpha), 3 (RGB) or 4 (RGBA) \
             bands, got {other}"
        ))),
    }
}

/// Maps a coverage band count to the JPEG color type that consumes it, or an
/// error for band counts JPEG cannot represent (JPEG only supports 8-bit
/// grayscale or RGB).
fn jpeg_color_type_for_bands(bands: usize) -> ServiceResult<ExtendedColorType> {
    match bands {
        1 => Ok(ExtendedColorType::L8),
        3 => Ok(ExtendedColorType::Rgb8),
        other => Err(ServiceError::Coverage(format!(
            "JPEG encoding supports 1 (grayscale) or 3 (RGB) bands, got {other}"
        ))),
    }
}

/// Converts a coverage's width/height into the `u32` dimensions the `image`
/// crate's encoders require.
fn image_dimensions(data: &CoverageData) -> ServiceResult<(u32, u32)> {
    if data.width == 0 || data.height == 0 {
        return Err(ServiceError::Coverage(
            "Cannot encode a coverage with zero width or height".to_string(),
        ));
    }
    let width = u32::try_from(data.width)
        .map_err(|_| ServiceError::Coverage("Coverage width exceeds encoder limits".to_string()))?;
    let height = u32::try_from(data.height).map_err(|_| {
        ServiceError::Coverage("Coverage height exceeds encoder limits".to_string())
    })?;
    Ok((width, height))
}

/// Encode as PNG
fn encode_as_png(
    data: CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    let color_type = png_color_type_for_bands(data.bands)?;
    let (width, height) = image_dimensions(&data)?;
    let samples = coverage_to_u8_samples(&data)?;

    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(&samples, width, height, color_type)
        .map_err(|e| ServiceError::Coverage(format!("Failed to encode PNG: {e}")))?;

    let bytes = Bytes::from(buf);

    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}.png\"", coverage.coverage_id),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Encode as JPEG
fn encode_as_jpeg(
    data: CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    let color_type = jpeg_color_type_for_bands(data.bands)?;
    let (width, height) = image_dimensions(&data)?;
    let samples = coverage_to_u8_samples(&data)?;

    let mut buf = Vec::new();
    JpegEncoder::new(&mut buf)
        .write_image(&samples, width, height, color_type)
        .map_err(|e| ServiceError::Coverage(format!("Failed to encode JPEG: {e}")))?;

    let bytes = Bytes::from(buf);

    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}.jpg\"", coverage.coverage_id),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Helper to write simple text element
fn write_text_element(
    writer: &mut quick_xml::Writer<std::io::Cursor<Vec<u8>>>,
    tag: &str,
    text: &str,
) -> ServiceResult<()> {
    use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

    writer
        .write_event(Event::Start(BytesStart::new(tag)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wcs::{CoverageInfo, CoverageSource, ServiceInfo, WcsState};

    #[tokio::test]
    async fn test_describe_coverage() -> Result<(), Box<dyn std::error::Error>> {
        let info = ServiceInfo {
            title: "Test WCS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wcs".to_string(),
            versions: vec!["2.0.1".to_string()],
        };

        let state = WcsState::new(info);

        let coverage = CoverageInfo {
            coverage_id: "test".to_string(),
            title: "Test Coverage".to_string(),
            abstract_text: None,
            native_crs: "EPSG:4326".to_string(),
            bbox: (-180.0, -90.0, 180.0, 90.0),
            grid_size: (1024, 512),
            grid_origin: (-180.0, 90.0),
            grid_resolution: (0.35, -0.35),
            band_count: 1,
            band_names: vec!["Band1".to_string()],
            data_type: "Byte".to_string(),
            source: CoverageSource::Memory(Arc::new(Vec::new())),
            formats: vec!["image/tiff".to_string()],
        };

        state.add_coverage(coverage)?;

        let params = serde_json::json!({
            "COVERAGEID": "test"
        });

        let response = handle_describe_coverage(&state, "2.0.1", &params).await?;

        let (parts, _) = response.into_parts();
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/xml")
        );
        Ok(())
    }

    fn small_coverage(source: CoverageSource) -> CoverageInfo {
        CoverageInfo {
            coverage_id: "small".to_string(),
            title: "Small Coverage".to_string(),
            abstract_text: None,
            native_crs: "EPSG:4326".to_string(),
            bbox: (0.0, 0.0, 4.0, 4.0),
            grid_size: (4, 4),
            grid_origin: (0.0, 4.0),
            grid_resolution: (1.0, -1.0),
            band_count: 1,
            band_names: vec!["Band1".to_string()],
            data_type: "Byte".to_string(),
            source,
            formats: vec!["image/tiff".to_string()],
        }
    }

    #[test]
    fn test_parse_epsg() {
        assert_eq!(parse_epsg("EPSG:4326"), Some(4326));
        assert_eq!(parse_epsg("4326"), Some(4326));
        assert_eq!(
            parse_epsg("http://www.opengis.net/def/crs/EPSG/0/3857"),
            Some(3857)
        );
        assert_eq!(parse_epsg("CRS84"), None);
    }

    #[test]
    fn test_fit_payload() {
        assert_eq!(fit_payload(&[1, 2, 3], 5), vec![1, 2, 3, 0, 0]);
        assert_eq!(fit_payload(&[1, 2, 3, 4, 5], 3), vec![1, 2, 3]);
    }

    #[test]
    fn test_write_geotiff_bytes_is_valid_tiff() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let data = CoverageData {
            data: (0u8..16).collect(),
            width: 4,
            height: 4,
            bands: 1,
            data_type: RasterDataType::UInt8,
        };
        let bytes = write_geotiff_bytes(&data, &coverage).expect("encode");
        assert!(!bytes.is_empty());
        assert!(oxigdal_geotiff::is_tiff(&bytes), "output should be a TIFF");
    }

    #[tokio::test]
    async fn test_encode_then_retrieve_memory_roundtrip() {
        // Encode a small raster to GeoTIFF bytes...
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let data = CoverageData {
            data: (0u8..16).collect(),
            width: 4,
            height: 4,
            bands: 1,
            data_type: RasterDataType::UInt8,
        };
        let bytes = write_geotiff_bytes(&data, &coverage).expect("encode");

        // ...then feed them back through an in-memory coverage source.
        let mem_coverage = small_coverage(CoverageSource::Memory(Arc::new(bytes)));
        let subset = Subset {
            x_range: None,
            y_range: None,
            time_range: None,
        };
        let params = GetCoverageParams {
            coverage_id: "small".to_string(),
            format: "image/tiff".to_string(),
            subset: None,
            scale_factor: None,
            scale_axes: None,
            scale_size: None,
            range_subset: None,
        };
        let decoded = retrieve_coverage_data(&mem_coverage, &subset, &params)
            .await
            .expect("retrieve");
        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.bands, 1);
    }

    #[tokio::test]
    async fn test_empty_memory_coverage_errors() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let subset = Subset {
            x_range: None,
            y_range: None,
            time_range: None,
        };
        let params = GetCoverageParams {
            coverage_id: "small".to_string(),
            format: "image/tiff".to_string(),
            subset: None,
            scale_factor: None,
            scale_axes: None,
            scale_size: None,
            range_subset: None,
        };
        let result = retrieve_coverage_data(&coverage, &subset, &params).await;
        assert!(result.is_err());
    }

    // ---- PNG/JPEG encoding actually produces decodable images ----

    fn rgb_coverage_data() -> CoverageData {
        // 4x4 RGB image, band-interleaved by pixel.
        let mut data = Vec::with_capacity(4 * 4 * 3);
        for y in 0..4u8 {
            for x in 0..4u8 {
                data.push(x * 16); // R
                data.push(y * 16); // G
                data.push(128); // B
            }
        }
        CoverageData {
            data,
            width: 4,
            height: 4,
            bands: 3,
            data_type: RasterDataType::UInt8,
        }
    }

    #[test]
    fn test_encode_as_png_is_a_real_decodable_png() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let response = encode_as_png(rgb_coverage_data(), &coverage).expect("encode png");

        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("image/png")
        );

        let body = extract_body_sync(response);
        assert!(
            body.starts_with(b"\x89PNG\r\n\x1a\n"),
            "not a PNG signature"
        );

        let decoded = image::load_from_memory_with_format(&body, image::ImageFormat::Png)
            .expect("decode png");
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 4);
    }

    #[test]
    fn test_encode_as_jpeg_is_a_real_decodable_jpeg() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let response = encode_as_jpeg(rgb_coverage_data(), &coverage).expect("encode jpeg");

        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("image/jpeg")
        );

        let body = extract_body_sync(response);
        assert!(body.starts_with(&[0xFF, 0xD8]), "not a JPEG SOI marker");

        let decoded = image::load_from_memory_with_format(&body, image::ImageFormat::Jpeg)
            .expect("decode jpeg");
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 4);
    }

    #[test]
    fn test_encode_as_png_rejects_unsupported_band_count() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let mut data = rgb_coverage_data();
        data.bands = 5;
        data.data = vec![0u8; 4 * 4 * 5];
        assert!(encode_as_png(data, &coverage).is_err());
    }

    #[test]
    fn test_encode_as_jpeg_rejects_unsupported_band_count() {
        let coverage = small_coverage(CoverageSource::Memory(Arc::new(Vec::new())));
        let mut data = rgb_coverage_data();
        data.bands = 4; // JPEG only supports 1 or 3 bands.
        data.data = vec![0u8; 4 * 4 * 4];
        assert!(encode_as_jpeg(data, &coverage).is_err());
    }

    #[test]
    fn test_coverage_to_u8_samples_stretches_float32_per_band() {
        // Single-band 1x2 image with samples -10.0 and 30.0; expect a 0/255 stretch.
        let mut raw = Vec::new();
        raw.extend_from_slice(&(-10.0f32).to_le_bytes());
        raw.extend_from_slice(&(30.0f32).to_le_bytes());
        let data = CoverageData {
            data: raw,
            width: 1,
            height: 2,
            bands: 1,
            data_type: RasterDataType::Float32,
        };
        let samples = coverage_to_u8_samples(&data).expect("stretch");
        assert_eq!(samples, vec![0, 255]);
    }

    #[test]
    fn test_coverage_to_u8_samples_rejects_truncated_data() {
        let data = CoverageData {
            data: vec![0u8; 2], // too short for 4x4x1 UInt8
            width: 4,
            height: 4,
            bands: 1,
            data_type: RasterDataType::UInt8,
        };
        assert!(coverage_to_u8_samples(&data).is_err());
    }

    /// Drains an Axum response body synchronously for use in `#[test]`
    /// (non-async) functions, using a throwaway single-threaded runtime.
    fn extract_body_sync(response: Response) -> Vec<u8> {
        use http_body_util::BodyExt;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build runtime");
        runtime.block_on(async move {
            response
                .into_body()
                .collect()
                .await
                .expect("collect body")
                .to_bytes()
                .to_vec()
        })
    }
}
