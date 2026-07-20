//! WPS GetCapabilities implementation

use crate::error::{ServiceError, ServiceResult};
use crate::wps::WpsState;
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
    state: &WpsState,
    version: &str,
) -> Result<Response, ServiceError> {
    match version {
        "2.0.0" => generate_capabilities_20(state),
        _ => Err(ServiceError::InvalidParameter(
            "VERSION".to_string(),
            format!("Unsupported version: {}", version),
        )),
    }
}

/// Generate WPS 2.0 capabilities document
fn generate_capabilities_20(state: &WpsState) -> Result<Response, ServiceError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let mut root = BytesStart::new("wps:Capabilities");
    root.push_attribute(("version", "2.0.0"));
    root.push_attribute(("xmlns:wps", "http://www.opengis.net/wps/2.0"));
    root.push_attribute(("xmlns:ows", "http://www.opengis.net/ows/2.0"));

    writer
        .write_event(Event::Start(root))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // ServiceIdentification
    write_service_identification(&mut writer, state)?;

    // ServiceProvider
    write_service_provider(&mut writer, state)?;

    // OperationsMetadata
    write_operations_metadata(&mut writer, state)?;

    // Process offerings
    write_process_offerings(&mut writer, state)?;

    writer
        .write_event(Event::End(BytesEnd::new("wps:Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

fn write_service_identification(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WpsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(writer, "ows:Title", &state.service_info.title)?;
    write_text_element(writer, "ows:ServiceType", "WPS")?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

fn write_service_provider(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WpsState,
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

fn write_operations_metadata(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    _state: &WpsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:OperationsMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let operations = vec!["GetCapabilities", "DescribeProcess", "Execute"];
    for op in operations {
        write_text_element(writer, "ows:Operation", op)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("ows:OperationsMetadata")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

fn write_process_offerings(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WpsState,
) -> ServiceResult<()> {
    writer
        .write_event(Event::Start(BytesStart::new("wps:Contents")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for entry in state.processes.iter() {
        let process = entry.value();

        writer
            .write_event(Event::Start(BytesStart::new("wps:ProcessSummary")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;

        write_text_element(writer, "ows:Identifier", process.identifier())?;
        write_text_element(writer, "ows:Title", process.title())?;

        writer
            .write_event(Event::End(BytesEnd::new("wps:ProcessSummary")))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("wps:Contents")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(())
}

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
