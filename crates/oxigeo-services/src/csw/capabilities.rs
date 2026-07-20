//! CSW GetCapabilities implementation
//!
//! Generates OGC-compliant CSW capabilities documents describing
//! service metadata and available operations.

use crate::csw::CswState;
use crate::error::{ServiceError, ServiceResult};
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
    state: &CswState,
    _version: &str,
) -> Result<Response, ServiceError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let mut root = BytesStart::new("csw:Capabilities");
    root.push_attribute(("version", "2.0.2"));
    root.push_attribute(("xmlns:csw", "http://www.opengis.net/cat/csw/2.0.2"));
    root.push_attribute(("xmlns:ows", "http://www.opengis.net/ows"));

    writer
        .write_event(Event::Start(root))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    write_text_element(&mut writer, "ows:Title", &state.service_info.title)?;
    write_text_element(&mut writer, "ows:ServiceType", "CSW")?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceIdentification")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("csw:Capabilities")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
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
