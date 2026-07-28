//! Built-in WPS processes.
//!
//! These processes perform real vector geometry operations (via
//! `oxigeo-algorithms`) on their GeoJSON inputs and return the computed
//! geometry as GeoJSON. They are registered under the WPS Execute identifiers
//! `buffer`, `clip` and `union`.

use crate::error::{ServiceError, ServiceResult};
use crate::wps::geometry::{
    buffer_geometry, clip_geometry, first_complex, first_literal, geometry_to_bytes,
    parse_clip_operation, parse_geometry, polygons_to_geometry, union_geometries,
};
use crate::wps::{
    ComplexDataType, DataType, InputDescription, LiteralDataType, OutputDescription, OutputValue,
    Process, ProcessInputs, ProcessOutputs, WpsState,
};
use async_trait::async_trait;
use std::sync::Arc;

/// MIME type used for GeoJSON complex data.
const GEOJSON_MIME: &str = "application/geo+json";

/// Register built-in processes.
pub fn register_builtin_processes(state: &WpsState) {
    state.add_process(Arc::new(BufferProcess)).ok();
    state.add_process(Arc::new(ClipProcess)).ok();
    state.add_process(Arc::new(UnionProcess)).ok();
}

/// Build a GeoJSON complex input description.
fn geojson_input(
    identifier: &str,
    title: &str,
    min: usize,
    max: Option<usize>,
) -> InputDescription {
    InputDescription {
        identifier: identifier.to_string(),
        title: title.to_string(),
        abstract_text: None,
        data_type: DataType::Complex(ComplexDataType {
            mime_type: GEOJSON_MIME.to_string(),
            encoding: None,
            schema: None,
        }),
        min_occurs: min,
        max_occurs: max,
    }
}

/// Build the standard single GeoJSON `result` output description.
fn geojson_result_output(title: &str) -> OutputDescription {
    OutputDescription {
        identifier: "result".to_string(),
        title: title.to_string(),
        abstract_text: None,
        data_type: DataType::Complex(ComplexDataType {
            mime_type: GEOJSON_MIME.to_string(),
            encoding: None,
            schema: None,
        }),
    }
}

/// Build a `ProcessOutputs` carrying a single GeoJSON `result`.
fn geojson_result(bytes: Vec<u8>) -> ProcessOutputs {
    let outputs = ProcessOutputs::default();
    outputs
        .outputs
        .insert("result".to_string(), OutputValue::Complex(bytes));
    outputs
}

/// Buffer process — expands an input geometry by a distance.
struct BufferProcess;

#[async_trait]
impl Process for BufferProcess {
    fn identifier(&self) -> &str {
        "buffer"
    }

    fn title(&self) -> &str {
        "Buffer Geometry"
    }

    fn abstract_text(&self) -> Option<&str> {
        Some("Creates a buffer polygon around the input geometry at the given distance")
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![
            geojson_input("geometry", "Input Geometry", 1, Some(1)),
            InputDescription {
                identifier: "distance".to_string(),
                title: "Buffer Distance".to_string(),
                abstract_text: Some(
                    "Buffer distance in the units of the input CRS (may be negative for \
                     polygons/linestrings)"
                        .to_string(),
                ),
                data_type: DataType::Literal(LiteralDataType {
                    data_type: "double".to_string(),
                    allowed_values: None,
                }),
                min_occurs: 1,
                max_occurs: Some(1),
            },
        ]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![geojson_result_output("Buffered Geometry")]
    }

