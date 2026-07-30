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
use oxigeo_core::io::{ByteRange, DataSource, FileDataSource};
use oxigeo_core::types::{GeoTransform, RasterDataType};
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

impl BytesDataSource {
    /// Borrows `range` out of the shared buffer, or reports the same
    /// end-of-file error [`DataSource::read_range`] reports for it.
    fn slice_for(&self, range: ByteRange) -> oxigeo_core::error::Result<&[u8]> {
        let eof = || {
            oxigeo_core::error::OxiGeoError::Io(oxigeo_core::error::IoError::UnexpectedEof {
                offset: range.end,
            })
        };
        let start = usize::try_from(range.start).map_err(|_| eof())?;
        let end = usize::try_from(range.end).map_err(|_| eof())?;
        // `get` rejects both an inverted range (`start > end`) and one running
        // past the buffer, exactly like the explicit checks it replaces.
        self.data.get(start..end).ok_or_else(eof)
    }
}

/// Builds the error a `read_range_into` implementation returns when the
/// caller's destination buffer cannot hold the whole range.
///
/// Mirrors the message `oxigeo_core::io`'s built-in sources produce (their
/// helper is crate-private) so the diagnostic is identical whichever source a
/// caller is holding.
fn dst_too_small(needed: usize, available: usize) -> oxigeo_core::error::OxiGeoError {
    oxigeo_core::error::OxiGeoError::invalid_parameter(
        "dst",
        format!(
            "destination buffer is {available} bytes but the requested range needs {needed}; \
             size it with ByteRange::len()"
        ),
    )
}

/// Computes the destination length `range` requires, or `None` when the range
/// is itself malformed (inverted, or wider than `usize`).
///
/// A `None` result means "let the source's own range check report it", which
/// keeps `read_range_into` erroring exactly like `read_range` instead of
/// underflowing on `ByteRange::len`.
fn needed_len(range: ByteRange) -> Option<usize> {
    usize::try_from(range.end.checked_sub(range.start)?).ok()
}

impl DataSource for BytesDataSource {
    fn size(&self) -> oxigeo_core::error::Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> oxigeo_core::error::Result<Vec<u8>> {
        Ok(self.slice_for(range)?.to_vec())
    }

    /// Copies straight out of the shared buffer, skipping the intermediate
    /// `Vec` the trait's default implementation would allocate per block
    /// (cool-japan/oxigeo#14).
    fn read_range_into(
        &self,
        range: ByteRange,
        dst: &mut [u8],
    ) -> oxigeo_core::error::Result<usize> {
        if let Some(needed) = needed_len(range)
            && dst.len() < needed
        {
            return Err(dst_too_small(needed, dst.len()));
        }
        let src = self.slice_for(range)?;
        let available = dst.len();
        let out = dst
            .get_mut(..src.len())
            .ok_or_else(|| dst_too_small(src.len(), available))?;
        out.copy_from_slice(src);
        Ok(src.len())
    }

    /// Lends the requested bytes straight out of the resident coverage buffer:
    /// decoding a tile costs neither an allocation nor a copy.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        self.slice_for(range).ok()
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
    let reader = oxigeo_geotiff::GeoTiffReader::open(source)
        .map_err(|e| ServiceError::Coverage(format!("Failed to open GeoTIFF coverage: {e}")))?;

    let width = reader.width() as usize;
    let height = reader.height() as usize;
    let bands = (reader.band_count() as usize).max(1);
    let data_type = reader
        .data_type()
        .unwrap_or_else(|| parse_data_type(&coverage.data_type));

