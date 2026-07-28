//! Geometry conversion and processing helpers for built-in WPS processes.
//!
//! Bridges GeoJSON (the WPS complex-data interchange format used by the
//! built-in processes) and the `oxigeo-algorithms` vector geometry types, and
//! runs the real buffer / clip / union operations.

use crate::error::{ServiceError, ServiceResult};
use crate::wps::{InputValue, ProcessInputs};
use geojson::{Geometry, GeometryValue};
use oxigeo_algorithms::{
    BufferOptions, ClipOperation, Coordinate, LineString, Point, Polygon, buffer_linestring,
    buffer_point, buffer_polygon, clip_polygons, union_polygons,
};

/// Convert an internal algorithm error into a WPS process-execution error.
fn algo_err(err: impl std::fmt::Display) -> ServiceError {
    ServiceError::ProcessExecution(err.to_string())
}

/// Retrieve the first literal value for an input identifier.
pub fn first_literal(inputs: &ProcessInputs, id: &str) -> Option<String> {
    inputs.inputs.get(id).and_then(|values| {
        values.iter().find_map(|v| match v {
            InputValue::Literal(s) => Some(s.clone()),
            _ => None,
        })
    })
}

/// Retrieve every complex (byte) payload for an input identifier, in order.
pub fn all_complex(inputs: &ProcessInputs, id: &str) -> Vec<Vec<u8>> {
    inputs
        .inputs
        .get(id)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| match v {
                    InputValue::Complex(bytes) => Some(bytes.clone()),
                    InputValue::Literal(s) => Some(s.clone().into_bytes()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Retrieve the first complex payload for an input identifier.
pub fn first_complex(inputs: &ProcessInputs, id: &str) -> Option<Vec<u8>> {
    all_complex(inputs, id).into_iter().next()
}

/// Parse a GeoJSON geometry from raw bytes.
///
/// Accepts a bare `Geometry`, a `Feature` (its geometry is used), or a
/// single-feature `FeatureCollection`.
pub fn parse_geometry(bytes: &[u8]) -> ServiceResult<Geometry> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| ServiceError::InvalidGeoJson(format!("input is not valid UTF-8: {e}")))?;
    let gj: geojson::GeoJson = text
        .parse()
        .map_err(|e| ServiceError::InvalidGeoJson(format!("{e}")))?;
    match gj {
        geojson::GeoJson::Geometry(g) => Ok(g),
        geojson::GeoJson::Feature(f) => f
            .geometry
            .ok_or_else(|| ServiceError::InvalidGeoJson("feature has no geometry".to_string())),
        geojson::GeoJson::FeatureCollection(fc) => fc
            .features
            .into_iter()
            .find_map(|f| f.geometry)
            .ok_or_else(|| {
                ServiceError::InvalidGeoJson(
                    "feature collection has no geometry to process".to_string(),
                )
            }),
    }
}

fn position_to_coord(pos: &geojson::Position) -> ServiceResult<Coordinate> {
    let slice = pos.as_slice();
    if slice.len() < 2 {
        return Err(ServiceError::InvalidGeoJson(
            "position must have at least 2 ordinates".to_string(),
        ));
    }
    Ok(Coordinate::new_2d(slice[0], slice[1]))
}

fn ring_to_linestring(ring: &[geojson::Position]) -> ServiceResult<LineString> {
    let coords = ring
        .iter()
        .map(position_to_coord)
        .collect::<ServiceResult<Vec<_>>>()?;
    LineString::new(coords).map_err(algo_err)
}

/// Convert a GeoJSON polygon ring set into an `oxigeo` [`Polygon`].
fn geojson_rings_to_polygon(rings: &[Vec<geojson::Position>]) -> ServiceResult<Polygon> {
    let exterior_ring = rings.first().ok_or_else(|| {
        ServiceError::InvalidGeoJson("polygon must have an exterior ring".to_string())
    })?;
    let exterior = ring_to_linestring(exterior_ring)?;
    let interiors = rings
        .iter()
        .skip(1)
        .map(|r| ring_to_linestring(r))
        .collect::<ServiceResult<Vec<_>>>()?;
    Polygon::new(exterior, interiors).map_err(algo_err)
}

/// Extract all polygons from a GeoJSON geometry (Polygon or MultiPolygon).
///
/// Returns an error for geometry types that are not areal.
pub fn geometry_to_polygons(geometry: &Geometry) -> ServiceResult<Vec<Polygon>> {
    match &geometry.value {
        GeometryValue::Polygon { coordinates } => Ok(vec![geojson_rings_to_polygon(coordinates)?]),
        GeometryValue::MultiPolygon { coordinates } => coordinates
            .iter()
            .map(|rings| geojson_rings_to_polygon(rings))
            .collect(),
        other => Err(ServiceError::InvalidParameter(
            "geometry".to_string(),
            format!(
                "expected Polygon or MultiPolygon, got {}",
                other.type_name()
            ),
        )),
    }
}

fn linestring_to_positions(ring: &LineString) -> Vec<geojson::Position> {
    ring.coords
        .iter()
        .map(|c| geojson::Position::from([c.x, c.y]))
        .collect()
}

fn polygon_to_geojson_rings(polygon: &Polygon) -> Vec<Vec<geojson::Position>> {
    let mut rings = Vec::with_capacity(1 + polygon.interiors.len());
    rings.push(linestring_to_positions(&polygon.exterior));
    for hole in &polygon.interiors {
        rings.push(linestring_to_positions(hole));
    }
    rings
}

/// Serialize a set of result polygons back into a GeoJSON geometry.
///
/// A single polygon becomes a `Polygon`; multiple polygons become a
/// `MultiPolygon`.
pub fn polygons_to_geometry(polygons: &[Polygon]) -> Geometry {
    if polygons.len() == 1 {
        Geometry::new(GeometryValue::Polygon {
            coordinates: polygon_to_geojson_rings(&polygons[0]),
        })
    } else {
        Geometry::new(GeometryValue::MultiPolygon {
            coordinates: polygons.iter().map(polygon_to_geojson_rings).collect(),
        })
    }
}

/// Serialize a GeoJSON geometry to its byte representation.
pub fn geometry_to_bytes(geometry: &Geometry) -> ServiceResult<Vec<u8>> {
    serde_json::to_vec(geometry).map_err(|e| ServiceError::Serialization(e.to_string()))
}

/// Perform a real buffer of a GeoJSON geometry by `distance`.
///
/// Points, linestrings and polygons are all supported; a MultiPolygon buffers
/// each constituent polygon. The result is one or more polygons.
pub fn buffer_geometry(geometry: &Geometry, distance: f64) -> ServiceResult<Vec<Polygon>> {
    let options = BufferOptions::default();
    match &geometry.value {
        GeometryValue::Point { coordinates } => {
            let coord = position_to_coord(coordinates)?;
            let point = Point::new(coord.x, coord.y);
            Ok(vec![
                buffer_point(&point, distance, &options).map_err(algo_err)?,
            ])
        }
        GeometryValue::MultiPoint { coordinates } => coordinates
            .iter()
            .map(|c| {
                let coord = position_to_coord(c)?;
                let point = Point::new(coord.x, coord.y);
                buffer_point(&point, distance, &options).map_err(algo_err)
            })
            .collect(),
        GeometryValue::LineString { coordinates } => {
            let line = ring_to_linestring(coordinates)?;
            Ok(vec![
                buffer_linestring(&line, distance, &options).map_err(algo_err)?,
            ])
        }
        GeometryValue::MultiLineString { coordinates } => coordinates
            .iter()
            .map(|ls| {
                let line = ring_to_linestring(ls)?;
                buffer_linestring(&line, distance, &options).map_err(algo_err)
            })
            .collect(),
        GeometryValue::Polygon { .. } | GeometryValue::MultiPolygon { .. } => {
            let polygons = geometry_to_polygons(geometry)?;
            polygons
                .iter()
                .map(|p| buffer_polygon(p, distance, &options).map_err(algo_err))
                .collect()
        }
        other => Err(ServiceError::InvalidParameter(
            "geometry".to_string(),
            format!("cannot buffer geometry of type {}", other.type_name()),
        )),
    }
}

/// Parse a [`ClipOperation`] from an optional literal operation name.
pub fn parse_clip_operation(name: Option<&str>) -> ServiceResult<ClipOperation> {
    match name.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("intersection") | Some("intersect") => Ok(ClipOperation::Intersection),
        Some("difference") => Ok(ClipOperation::Difference),
        Some("union") => Ok(ClipOperation::Union),
        Some("symmetricdifference") | Some("symdifference") | Some("xor") => {
            Ok(ClipOperation::SymmetricDifference)
        }
        Some(other) => Err(ServiceError::InvalidParameter(
            "operation".to_string(),
            format!("unknown clip operation '{other}'"),
        )),
    }
}

