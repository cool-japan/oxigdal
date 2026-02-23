//! WCS GetCapabilities implementation
//!
//! Generates OGC-compliant WCS capabilities documents describing
//! service metadata, available coverages, and supported operations.

use crate::error::{ServiceError, ServiceResult};
use crate::wcs::WcsState;
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use std::io::Cursor;

/// Handle GetCapabilities request
pub async fn handle_get_capabilities(
    state: &WcsState,
    version: &str,
) -> Result<Response, ServiceError> {
    match version {
        "2.0.0" | "2.0.1" => generate_capabilities_20(state),
        _ => Err(ServiceError::InvalidParameter(
            "VERSION".to_string(),
            format!("Unsupported version: {}", version),
        )),
    }
}

/// Generate WCS 2.0 capabilities document
fn generate_capabilities_20(state: &WcsState) -> Result<Response, ServiceError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Root element
    let mut root = BytesStart::new("wcs:Capabilities");
    root.push_attribute(("version", "2.0.1"));
    root.push_attribute(("xmlns:wcs", "http://www.opengis.net/wcs/2.0"));
    root.push_attribute(("xmlns:ows", "http://www.opengis.net/ows/2.0"));
    root.push_attribute(("xmlns:gml", "http://www.opengis.net/gml/3.2"));
    root.push_attribute(("xmlns:gmlcov", "http://www.opengis.net/gmlcov/1.0"));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
    root.push_attribute((
        "xsi:schemaLocation",
        "http://www.opengis.net/wcs/2.0 http://schemas.opengis.net/wcs/2.0/wcsAll.xsd",
    ));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // ServiceIdentification
    write_service_identification(&mut writer, state)?;

    // ServiceProvider
    write_service_provider(&mut writer, state)?;

    // OperationsMetadata
    write_operations_metadata(&mut writer, state)?;

    // ServiceMetadata
    write_service_metadata(&mut writer, state)?;

    // Contents
    write_contents(&mut writer, state)?;

    // Close root element
    writer
        .write_event(Event::End(BytesEnd::new("wcs:Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Write ServiceIdentification section
fn write_service_identification(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WcsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:Title", &state.service_info.title)?;

    if let Some(ref abstract_text) = state.service_info.abstract_text {
        write_text_element(writer, "ows:Abstract", abstract_text)?;
    }

    write_text_element(writer, "ows:ServiceType", "OGC WCS")?;

    for version in &state.service_info.versions {
        write_text_element(writer, "ows:ServiceTypeVersion", version)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write ServiceProvider section
fn write_service_provider(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WcsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceProvider")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:ProviderName", &state.service_info.provider)?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceProvider")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write OperationsMetadata section
fn write_operations_metadata(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WcsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:OperationsMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let operations = vec!["GetCapabilities", "DescribeCoverage", "GetCoverage"];

    for op in operations {
        write_operation(writer, op, &state.service_info.service_url)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("ows:OperationsMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write single operation metadata
fn write_operation(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    service_url: &str,
) -> ServiceResult<()> {
    let mut op = BytesStart::new("ows:Operation");
    op.push_attribute(("name", name));
    writer
        .write_event(Event::Start(op))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // DCP
    writer
        .write_event(Event::Start(BytesStart::new("ows:DCP")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Start(BytesStart::new("ows:HTTP")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // GET
    let mut get = BytesStart::new("ows:Get");
    get.push_attribute(("xlink:href", service_url));
    writer
        .write_event(Event::Empty(get))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // POST
    let mut post = BytesStart::new("ows:Post");
    post.push_attribute(("xlink:href", service_url));
    writer
        .write_event(Event::Empty(post))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:HTTP")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:DCP")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:Operation")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write ServiceMetadata section
fn write_service_metadata(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    _state: &WcsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("wcs:ServiceMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Format supported
    write_text_element(writer, "wcs:formatSupported", "image/tiff")?;
    write_text_element(writer, "wcs:formatSupported", "image/png")?;
    write_text_element(writer, "wcs:formatSupported", "image/jpeg")?;
    write_text_element(writer, "wcs:formatSupported", "application/netcdf")?;

    writer
        .write_event(Event::End(BytesEnd::new("wcs:ServiceMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write Contents section
fn write_contents(writer: &mut Writer<Cursor<Vec<u8>>>, state: &WcsState) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("wcs:Contents")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for entry in state.coverages.iter() {
        let coverage = entry.value();

        writer
            .write_event(Event::Start(BytesStart::new("wcs:CoverageSummary")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;

        write_text_element(writer, "wcs:CoverageId", &coverage.coverage_id)?;
        write_text_element(writer, "wcs:CoverageSubtype", "RectifiedGridCoverage")?;

        // BoundingBox
        write_bounding_box(
            writer,
            &coverage.native_crs,
            coverage.bbox.0,
            coverage.bbox.1,
            coverage.bbox.2,
            coverage.bbox.3,
        )?;

        writer
            .write_event(Event::End(BytesEnd::new("wcs:CoverageSummary")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("wcs:Contents")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write BoundingBox
fn write_bounding_box(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    crs: &str,
    minx: f64,
    miny: f64,
    maxx: f64,
    maxy: f64,
) -> ServiceResult<()> {
    let mut bbox = BytesStart::new("ows:BoundingBox");
    bbox.push_attribute(("crs", crs));
    bbox.push_attribute(("dimensions", "2"));
    writer
        .write_event(Event::Start(bbox))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:LowerCorner", &format!("{} {}", minx, miny))?;
    write_text_element(writer, "ows:UpperCorner", &format!("{} {}", maxx, maxy))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:BoundingBox")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Helper to write simple text element
fn write_text_element(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    tag: &str,
    text: &str,
) -> ServiceResult<()> {
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
    use crate::wcs::{CoverageInfo, CoverageSource, ServiceInfo};

    #[tokio::test]
    async fn test_get_capabilities() -> Result<(), Box<dyn std::error::Error>> {
        let info = ServiceInfo {
            title: "Test WCS".to_string(),
            abstract_text: Some("Test service".to_string()),
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

        let response = handle_get_capabilities(&state, "2.0.1").await?;

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