    async fn execute(&self, inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        let geometry_bytes = first_complex(&inputs, "geometry").ok_or_else(|| {
            ServiceError::MissingParameter("geometry (GeoJSON complex input)".to_string())
        })?;
        let distance_str = first_literal(&inputs, "distance")
            .ok_or_else(|| ServiceError::MissingParameter("distance".to_string()))?;
        let distance: f64 = distance_str.trim().parse().map_err(|_| {
            ServiceError::InvalidParameter(
                "distance".to_string(),
                format!("expected a number, got '{distance_str}'"),
            )
        })?;
        if !distance.is_finite() {
            return Err(ServiceError::InvalidParameter(
                "distance".to_string(),
                "distance must be finite".to_string(),
            ));
        }

        let geometry = parse_geometry(&geometry_bytes)?;
        let result = buffer_geometry(&geometry, distance)?;
        if result.is_empty() {
            return Err(ServiceError::ProcessExecution(
                "buffer produced an empty result".to_string(),
            ));
        }
        let out_geometry = polygons_to_geometry(&result);
        Ok(geojson_result(geometry_to_bytes(&out_geometry)?))
    }
}

/// Clip process — clips a subject polygon against a clip polygon.
struct ClipProcess;

#[async_trait]
impl Process for ClipProcess {
    fn identifier(&self) -> &str {
        "clip"
    }

    fn title(&self) -> &str {
        "Clip Geometry"
    }

    fn abstract_text(&self) -> Option<&str> {
        Some(
            "Clips a subject polygon against a clip polygon (intersection, difference, union or \
             symmetric difference)",
        )
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![
            geojson_input("geometry", "Subject Polygon", 1, Some(1)),
            geojson_input("clip", "Clip Polygon", 1, Some(1)),
            InputDescription {
                identifier: "operation".to_string(),
                title: "Boolean Operation".to_string(),
                abstract_text: Some(
                    "One of: intersection (default), difference, union, symmetricDifference"
                        .to_string(),
                ),
                data_type: DataType::Literal(LiteralDataType {
                    data_type: "string".to_string(),
                    allowed_values: Some(vec![
                        "intersection".to_string(),
                        "difference".to_string(),
                        "union".to_string(),
                        "symmetricDifference".to_string(),
                    ]),
                }),
                min_occurs: 0,
                max_occurs: Some(1),
            },
        ]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![geojson_result_output("Clipped Geometry")]
    }

    async fn execute(&self, inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        let subject_bytes = first_complex(&inputs, "geometry").ok_or_else(|| {
            ServiceError::MissingParameter("geometry (subject GeoJSON polygon)".to_string())
        })?;
        let clip_bytes = first_complex(&inputs, "clip").ok_or_else(|| {
            ServiceError::MissingParameter("clip (clip GeoJSON polygon)".to_string())
        })?;
        let operation = parse_clip_operation(first_literal(&inputs, "operation").as_deref())?;

        let subject = parse_geometry(&subject_bytes)?;
        let clip = parse_geometry(&clip_bytes)?;
        let result = clip_geometry(&subject, &clip, operation)?;
        if result.is_empty() {
            // An empty clip result is a legitimate outcome (disjoint inputs);
            // return an explicit empty GeometryCollection rather than a fake
            // success with no output.
            let empty = geojson::Geometry::new(geojson::GeometryValue::GeometryCollection {
                geometries: Vec::new(),
            });
            return Ok(geojson_result(geometry_to_bytes(&empty)?));
        }
        let out_geometry = polygons_to_geometry(&result);
        Ok(geojson_result(geometry_to_bytes(&out_geometry)?))
    }
}

/// Union process — merges two or more polygons into their geometric union.
struct UnionProcess;

#[async_trait]
impl Process for UnionProcess {
    fn identifier(&self) -> &str {
        "union"
    }

    fn title(&self) -> &str {
        "Union Geometries"
    }

    fn abstract_text(&self) -> Option<&str> {
        Some("Computes the geometric union of two or more input polygons")
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![geojson_input("geometry", "Input Polygons", 2, None)]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![geojson_result_output("Union Geometry")]
    }

