//! WPS process description and execution.

use crate::error::ServiceError;
use crate::wps::{InputValue, OutputValue, ProcessInputs, WpsState};
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

/// Request/response control keys that are not process inputs.
const CONTROL_KEYS: &[&str] = &[
    "identifier",
    "service",
    "version",
    "request",
    "response",
    "mode",
    "lineage",
    "status",
    "storeexecuteresponse",
    "rawdataoutput",
    "responsedocument",
    "inputs",
    "outputs",
];

/// Handle DescribeProcess request.
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

    let mut inputs_xml = String::new();
    for input in process.inputs() {
        inputs_xml.push_str(&format!(
            "    <wps:Input minOccurs=\"{}\" maxOccurs=\"{}\">\n      \
             <ows:Identifier>{}</ows:Identifier>\n      <ows:Title>{}</ows:Title>\n    \
             </wps:Input>\n",
            input.min_occurs,
            input
                .max_occurs
                .map(|m| m.to_string())
                .unwrap_or_else(|| "unbounded".to_string()),
            xml_escape(&input.identifier),
            xml_escape(&input.title),
        ));
    }
    let mut outputs_xml = String::new();
    for output in process.outputs() {
        outputs_xml.push_str(&format!(
            "    <wps:Output>\n      <ows:Identifier>{}</ows:Identifier>\n      \
             <ows:Title>{}</ows:Title>\n    </wps:Output>\n",
            xml_escape(&output.identifier),
            xml_escape(&output.title),
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<wps:ProcessDescriptions xmlns:wps="http://www.opengis.net/wps/2.0" xmlns:ows="http://www.opengis.net/ows/2.0">
  <ProcessDescription>
    <ows:Identifier>{}</ows:Identifier>
    <ows:Title>{}</ows:Title>
{}{}  </ProcessDescription>
</wps:ProcessDescriptions>"#,
        xml_escape(process.identifier()),
        xml_escape(process.title()),
        inputs_xml,
        outputs_xml,
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Handle Execute request.
///
/// Inputs are parsed from the request parameters (both the JSON `inputs` object
/// form and the flat KVP form are supported), the selected process performs its
/// real computation, and the resulting outputs are serialized into the WPS
/// ExecuteResponse.
pub async fn handle_execute(
    state: &WpsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let exec_params: ExecuteParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("identifier".to_string(), e.to_string()))?;

    let process = state
        .get_process(&exec_params.identifier)
        .ok_or_else(|| ServiceError::NotFound(format!("Process: {}", exec_params.identifier)))?;

    let inputs = build_process_inputs(params)?;
    let outputs = process.execute(inputs).await?;

    let mut outputs_xml = String::new();
    for entry in outputs.outputs.iter() {
        let identifier = entry.key();
        let value = entry.value();
        let data_xml = match value {
            OutputValue::Literal(s) => {
                format!("      <wps:Data>{}</wps:Data>\n", xml_escape(s))
            }
            OutputValue::Reference(url) => {
                format!(
                    "      <wps:Reference xlink:href=\"{}\"/>\n",
                    xml_escape(url)
                )
            }
            OutputValue::Complex(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                // Wrap in CDATA so embedded JSON braces/quotes need no escaping.
                format!(
                    "      <wps:Data mimeType=\"application/geo+json\"><![CDATA[{}]]></wps:Data>\n",
                    text
                )
            }
        };
        outputs_xml.push_str(&format!(
            "    <wps:Output id=\"{}\">\n{}    </wps:Output>\n",
            xml_escape(identifier),
            data_xml,
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<wps:Result xmlns:wps="http://www.opengis.net/wps/2.0" xmlns:xlink="http://www.w3.org/1999/xlink">
{}</wps:Result>"#,
        outputs_xml,
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Build a [`ProcessInputs`] from the request parameters.
///
/// Two encodings are accepted:
///
/// - JSON: a top-level `inputs` object mapping identifiers to values, where a
///   value may be a GeoJSON object (complex), an array (repeated input), or a
///   scalar (literal).
/// - KVP: any top-level parameter that is not a control key becomes an input;
///   string values that look like JSON (`{`/`[`) are treated as complex data.
fn build_process_inputs(params: &serde_json::Value) -> Result<ProcessInputs, ServiceError> {
    let inputs = ProcessInputs::default();

    if let Some(serde_json::Value::Object(map)) = params.get("inputs") {
        for (key, value) in map {
            append_input_values(&inputs, key, value);
        }
        return Ok(inputs);
    }

    if let Some(obj) = params.as_object() {
        for (key, value) in obj {
            if CONTROL_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                continue;
            }
            append_input_values(&inputs, key, value);
        }
    }

    Ok(inputs)
}

/// Append one or more [`InputValue`]s for `key` derived from a JSON value.
fn append_input_values(inputs: &ProcessInputs, key: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                append_single_value(inputs, key, item);
            }
        }
        other => append_single_value(inputs, key, other),
    }
}

fn append_single_value(inputs: &ProcessInputs, key: &str, value: &serde_json::Value) {
    let input_value = match value {
        // A JSON object is complex data (e.g. a GeoJSON geometry/feature).
        serde_json::Value::Object(_) => {
            InputValue::Complex(serde_json::to_vec(value).unwrap_or_default())
        }
        serde_json::Value::String(s) => {
            let trimmed = s.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                InputValue::Complex(s.clone().into_bytes())
            } else {
                InputValue::Literal(s.clone())
            }
        }
        serde_json::Value::Number(n) => InputValue::Literal(n.to_string()),
        serde_json::Value::Bool(b) => InputValue::Literal(b.to_string()),
        serde_json::Value::Null => return,
        serde_json::Value::Array(_) => {
            // Nested arrays are flattened one level by append_input_values.
            InputValue::Complex(serde_json::to_vec(value).unwrap_or_default())
        }
    };
    inputs
        .inputs
        .entry(key.to_string())
        .or_default()
        .push(input_value);
}

/// Escape a string for inclusion in XML text/attribute content.
fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::wps::{ServiceInfo, WpsState};

    fn test_state() -> WpsState {
        WpsState::new(ServiceInfo {
            title: "Test WPS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wps".to_string(),
            versions: vec!["2.0.0".to_string()],
        })
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn build_inputs_json_form() {
        let params = serde_json::json!({
            "identifier": "buffer",
            "inputs": {
                "geometry": {"type": "Point", "coordinates": [0, 0]},
                "distance": 5
            }
        });
        let inputs = build_process_inputs(&params).unwrap();
        assert!(inputs.inputs.get("geometry").is_some());
        assert!(inputs.inputs.get("distance").is_some());
        match inputs.inputs.get("distance").unwrap().first().unwrap() {
            InputValue::Literal(s) => assert_eq!(s, "5"),
            _ => panic!("expected literal distance"),
        }
    }

    #[test]
    fn build_inputs_repeated_kvp_array() {
        let params = serde_json::json!({
            "identifier": "union",
            "inputs": {
                "geometry": [
                    {"type": "Polygon", "coordinates": []},
                    {"type": "Polygon", "coordinates": []}
                ]
            }
        });
        let inputs = build_process_inputs(&params).unwrap();
        assert_eq!(inputs.inputs.get("geometry").unwrap().len(), 2);
    }

    #[tokio::test]
    async fn execute_buffer_via_handler() {
        let state = test_state();
        let params = serde_json::json!({
            "identifier": "buffer",
            "inputs": {
                "geometry": {"type": "Point", "coordinates": [0, 0]},
                "distance": 5
            }
        });
        let resp = handle_execute(&state, "2.0.0", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("wps:Result"));
        assert!(body.contains("Polygon"));
        assert!(body.contains("id=\"result\""));
    }

    #[tokio::test]
    async fn execute_unknown_process_errors() {
        let state = test_state();
        let params = serde_json::json!({ "identifier": "nonexistent" });
        assert!(handle_execute(&state, "2.0.0", &params).await.is_err());
    }

    #[tokio::test]
    async fn describe_process_lists_real_io() {
        let state = test_state();
        let params = serde_json::json!({ "identifier": "buffer" });
        let resp = handle_describe_process(&state, "2.0.0", &params)
            .await
            .unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("geometry"));
        assert!(body.contains("distance"));
        assert!(body.contains("result"));
    }
}