/// Clip `subject` polygons against `clip` polygons with the given operation.
///
/// Every subject polygon is clipped against every clip polygon and the results
/// are concatenated.
pub fn clip_geometry(
    subject: &Geometry,
    clip: &Geometry,
    op: ClipOperation,
) -> ServiceResult<Vec<Polygon>> {
    let subjects = geometry_to_polygons(subject)?;
    let clips = geometry_to_polygons(clip)?;
    let mut results = Vec::new();
    for s in &subjects {
        for c in &clips {
            results.extend(clip_polygons(s, c, op).map_err(algo_err)?);
        }
    }
    Ok(results)
}

/// Union all polygons contained in the supplied geometries.
pub fn union_geometries(geometries: &[Geometry]) -> ServiceResult<Vec<Polygon>> {
    let mut polygons = Vec::new();
    for g in geometries {
        polygons.extend(geometry_to_polygons(g)?);
    }
    if polygons.is_empty() {
        return Err(ServiceError::ProcessExecution(
            "union requires at least one input polygon".to_string(),
        ));
    }
    union_polygons(&polygons).map_err(algo_err)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn point_geojson(x: f64, y: f64) -> Vec<u8> {
        format!(r#"{{"type":"Point","coordinates":[{x},{y}]}}"#).into_bytes()
    }

    fn square_geojson(min: f64, max: f64) -> Vec<u8> {
        format!(
            r#"{{"type":"Polygon","coordinates":[[[{min},{min}],[{max},{min}],[{max},{max}],[{min},{max}],[{min},{min}]]]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn buffer_point_produces_polygon_of_expected_area() {
        let geom = parse_geometry(&point_geojson(0.0, 0.0)).unwrap();
        let result = buffer_geometry(&geom, 10.0).unwrap();
        assert_eq!(result.len(), 1);
        // Area of the buffered disc should approach pi * r^2 = ~314.
        let area =
            oxigeo_algorithms::area_polygon(&result[0], oxigeo_algorithms::AreaMethod::Planar)
                .unwrap();
        assert!(area > 300.0 && area < 315.0, "buffer area was {area}");
    }

    #[test]
    fn buffer_rejects_empty_output() {
        let geom = parse_geometry(&square_geojson(0.0, 10.0)).unwrap();
        let result = buffer_geometry(&geom, 1.0).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn clip_intersection_of_overlapping_squares() {
        let a = parse_geometry(&square_geojson(0.0, 10.0)).unwrap();
        let b = parse_geometry(&square_geojson(5.0, 15.0)).unwrap();
        let result = clip_geometry(&a, &b, ClipOperation::Intersection).unwrap();
        assert!(!result.is_empty());
        let area =
            oxigeo_algorithms::area_polygon(&result[0], oxigeo_algorithms::AreaMethod::Planar)
                .unwrap();
        // Overlap is a 5x5 square = 25.
        assert!((area - 25.0).abs() < 1.0, "intersection area was {area}");
    }

    #[test]
    fn union_of_two_squares_is_non_empty() {
        let a = parse_geometry(&square_geojson(0.0, 10.0)).unwrap();
        let b = parse_geometry(&square_geojson(5.0, 15.0)).unwrap();
        let result = union_geometries(&[a, b]).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn parse_rejects_non_geojson() {
        assert!(parse_geometry(b"not json").is_err());
    }

    #[test]
    fn roundtrip_polygon_geometry() {
        let geom = parse_geometry(&square_geojson(0.0, 10.0)).unwrap();
        let polygons = geometry_to_polygons(&geom).unwrap();
        let back = polygons_to_geometry(&polygons);
        let bytes = geometry_to_bytes(&back).unwrap();
        assert!(!bytes.is_empty());
        // Re-parse to confirm it is valid GeoJSON.
        assert!(parse_geometry(&bytes).is_ok());
    }
}