    // `read_band(level, band)` returns ONE de-interleaved band plane
    // (`width × height × bytes_per_sample`), but every consumer of
    // `CoverageData::data` -- `write_geotiff_bytes` and
    // `coverage_to_u8_samples` -- treats it as pixel-interleaved across
    // `bands`. Read each plane and weave them back together, so a multi-band
    // coverage is served in full instead of being zero-padded (GeoTIFF) or
    // rejected as truncated (PNG/JPEG).
    // See <https://github.com/cool-japan/oxigeo/issues/14>.
    let bytes_per_sample = data_type.size_bytes();
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| ServiceError::Coverage("Coverage dimensions overflow usize".to_string()))?;
    let plane_len = pixel_count
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| ServiceError::Coverage("Coverage band size overflows usize".to_string()))?;

    let data = if bands == 1 {
        reader
            .read_band(0, 0)
            .map_err(|e| ServiceError::Coverage(format!("Failed to read GeoTIFF raster: {e}")))?
    } else {
        let total = plane_len.checked_mul(bands).ok_or_else(|| {
            ServiceError::Coverage("Coverage payload size overflows usize".to_string())
        })?;
        let mut interleaved = vec![0u8; total];
        for band in 0..bands {
            let plane = reader.read_band(0, band).map_err(|e| {
                ServiceError::Coverage(format!("Failed to read GeoTIFF band {band}: {e}"))
            })?;
            if plane.len() != plane_len {
                return Err(ServiceError::Coverage(format!(
                    "GeoTIFF band {band} returned {} bytes, expected {plane_len} \
                     ({width}x{height} x {bytes_per_sample} byte(s))",
                    plane.len()
                )));
            }
            for px in 0..pixel_count {
                let src = px * bytes_per_sample;
                let dst = (px * bands + band) * bytes_per_sample;
                interleaved[dst..dst + bytes_per_sample]
                    .copy_from_slice(&plane[src..src + bytes_per_sample]);
            }
        }
        interleaved
    };

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
        "Remote coverage fetching is unavailable: oxigeo-services was built without the \
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
    use oxigeo_geotiff::{Compression, OverviewResampling, WriterConfig};

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

    let path = std::env::temp_dir().join(format!("oxigeo_wcs_{}.tif", uuid::Uuid::new_v4()));
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
    config: oxigeo_geotiff::WriterConfig,
    payload: &[u8],
) -> ServiceResult<()> {
    use oxigeo_geotiff::{GeoTiffWriter, GeoTiffWriterOptions};

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
/// into an `f64`, reading it from a **host-native** byte slice whose length
/// matches [`RasterDataType::size_bytes`].
///
/// `CoverageData::data` is assembled from `GeoTiffReader::read_band`, which
/// normalises decoded samples to host order regardless of the file's `II`/`MM`
/// header (see `oxigeo_geotiff`'s *Byte order of decoded samples* crate docs).
/// These reads are therefore `from_ne_bytes`, not `from_le_bytes`: the two are
/// the same thing on a little-endian host, but the little-endian spelling
/// claimed a contract the data does not have and would mis-decode every
/// multi-byte coverage on a big-endian one (cool-japan/oxigeo#14).
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
            f64::from(u16::from_ne_bytes(arr))
        }
        RasterDataType::Int16 => {
            let arr: [u8; 2] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(i16::from_ne_bytes(arr))
        }
        RasterDataType::UInt32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(u32::from_ne_bytes(arr))
        }
        RasterDataType::Int32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(i32::from_ne_bytes(arr))
        }
        RasterDataType::Float32 => {
            let arr: [u8; 4] = bytes.try_into().map_err(|_| invalid())?;
            f64::from(f32::from_ne_bytes(arr))
        }
        RasterDataType::UInt64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            u64::from_ne_bytes(arr) as f64
        }
        RasterDataType::Int64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            i64::from_ne_bytes(arr) as f64
        }
        RasterDataType::Float64 => {
            let arr: [u8; 8] = bytes.try_into().map_err(|_| invalid())?;
            f64::from_ne_bytes(arr)
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

    /// cool-japan/oxigeo#14: the zero-copy entry points must agree with
    /// `read_range` byte for byte, and error for error.
    #[test]
    fn test_issue_14_bytes_source_read_range_into_matches_read_range() {
        let source = BytesDataSource {
            data: Arc::new((0u8..32).collect()),
        };
        for range in [
            ByteRange::new(0, 32),  // whole buffer
            ByteRange::new(8, 20),  // interior
            ByteRange::new(0, 1),   // leading boundary
            ByteRange::new(31, 32), // trailing boundary
            ByteRange::new(5, 5),   // empty
            ByteRange::new(32, 32), // empty at EOF
        ] {
            let expected = source.read_range(range).expect("read_range");
            let mut dst = vec![0xAAu8; expected.len()];
            let written = source.read_range_into(range, &mut dst).expect("read_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(dst, expected, "bytes mismatch for {range:?}");
        }

        // Past EOF / inverted: both paths must fail, and `read_range_into` must
        // not panic on the underflowing length.
        for range in [
            ByteRange::new(28, 40),
            ByteRange::new(32, 33),
            ByteRange::new(20, 8),
        ] {
            assert!(source.read_range(range).is_err(), "read_range {range:?}");
            let mut dst = vec![0u8; 64];
            let err = source
                .read_range_into(range, &mut dst)
                .expect_err("read_range_into should reject");
            assert!(
                matches!(
                    err,
                    oxigeo_core::error::OxiGeoError::Io(
                        oxigeo_core::error::IoError::UnexpectedEof { .. }
                    )
                ),
                "expected EOF for {range:?}, got {err}"
            );
        }
    }

    #[test]
    fn test_issue_14_bytes_source_read_range_into_buffer_sizing() {
        let source = BytesDataSource {
            data: Arc::new((0u8..16).collect()),
        };
        let range = ByteRange::new(4, 12);

        // Too long: only the first 8 bytes are written, the tail is preserved.
        let mut dst = vec![0xEEu8; 12];
        assert_eq!(
            source.read_range_into(range, &mut dst).expect("read_into"),
            8
        );
        assert_eq!(&dst[..8], &(4u8..12).collect::<Vec<u8>>()[..]);
        assert_eq!(&dst[8..], &[0xEE; 4], "tail must be left alone");

        // Too short: rejected before anything is written.
        let mut dst = vec![0xEEu8; 7];
        let err = source
            .read_range_into(range, &mut dst)
            .expect_err("short dst must be rejected");
        assert!(
            matches!(
                err,
                oxigeo_core::error::OxiGeoError::InvalidParameter { parameter, .. }
                    if parameter == "dst"
            ),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 7], "dst must be untouched");

        // An empty range writes nothing, even into an empty destination.
        assert_eq!(
            source
                .read_range_into(ByteRange::new(3, 3), &mut [])
                .expect("empty range"),
            0
        );
    }

    #[test]
    fn test_issue_14_bytes_source_range_slice_borrows_backing_buffer() {
        let payload: Arc<Vec<u8>> = Arc::new((0u8..64).collect());
        let source = BytesDataSource {
            data: Arc::clone(&payload),
        };
        let borrowed = source.range_slice(ByteRange::new(16, 48)).expect("borrow");
        assert_eq!(borrowed, &payload[16..48]);
        assert!(
            std::ptr::eq(borrowed.as_ptr(), payload[16..48].as_ptr()),
            "range_slice must borrow the shared buffer, not copy it"
        );
        assert!(
            source
                .range_slice(ByteRange::new(9, 9))
                .expect("empty")
                .is_empty()
        );
        assert!(
            source.range_slice(ByteRange::new(60, 65)).is_none(),
            "past EOF"
        );
        assert!(
            source.range_slice(ByteRange::new(40, 8)).is_none(),
            "inverted"
        );
        assert!(
            source
                .range_slice(ByteRange::new(u64::MAX - 1, u64::MAX))
                .is_none(),
            "unrepresentable offset"
        );
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
        assert!(oxigeo_geotiff::is_tiff(&bytes), "output should be a TIFF");
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

    // ---- cool-japan/oxigeo#14: multi-band coverages must decode in full ----

    /// Distinguishable sample value for `band` at pixel index `pixel`:
    /// band 0 = 10+i, band 1 = 100+i, band 2 = 200+i. A band mix-up or a
    /// missing plane therefore shows up as a wildly different byte.
    fn issue_14_sample(pixel: usize, band: usize) -> u8 {
        let base: u8 = match band {
            0 => 10,
            1 => 100,
            _ => 200,
        };
        base + pixel as u8
    }

    /// Writes a `width`x`height`, `bands`-band `UInt8` GeoTIFF fixture to a
    /// fresh temp path, returning the path and the exact band-interleaved
    /// payload that was written.
    ///
    /// Uncompressed and striped, mirroring [`write_geotiff_bytes`], so the
    /// fixture depends on no optional codec.
    fn write_issue_14_fixture(
        width: usize,
        height: usize,
        bands: usize,
    ) -> (std::path::PathBuf, Vec<u8>) {
        use oxigeo_geotiff::{
            Compression, GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
        };

        let pixel_count = width * height;
        let mut payload = Vec::with_capacity(pixel_count * bands);
        for pixel in 0..pixel_count {
            for band in 0..bands {
                payload.push(issue_14_sample(pixel, band));
            }
        }

        let mut config = WriterConfig::new(
            width as u64,
            height as u64,
            bands as u16,
            RasterDataType::UInt8,
        )
        .with_compression(Compression::None)
        .with_overviews(false, OverviewResampling::Nearest);
        config.tile_width = None;
        config.tile_height = None;

        let path =
            std::env::temp_dir().join(format!("oxigeo_wcs_issue14_{}.tif", uuid::Uuid::new_v4()));
        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("fixture writer should be creatable");
        writer.write(&payload).expect("fixture should be writable");
        drop(writer);

        (path, payload)
    }

    /// Decodes a file-backed coverage through the real GetCoverage retrieval
    /// path (`retrieve_coverage_data` -> `decode_geotiff`).
    async fn retrieve_issue_14_fixture(
        path: &std::path::Path,
        width: usize,
        height: usize,
        bands: usize,
    ) -> ServiceResult<CoverageData> {
        let mut coverage = small_coverage(CoverageSource::File(path.to_path_buf()));
        coverage.grid_size = (width, height);
        coverage.band_count = bands;
        coverage.band_names = (0..bands).map(|b| format!("Band{}", b + 1)).collect();

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
        retrieve_coverage_data(&coverage, &subset, &params).await
    }

    /// `decode_geotiff` used to store `read_band(0, 0)` -- ONE de-interleaved
    /// band plane -- as `CoverageData::data` while reporting `bands: N`, so
    /// every consumer saw a buffer `N` times too short: GeoTIFF output was
    /// zero-padded and PNG/JPEG output failed as "truncated".
    #[tokio::test]
    async fn test_issue_14_decode_geotiff_multiband_interleaves_all_bands() {
        let (width, height, bands) = (4usize, 3usize, 3usize);
        let (path, expected) = write_issue_14_fixture(width, height, bands);

        let decoded = retrieve_issue_14_fixture(&path, width, height, bands).await;
        let _ = std::fs::remove_file(&path);
        let decoded = decoded.expect("3-band coverage should decode");

        assert_eq!(decoded.width, width, "decoded width");
        assert_eq!(decoded.height, height, "decoded height");
        assert_eq!(
            decoded.bands, bands,
            "decoded band count: expected {bands}, got {}",
            decoded.bands
        );
        assert_eq!(
            decoded.data_type,
            RasterDataType::UInt8,
            "decoded data type"
        );
        assert_eq!(
            decoded.data.len(),
            width * height * bands,
            "decoded payload must hold every band: expected {} bytes for a {width}x{height} \
             x {bands} band UInt8 coverage, got {} (a single plane is {} bytes)",
            width * height * bands,
            decoded.data.len(),
            width * height
        );

        for pixel in 0..width * height {
            for band in 0..bands {
                let got = decoded.data[pixel * bands + band];
                let want = issue_14_sample(pixel, band);
                assert_eq!(
                    got,
                    want,
                    "band {band}, pixel {pixel} (row {}, col {}): expected {want}, got {got}; \
                     interleaved payload must match the {} bytes written",
                    pixel / width,
                    pixel % width,
                    expected.len()
                );
            }
        }
        assert_eq!(
            decoded.data, expected,
            "payload must round-trip byte for byte"
        );
    }

    /// The single-band path -- the common case, and the only one the older
    /// tests covered -- must be left exactly as it was by the re-interleave.
    #[tokio::test]
    async fn test_issue_14_decode_geotiff_single_band_unchanged() {
        let (width, height, bands) = (4usize, 3usize, 1usize);
        let (path, expected) = write_issue_14_fixture(width, height, bands);

        let decoded = retrieve_issue_14_fixture(&path, width, height, bands).await;
        let _ = std::fs::remove_file(&path);
        let decoded = decoded.expect("1-band coverage should decode");

        assert_eq!(decoded.width, width, "decoded width");
        assert_eq!(decoded.height, height, "decoded height");
        assert_eq!(decoded.bands, 1, "decoded band count");
        assert_eq!(
            decoded.data.len(),
            width * height,
            "single-band payload must be exactly one plane: expected {} bytes, got {}",
            width * height,
            decoded.data.len()
        );
        for (pixel, (&got, &want)) in decoded.data.iter().zip(expected.iter()).enumerate() {
            assert_eq!(got, want, "pixel {pixel}: expected {want}, got {got}");
        }
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
        // `to_ne_bytes`: `CoverageData::data` holds host-native samples.
        let mut raw = Vec::new();
        raw.extend_from_slice(&(-10.0f32).to_ne_bytes());
        raw.extend_from_slice(&(30.0f32).to_ne_bytes());
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