    async fn execute(&self, inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        let all_bytes = crate::wps::geometry::all_complex(&inputs, "geometry");
        if all_bytes.len() < 2 {
            return Err(ServiceError::InvalidParameter(
                "geometry".to_string(),
                format!(
                    "union requires at least 2 input geometries, got {}",
                    all_bytes.len()
                ),
            ));
        }
        let geometries = all_bytes
            .iter()
            .map(|b| parse_geometry(b))
            .collect::<ServiceResult<Vec<_>>>()?;
        let result = union_geometries(&geometries)?;
        let out_geometry = polygons_to_geometry(&result);
        Ok(geojson_result(geometry_to_bytes(&out_geometry)?))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::wps::InputValue;

    fn inputs_with(entries: Vec<(&str, InputValue)>) -> ProcessInputs {
        let inputs = ProcessInputs::default();
        for (key, value) in entries {
            inputs
                .inputs
                .entry(key.to_string())
                .or_default()
                .push(value);
        }
        inputs
    }

    fn square(min: f64, max: f64) -> Vec<u8> {
        format!(
            r#"{{"type":"Polygon","coordinates":[[[{min},{min}],[{max},{min}],[{max},{max}],[{min},{max}],[{min},{min}]]]}}"#
        )
        .into_bytes()
    }

    fn extract_result(outputs: &ProcessOutputs) -> Vec<u8> {
        match outputs.outputs.get("result").map(|v| v.clone()) {
            Some(OutputValue::Complex(bytes)) => bytes,
            _ => panic!("expected complex result output"),
        }
    }

    #[tokio::test]
    async fn buffer_process_returns_real_geometry() {
        let proc = BufferProcess;
        let inputs = inputs_with(vec![
            (
                "geometry",
                InputValue::Complex(br#"{"type":"Point","coordinates":[0,0]}"#.to_vec()),
            ),
            ("distance", InputValue::Literal("5".to_string())),
        ]);
        let outputs = proc.execute(inputs).await.unwrap();
        let bytes = extract_result(&outputs);
        let geom = parse_geometry(&bytes).unwrap();
        // A buffered point is a polygon.
        assert_eq!(geom.value.type_name(), "Polygon");
    }

    #[tokio::test]
    async fn buffer_process_missing_geometry_errors() {
        let proc = BufferProcess;
        let inputs = inputs_with(vec![("distance", InputValue::Literal("5".to_string()))]);
        assert!(proc.execute(inputs).await.is_err());
    }

    #[tokio::test]
    async fn buffer_process_bad_distance_errors() {
        let proc = BufferProcess;
        let inputs = inputs_with(vec![
            (
                "geometry",
                InputValue::Complex(br#"{"type":"Point","coordinates":[0,0]}"#.to_vec()),
            ),
            ("distance", InputValue::Literal("abc".to_string())),
        ]);
        assert!(proc.execute(inputs).await.is_err());
    }

    #[tokio::test]
    async fn clip_process_intersects() {
        let proc = ClipProcess;
        let inputs = inputs_with(vec![
            ("geometry", InputValue::Complex(square(0.0, 10.0))),
            ("clip", InputValue::Complex(square(5.0, 15.0))),
        ]);
        let outputs = proc.execute(inputs).await.unwrap();
        let bytes = extract_result(&outputs);
        let geom = parse_geometry(&bytes).unwrap();
        assert!(matches!(geom.value.type_name(), "Polygon" | "MultiPolygon"));
    }

    #[tokio::test]
    async fn union_process_requires_two_inputs() {
        let proc = UnionProcess;
        let inputs = inputs_with(vec![("geometry", InputValue::Complex(square(0.0, 10.0)))]);
        assert!(proc.execute(inputs).await.is_err());
    }

    #[tokio::test]
    async fn union_process_merges_two_polygons() {
        let proc = UnionProcess;
        let inputs = inputs_with(vec![
            ("geometry", InputValue::Complex(square(0.0, 10.0))),
            ("geometry", InputValue::Complex(square(5.0, 15.0))),
        ]);
        let outputs = proc.execute(inputs).await.unwrap();
        let bytes = extract_result(&outputs);
        assert!(parse_geometry(&bytes).is_ok());
    }
}
