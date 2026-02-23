//! WCS coverage description and retrieval
//!
//! Implements DescribeCoverage and GetCoverage operations for
//! raster data access and format conversion.

use crate::error::{ServiceError, ServiceResult};
use crate::wcs::WcsState;
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::Deserialize;

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
#[allow(dead_code)]
struct CoverageData {
    /// Raster data
    data: Vec<u8>,
    /// Width
    width: usize,
    /// Height
    height: usize,
    /// Band count
    bands: usize,
}

/// Retrieve coverage data
async fn retrieve_coverage_data(
    coverage: &crate::wcs::CoverageInfo,
    _subset: &Subset,
    _params: &GetCoverageParams,
) -> ServiceResult<CoverageData> {
    use crate::wcs::CoverageSource;

    match &coverage.source {
        CoverageSource::File(_path) => {
            // Load from file using OxiGDAL
            // For now, return placeholder data
            Ok(CoverageData {
                data: vec![0u8; coverage.grid_size.0 * coverage.grid_size.1 * coverage.band_count],
                width: coverage.grid_size.0,
                height: coverage.grid_size.1,
                bands: coverage.band_count,
            })
        }
        CoverageSource::Url(_url) => Err(ServiceError::Coverage(
            "URL-based coverages not yet implemented".to_string(),
        )),
        CoverageSource::Memory => Err(ServiceError::Coverage(
            "In-memory coverages not yet implemented".to_string(),
        )),
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

/// Encode as GeoTIFF
fn encode_as_geotiff(
    data: CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    // Use OxiGDAL GeoTIFF writer
    // For now, return placeholder
    let bytes = Bytes::from(data.data);

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

/// Encode as PNG
fn encode_as_png(
    data: CoverageData,
    coverage: &crate::wcs::CoverageInfo,
) -> Result<Response, ServiceError> {
    // For PNG, we need RGB data
    if data.bands < 3 {
        return Err(ServiceError::Coverage(
            "PNG requires at least 3 bands".to_string(),
        ));
    }

    let bytes = Bytes::from(data.data);

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
    let bytes = Bytes::from(data.data);

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
            source: CoverageSource::Memory,
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
}
