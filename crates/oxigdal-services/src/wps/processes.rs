//! WPS process description and execution

use crate::error::ServiceError;
use crate::wps::{ProcessInputs, WpsState};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

/// Parameters for DescribeProcess request
#[derive(Debug, Deserialize)]
pub struct DescribeProcessParams {
    /// Process identifier
    pub identifier: String,
}

/// Parameters for Execute request
#[derive(Debug, Deserialize)]
pub struct ExecuteParams {
    /// Process identifier
    pub identifier: String,
}

/// Handle DescribeProcess request
pub async fn handle_describe_process(
    state: &WpsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: DescribeProcessParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("identifier".to_string(), e.to_string()))?;

    let process = state
        .get_process(&params.identifier)
        .ok_or_else(|| ServiceError::NotFound(format!("Process: {}", params.identifier)))?;

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<wps:ProcessDescriptions xmlns:wps="http://www.opengis.net/wps/2.0">
  <ProcessDescription>
    <ows:Identifier>{}</ows:Identifier>
    <ows:Title>{}</ows:Title>
  </ProcessDescription>
</wps:ProcessDescriptions>"#,
        process.identifier(),
        process.title()
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Handle Execute request
pub async fn handle_execute(
    state: &WpsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: ExecuteParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("identifier".to_string(), e.to_string()))?;

    let process = state
        .get_process(&params.identifier)
        .ok_or_else(|| ServiceError::NotFound(format!("Process: {}", params.identifier)))?;

    let inputs = ProcessInputs::default();
    let _outputs = process.execute(inputs).await?;

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<wps:ExecuteResponse xmlns:wps="http://www.opengis.net/wps/2.0">
  <wps:Status>ProcessSucceeded</wps:Status>
</wps:ExecuteResponse>"#;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}
