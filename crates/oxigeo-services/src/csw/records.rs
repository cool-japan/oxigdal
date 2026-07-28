//! CSW record retrieval and search

use crate::csw::{CswState, MetadataRecord};
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

/// Escape a string for safe inclusion in XML character data / attributes.
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

/// Look up a query parameter case-insensitively from the flattened KVP object.
///
/// CSW clients are inconsistent about parameter casing (`maxRecords`,
/// `maxrecords`, `MAXRECORDS`), so we normalise on lookup.
fn param_ci<'a>(params: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let obj = params.as_object()?;
    let key_lower = key.to_ascii_lowercase();
    obj.iter()
        .find(|(k, _)| k.to_ascii_lowercase() == key_lower)
        .map(|(_, v)| v)
}

/// Extract a string-valued parameter (KVP values arrive as JSON strings).
fn param_str(params: &serde_json::Value, key: &str) -> Option<String> {
    match param_ci(params, key)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Extract a `usize` parameter, tolerating both JSON numbers and strings.
fn param_usize(params: &serde_json::Value, key: &str) -> Option<usize> {
    match param_ci(params, key)? {
        serde_json::Value::Number(n) => n.as_u64().map(|v| v as usize),
        serde_json::Value::String(s) => s.trim().parse::<usize>().ok(),
        _ => None,
    }
}

/// Build the JSON queryable view of a metadata record used for CQL evaluation.
///
/// Both the canonical Dublin Core queryable names and their lower-case forms
/// are provided, plus the `AnyText` full-text pseudo-queryable (concatenation
/// of every searchable text field) as required by the CSW `csw:AnyText` core
/// queryable.
fn record_queryables(record: &MetadataRecord) -> serde_json::Value {
    let keywords_joined = record.keywords.join(" ");
    let abstract_text = record.abstract_text.clone().unwrap_or_default();
    let any_text = format!(
        "{} {} {} {}",
        record.identifier, record.title, abstract_text, keywords_joined
    );

    let mut map = serde_json::Map::new();
    let mut insert = |k: &str, v: &str| {
        map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    };
    insert("identifier", &record.identifier);
    insert("Identifier", &record.identifier);
    insert("title", &record.title);
    insert("Title", &record.title);
    insert("abstract", &abstract_text);
    insert("Abstract", &abstract_text);
    insert("subject", &keywords_joined);
    insert("Subject", &keywords_joined);
    insert("keywords", &keywords_joined);
    insert("AnyText", &any_text);
    insert("anytext", &any_text);

    serde_json::Value::Object(map)
}

/// Serialize a single metadata record as a `csw:Record` (Dublin Core) element.
fn record_to_xml(record: &MetadataRecord) -> String {
    let mut xml = String::new();
    xml.push_str("    <csw:Record>\n");
    xml.push_str(&format!(
        "      <dc:identifier>{}</dc:identifier>\n",
        xml_escape(&record.identifier)
    ));
    xml.push_str(&format!(
        "      <dc:title>{}</dc:title>\n",
        xml_escape(&record.title)
    ));
    if let Some(ref abstract_text) = record.abstract_text {
        xml.push_str(&format!(
            "      <dct:abstract>{}</dct:abstract>\n",
            xml_escape(abstract_text)
        ));
    }
    for keyword in &record.keywords {
        xml.push_str(&format!(
            "      <dc:subject>{}</dc:subject>\n",
            xml_escape(keyword)
        ));
    }
    if let Some((min_x, min_y, max_x, max_y)) = record.bbox {
        xml.push_str(
            "      <ows:BoundingBox crs=\"urn:ogc:def:crs:EPSG::4326\" dimensions=\"2\">\n",
        );
        xml.push_str(&format!(
            "        <ows:LowerCorner>{} {}</ows:LowerCorner>\n",
            min_x, min_y
        ));
        xml.push_str(&format!(
            "        <ows:UpperCorner>{} {}</ows:UpperCorner>\n",
            max_x, max_y
        ));
        xml.push_str("      </ows:BoundingBox>\n");
    }
    xml.push_str("    </csw:Record>\n");
    xml
}

/// Handle GetRecords request.
///
/// Performs a real search over the in-memory catalog (`state.records`):
///
/// - `constraint` / `cql_text` is parsed with the shared CQL parser and
///   evaluated against each record's Dublin Core queryables (`title`,
///   `abstract`, `subject`, `AnyText`, ...). An unparseable constraint fails
///   closed with an `InvalidParameter` exception rather than matching nothing
///   or everything.
/// - `startPosition` (1-based) and `maxRecords` paginate the sorted result set.
/// - `resultType=hits` returns counts only; `results` returns the record bodies.
pub async fn handle_get_records(
    state: &CswState,
    version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let max_records = param_usize(params, "maxRecords").unwrap_or(10);
    let start_position = param_usize(params, "startPosition").unwrap_or(1).max(1);
    let result_type = param_str(params, "resultType").unwrap_or_else(|| "results".to_string());
    let hits_only = result_type.eq_ignore_ascii_case("hits");

    // The constraint may arrive under several parameter names depending on the
    // client / CSW version. `constraintLanguage` is honoured only for CQL_TEXT;
    // an unsupported filter language is rejected rather than silently ignored.
    let constraint = param_str(params, "constraint")
        .or_else(|| param_str(params, "cql_text"))
        .or_else(|| param_str(params, "constraint_cql_text"));

    if let Some(lang) = param_str(params, "constraintLanguage")
        && !lang.eq_ignore_ascii_case("CQL_TEXT")
        && !lang.eq_ignore_ascii_case("CQL")
    {
        return Err(ServiceError::InvalidParameter(
            "constraintLanguage".to_string(),
            format!("unsupported constraint language '{lang}' (only CQL_TEXT is supported)"),
        ));
    }

    // Pre-compile the constraint so a malformed filter is reported before any
    // record is inspected (fail closed).
    let compiled = match &constraint {
        Some(text) if !text.trim().is_empty() => {
            Some(crate::ogc_features::CqlParser::parse(text).map_err(|e| {
                ServiceError::InvalidParameter(
                    "constraint".to_string(),
                    format!("unparseable CQL constraint '{text}': {e}"),
                )
            })?)
        }
        _ => None,
    };

    // Collect and deterministically order matching records.
    let mut matched: Vec<MetadataRecord> = state
        .records
        .iter()
        .filter(|entry| match &compiled {
            None => true,
            Some(expr) => {
                let queryables = record_queryables(entry.value());
                crate::ogc_features::CqlParser::evaluate(expr, &queryables)
            }
        })
        .map(|entry| entry.value().clone())
        .collect();
    matched.sort_by(|a, b| a.identifier.cmp(&b.identifier));

    let number_matched = matched.len();

    // Page window: startPosition is 1-based.
    let start_index = start_position - 1;
    let page: Vec<&MetadataRecord> = if hits_only || start_index >= matched.len() {
        Vec::new()
    } else {
        matched.iter().skip(start_index).take(max_records).collect()
    };
    let number_returned = page.len();

    // nextRecord is the 1-based position of the record following this page, or
    // 0 when the page reaches the end of the result set (per CSW 2.0.2).
    let next_record = if hits_only {
        0
    } else {
        let consumed = start_index + number_returned;
        if consumed < number_matched {
            consumed + 1
        } else {
            0
        }
    };

    let timestamp = chrono::Utc::now().to_rfc3339();

    let mut records_xml = String::new();
    for record in &page {
        records_xml.push_str(&record_to_xml(record));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<csw:GetRecordsResponse xmlns:csw="http://www.opengis.net/cat/csw/2.0.2"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:dct="http://purl.org/dc/terms/"
    xmlns:ows="http://www.opengis.net/ows"
    version="{version}">
  <csw:SearchStatus timestamp="{timestamp}"/>
  <csw:SearchResults numberOfRecordsMatched="{number_matched}" numberOfRecordsReturned="{number_returned}" nextRecord="{next_record}" elementSet="full">
{records_xml}  </csw:SearchResults>
</csw:GetRecordsResponse>"#,
    );

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

    let record = state
        .records
        .get(&params.id)
        .ok_or_else(|| ServiceError::NotFound(format!("Record: {}", params.id)))?;

    let body = record_to_xml(record.value());

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<csw:GetRecordByIdResponse xmlns:csw="http://www.opengis.net/cat/csw/2.0.2"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:dct="http://purl.org/dc/terms/"
    xmlns:ows="http://www.opengis.net/ows">
{body}</csw:GetRecordByIdResponse>"#,
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::csw::{CswState, MetadataRecord, ServiceInfo};

    fn test_state() -> CswState {
        let state = CswState::new(ServiceInfo {
            title: "Test CSW".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/csw".to_string(),
            versions: vec!["2.0.2".to_string()],
        });
        state
            .add_record(MetadataRecord {
                identifier: "rec.tokyo".to_string(),
                title: "Tokyo Land Cover".to_string(),
                abstract_text: Some("Land cover dataset for Tokyo metropolitan area".to_string()),
                keywords: vec!["landcover".to_string(), "japan".to_string()],
                bbox: Some((139.0, 35.0, 140.0, 36.0)),
            })
            .unwrap();
        state
            .add_record(MetadataRecord {
                identifier: "rec.osaka".to_string(),
                title: "Osaka Elevation".to_string(),
                abstract_text: Some("Digital elevation model for Osaka".to_string()),
                keywords: vec!["elevation".to_string(), "japan".to_string()],
                bbox: None,
            })
            .unwrap();
        state
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn get_records_returns_all_when_unfiltered() {
        let state = test_state();
        let params = serde_json::json!({});
        let resp = handle_get_records(&state, "2.0.2", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("numberOfRecordsMatched=\"2\""));
        assert!(body.contains("numberOfRecordsReturned=\"2\""));
        assert!(body.contains("rec.osaka"));
        assert!(body.contains("rec.tokyo"));
    }

    #[tokio::test]
    async fn get_records_applies_cql_constraint() {
        let state = test_state();
        let params = serde_json::json!({ "constraint": "title LIKE '%Tokyo%'" });
        let resp = handle_get_records(&state, "2.0.2", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("numberOfRecordsMatched=\"1\""));
        assert!(body.contains("rec.tokyo"));
        assert!(!body.contains("rec.osaka"));
    }

    #[tokio::test]
    async fn get_records_anytext_matches_abstract() {
        let state = test_state();
        let params = serde_json::json!({ "constraint": "AnyText LIKE '%elevation%'" });
        let resp = handle_get_records(&state, "2.0.2", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("numberOfRecordsMatched=\"1\""));
        assert!(body.contains("rec.osaka"));
    }

    #[tokio::test]
    async fn get_records_hits_returns_count_only() {
        let state = test_state();
        let params = serde_json::json!({ "resultType": "hits" });
        let resp = handle_get_records(&state, "2.0.2", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("numberOfRecordsMatched=\"2\""));
        assert!(body.contains("numberOfRecordsReturned=\"0\""));
        assert!(!body.contains("<csw:Record>"));
    }

    #[tokio::test]
    async fn get_records_paginates() {
        let state = test_state();
        let params = serde_json::json!({ "maxRecords": "1", "startPosition": "1" });
        let resp = handle_get_records(&state, "2.0.2", &params).await.unwrap();
        let body = body_of(resp).await;
        assert!(body.contains("numberOfRecordsMatched=\"2\""));
        assert!(body.contains("numberOfRecordsReturned=\"1\""));
        // Sorted by identifier: rec.osaka comes first.
        assert!(body.contains("rec.osaka"));
        assert!(body.contains("nextRecord=\"2\""));
    }

    #[tokio::test]
    async fn get_records_rejects_unparseable_constraint() {
        let state = test_state();
        let params = serde_json::json!({ "constraint": "this is not valid cql !!!" });
        let err = handle_get_records(&state, "2.0.2", &params)
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidParameter(_, _)));
    }

    #[tokio::test]
    async fn get_records_rejects_unsupported_constraint_language() {
        let state = test_state();
        let params = serde_json::json!({
            "constraint": "title = 'x'",
            "constraintLanguage": "FILTER",
        });
        let err = handle_get_records(&state, "2.0.2", &params)
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::InvalidParameter(_, _)));
    }
}
