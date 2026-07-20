//! WFS GetCapabilities implementation
//!
//! Generates OGC-compliant WFS capabilities documents describing
//! service metadata, available feature types, and supported operations.

use crate::error::{ServiceError, ServiceResult};
use crate::wfs::WfsState;
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
    state: &WfsState,
    version: &str,
) -> Result<Response, ServiceError> {
    match version {
        "2.0.0" | "2.0.2" => generate_capabilities_200(state),
        "3.0.0" => generate_capabilities_300(state),
        _ => Err(ServiceError::InvalidParameter(
            "VERSION".to_string(),
            format!("Unsupported version: {}", version),
        )),
    }
}

/// Generate WFS 2.0.0 capabilities document
fn generate_capabilities_200(state: &WfsState) -> Result<Response, ServiceError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Root element
    let mut root = BytesStart::new("wfs:WFS_Capabilities");
    root.push_attribute(("version", "2.0.0"));
    root.push_attribute(("xmlns:wfs", "http://www.opengis.net/wfs/2.0"));
    root.push_attribute(("xmlns:ows", "http://www.opengis.net/ows/1.1"));
    root.push_attribute(("xmlns:gml", "http://www.opengis.net/gml/3.2"));
    root.push_attribute(("xmlns:fes", "http://www.opengis.net/fes/2.0"));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
    root.push_attribute((
        "xsi:schemaLocation",
        "http://www.opengis.net/wfs/2.0 http://schemas.opengis.net/wfs/2.0/wfs.xsd",
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

    // FeatureTypeList
    write_feature_type_list(&mut writer, state)?;

    // Filter_Capabilities
    write_filter_capabilities(&mut writer)?;

    // Close root element
    writer
        .write_event(Event::End(BytesEnd::new("wfs:WFS_Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Generate WFS 3.0.0 (OGC API - Features) landing page
fn generate_capabilities_300(state: &WfsState) -> Result<Response, ServiceError> {
    // OGC API - Features uses JSON for capabilities
    let capabilities = serde_json::json!({
        "title": state.service_info.title,
        "description": state.service_info.abstract_text,
        "links": [
            {
                "rel": "self",
                "type": "application/json",
                "title": "This document",
                "href": format!("{}/", state.service_info.service_url)
            },
            {
                "rel": "service-desc",
                "type": "application/vnd.oai.openapi+json;version=3.0",
                "title": "API definition",
                "href": format!("{}/api", state.service_info.service_url)
            },
            {
                "rel": "conformance",
                "type": "application/json",
                "title": "Conformance classes",
                "href": format!("{}/conformance", state.service_info.service_url)
            },
            {
                "rel": "data",
                "type": "application/json",
                "title": "Collections",
                "href": format!("{}/collections", state.service_info.service_url)
            }
        ]
    });

    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&capabilities)
            .map_err(|e| ServiceError::Serialization(e.to_string()))?,
    )
        .into_response())
}

/// Write ServiceIdentification section
fn write_service_identification(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WfsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Title
    write_text_element(writer, "ows:Title", &state.service_info.title)?;

    // Abstract
    if let Some(ref abstract_text) = state.service_info.abstract_text {
        write_text_element(writer, "ows:Abstract", abstract_text)?;
    }

    // ServiceType
    write_text_element(writer, "ows:ServiceType", "WFS")?;

    // ServiceTypeVersion
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
    state: &WfsState,
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
    state: &WfsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:OperationsMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let operations = vec!["GetCapabilities", "DescribeFeatureType", "GetFeature"];

    for op in operations {
        write_operation(writer, op, &state.service_info.service_url)?;
    }

    if state.transactions_enabled {
        write_operation(writer, "Transaction", &state.service_info.service_url)?;
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

/// Write FeatureTypeList section
fn write_feature_type_list(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WfsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("wfs:FeatureTypeList")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for entry in state.feature_types.iter() {
        let ft = entry.value();

        writer
            .write_event(Event::Start(BytesStart::new("wfs:FeatureType")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;

        write_text_element(writer, "wfs:Name", &ft.name)?;
        write_text_element(writer, "wfs:Title", &ft.title)?;

        if let Some(ref abstract_text) = ft.abstract_text {
            write_text_element(writer, "wfs:Abstract", abstract_text)?;
        }

        write_text_element(writer, "wfs:DefaultCRS", &ft.default_crs)?;

        for crs in &ft.other_crs {
            write_text_element(writer, "wfs:OtherCRS", crs)?;
        }

        // WGS84BoundingBox
        if let Some((minx, miny, maxx, maxy)) = ft.bbox {
            write_wgs84_bbox(writer, minx, miny, maxx, maxy)?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("wfs:FeatureType")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("wfs:FeatureTypeList")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write WGS84BoundingBox
fn write_wgs84_bbox(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    minx: f64,
    miny: f64,
    maxx: f64,
    maxy: f64,
) -> ServiceResult<()> {
    let mut bbox = BytesStart::new("ows:WGS84BoundingBox");
    bbox.push_attribute(("dimensions", "2"));
    writer
        .write_event(Event::Start(bbox))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:LowerCorner", &format!("{} {}", minx, miny))?;
    write_text_element(writer, "ows:UpperCorner", &format!("{} {}", maxx, maxy))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:WGS84BoundingBox")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write Filter_Capabilities section
fn write_filter_capabilities(writer: &mut Writer<Cursor<Vec<u8>>>) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("fes:Filter_Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Conformance
    writer
        .write_event(Event::Start(BytesStart::new("fes:Conformance")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let constraints = vec![
        "ImplementsQuery",
        "ImplementsAdHocQuery",
        "ImplementsFunctions",
        "ImplementsResourceId",
        "ImplementsMinStandardFilter",
        "ImplementsStandardFilter",
        "ImplementsMinSpatialFilter",
        "ImplementsSpatialFilter",
        "ImplementsMinTemporalFilter",
        "ImplementsTemporalFilter",
    ];

    for constraint in constraints {
        write_constraint(writer, constraint, "TRUE")?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("fes:Conformance")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("fes:Filter_Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

/// Write constraint element
fn write_constraint(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    value: &str,
) -> ServiceResult<()> {
    let mut constraint = BytesStart::new("fes:Constraint");
    constraint.push_attribute(("name", name));
    writer
        .write_event(Event::Start(constraint))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:DefaultValue", value)?;

    writer
        .write_event(Event::End(BytesEnd::new("fes:Constraint")))
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
    use crate::wfs::ServiceInfo;

    #[tokio::test]
    async fn test_get_capabilities_200() -> Result<(), Box<dyn std::error::Error>> {
        let info = ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: Some("Test service".to_string()),
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["2.0.0".to_string()],
        };

        let state = WfsState::new(info);

        let response = handle_get_capabilities(&state, "2.0.0").await?;

        // Response should be XML
        let (parts, _body) = response.into_parts();
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/xml")
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_get_capabilities_300() -> Result<(), Box<dyn std::error::Error>> {
        let info = ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: Some("Test service".to_string()),
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["3.0.0".to_string()],
        };

        let state = WfsState::new(info);

        let response = handle_get_capabilities(&state, "3.0.0").await?;

        // Response should be JSON
        let (parts, _body) = response.into_parts();
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/json")
        );
        Ok(())
    }
}
