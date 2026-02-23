//! CSW record retrieval

use crate::csw::CswState;
use crate::error::ServiceError;
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

/// Parameters for GetRecords request
#[derive(Debug, Deserialize)]
pub struct GetRecordsParams {
    /// Maximum number of records to return
    pub max_records: Option<usize>,
}

/// Parameters for GetRecordById request
#[derive(Debug, Deserialize)]
pub struct GetRecordByIdParams {
    /// Record identifier
    pub id: String,
}

/// Handle GetRecords request
pub async fn handle_get_records(
    _state: &CswState,
    _version: &str,
    _params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<csw:GetRecordsResponse xmlns:csw="http://www.opengis.net/cat/csw/2.0.2">
  <csw:SearchStatus timestamp="2026-01-26T00:00:00Z"/>
  <csw:SearchResults numberOfRecordsMatched="0" numberOfRecordsReturned="0"/>
</csw:GetRecordsResponse>"#;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Handle GetRecordById request
pub async fn handle_get_record_by_id(
    state: &CswState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: GetRecordByIdParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("id".to_string(), e.to_string()))?;

    if state.records.get(&params.id).is_none() {
        return Err(ServiceError::NotFound(format!("Record: {}", params.id)));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<csw:GetRecordByIdResponse xmlns:csw="http://www.opengis.net/cat/csw/2.0.2">
  <csw:Record>
    <dc:identifier>{}</dc:identifier>
  </csw:Record>
</csw:GetRecordByIdResponse>"#,
        params.id
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}
