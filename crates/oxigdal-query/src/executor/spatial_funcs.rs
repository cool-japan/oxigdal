//! Spatial (`ST_*`) function evaluator.
//!
//! This module closes the eval gap between the query planner and the row-based
//! filter interpreter. The parser turns SQL like
//!
//! ```sql
//! WHERE ST_Intersects(geom, 'POLYGON((...))')
//! ```
//!
//! into an [`Expr::Function`](crate::parser::ast::Expr::Function) node. The
//! `Filter` operator now forwards every
//! `Expr::Function` to [`evaluate_spatial_function`], which dispatches on the
//! function name (case-insensitive) and returns either a typed
//! [`Value`] or a typed
//! [`QueryError`].
//!
//! # Value-variant rationale
//!
//! The runtime
//! [`Value`] enum lives in
//! `crates/oxigdal-query/src/executor/filter.rs` (it is the row-level value
//! type used by the row-based interpreter). A grep of `Value::` across
//! `crates/oxigdal-query/src` confirmed that *every* `match` on `Value` uses a
//! `_ => …` catch-all (binary/unary operator dispatch, comparison fall-through,
//! filter column extraction, etc.). Adding a new variant is therefore safe and
//! does not regress any existing call site.
//!
//! Following that observation we chose **`Value::Geometry(geo::Geometry<f64>)`**
//! over the alternative `Value::Wkt(String)`. Reasons:
//!
//! * Set-operation and constructor functions (`ST_Centroid`, `ST_Envelope`,
//!   `ST_Buffer`, `ST_Intersection`, …) naturally produce a typed
//!   `geo::Geometry<f64>`; round-tripping through a `String` would waste cycles
//!   and lose precision via float-to-text conversion.
//! * Downstream predicate evaluators (`ST_Intersects`, `ST_Contains`, …) keep
//!   the geometry in its parsed form, so they avoid re-parsing WKT on every
//!   row.
//! * The variant is constructed by this module only; literal WKT strings
//!   embedded in SQL stay as [`Value::String`] until parsed inside
//!   `parse_geometry_arg`, so the SQL parser/optimizer surface is unchanged.
//!
//! # Function dispatch
//!
//! Names match case-insensitively (the SQL parser preserves the original
//! casing). Supported operations:
//!
//! | Name                      | Arity | Result kind        |
//! |---------------------------|-------|--------------------|
//! | `ST_Intersects(a, b)`     | 2     | `Value::Boolean`   |
//! | `ST_Contains(a, b)`       | 2     | `Value::Boolean`   |
//! | `ST_Within(a, b)`         | 2     | `Value::Boolean`   |
//! | `ST_Disjoint(a, b)`       | 2     | `Value::Boolean`   |
//! | `ST_Equals(a, b)`         | 2     | `Value::Boolean`   |
//! | `ST_Touches(a, b)`        | 2     | `Value::Boolean`   |
//! | `ST_Overlaps(a, b)`       | 2     | `Value::Boolean`   |
//! | `ST_Crosses(a, b)`        | 2     | `Value::Boolean`   |
//! | `ST_Covers(a, b)`         | 2     | `Value::Boolean`   |
//! | `ST_CoveredBy(a, b)`      | 2     | `Value::Boolean`   |
//! | `ST_DWithin(a, b, dist)`  | 3     | `Value::Boolean`   |
//! | `ST_Distance(a, b)`       | 2     | `Value::Float64`   |
//! | `ST_Area(g)`              | 1     | `Value::Float64`   |
//! | `ST_Length(g)`            | 1     | `Value::Float64`   |
//! | `ST_Centroid(g)`          | 1     | `Value::Geometry`  |
//! | `ST_Envelope(g)`          | 1     | `Value::Geometry`  |
//! | `ST_Buffer(g, dist)`      | 2     | `Value::Geometry`  |
//! | `ST_Intersection(a, b)`   | 2     | `Value::Geometry`  |
//! | `ST_Union(a, b)`          | 2     | `Value::Geometry`  |
//! | `ST_Difference(a, b)`     | 2     | `Value::Geometry`  |
//!
//! Unsupported geometry combinations (e.g. `BooleanOps` on a non-`Polygon`)
//! return a typed [`QueryError::Unsupported`] rather than panicking.

use crate::error::{QueryError, Result};
use crate::executor::filter::Value;

use geo::algorithm::{
    area::Area, bool_ops::BooleanOps, bounding_rect::BoundingRect, buffer::Buffer,
    centroid::Centroid, contains::Contains,
};
use geo::algorithm::{intersects::Intersects, relate::Relate, within::Within};
use geo::line_measures::{Euclidean, Length};
use geo_types::{Coord, Geometry, LineString, Polygon};
use std::str::FromStr;
use wkt::Wkt;

/// Tolerance for coordinate-wise equality in `ST_Equals`.
const EQUALS_EPS: f64 = f64::EPSILON;

/// Evaluate a spatial (`ST_*`) function call.
///
/// The dispatch is case-insensitive on `name`. Returns a typed
/// [`Value`] on success or a typed [`QueryError`] on failure (invalid
/// arity, unparseable WKT, unsupported predicate for the given
/// geometry types, …).
///
/// `_coordinate_dim` is reserved for a future 3-D/M extension; the
/// current implementation operates strictly on 2-D `f64` geometries.
pub fn evaluate_spatial_function(
    name: &str,
    args: &[Value],
    _coordinate_dim: usize,
) -> Result<Value> {
    match name.to_ascii_uppercase().as_str() {
        // ── Binary boolean predicates ───────────────────────────────────────
        "ST_INTERSECTS" => binary_predicate(name, args, |a, b| a.intersects(b)),
        "ST_CONTAINS" => binary_predicate(name, args, |a, b| a.contains(b)),
        "ST_WITHIN" => binary_predicate(name, args, |a, b| a.is_within(b)),
        "ST_DISJOINT" => binary_predicate(name, args, |a, b| !a.intersects(b)),
        "ST_EQUALS" => binary_predicate(name, args, geom_equals_within_eps),
        "ST_TOUCHES" => binary_predicate(name, args, |a, b| a.relate(b).is_touches()),
        "ST_OVERLAPS" => binary_predicate(name, args, |a, b| a.relate(b).is_overlaps()),
        "ST_CROSSES" => binary_predicate(name, args, |a, b| a.relate(b).is_crosses()),
        "ST_COVERS" => binary_predicate(name, args, |a, b| a.relate(b).is_covers()),
        "ST_COVEREDBY" => binary_predicate(name, args, |a, b| a.relate(b).is_coveredby()),

        // ── Tolerance predicate ─────────────────────────────────────────────
        "ST_DWITHIN" => st_dwithin(args),

        // ── Metric / distance ───────────────────────────────────────────────
        "ST_DISTANCE" => st_distance(args),
        "ST_AREA" => st_area(args),
        "ST_LENGTH" => st_length(args),

        // ── Constructors ────────────────────────────────────────────────────
        "ST_CENTROID" => st_centroid(args),
        "ST_ENVELOPE" => st_envelope(args),
        "ST_BUFFER" => st_buffer(args),

        // ── Set operations ──────────────────────────────────────────────────
        "ST_INTERSECTION" => st_bool_op(args, BoolOp::Intersection),
        "ST_UNION" => st_bool_op(args, BoolOp::Union),
        "ST_DIFFERENCE" => st_bool_op(args, BoolOp::Difference),

        other => Err(QueryError::FunctionNotFound(other.to_string())),
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Arity check helper.
fn check_arity(name: &str, args: &[Value], expected: usize) -> Result<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(QueryError::InvalidArgument(format!(
            "{} expects {} arg(s), got {}",
            name,
            expected,
            args.len()
        )))
    }
}

/// Convert a [`Value`] to a [`Geometry<f64>`]. Strings are parsed as WKT.
fn parse_geometry_arg(v: &Value) -> Result<Geometry<f64>> {
    match v {
        Value::Geometry(g) => Ok(g.clone()),
        Value::String(s) => parse_wkt(s),
        other => Err(QueryError::TypeMismatch {
            expected: "geometry or WKT string".to_string(),
            actual: format!("{:?}", other),
        }),
    }
}

/// Parse a WKT string into a [`Geometry<f64>`].
fn parse_wkt(s: &str) -> Result<Geometry<f64>> {
    let parsed = Wkt::<f64>::from_str(s)
        .map_err(|e| QueryError::InvalidArgument(format!("wkt parse error: {}", e)))?;
    Geometry::<f64>::try_from(parsed)
        .map_err(|e| QueryError::InvalidArgument(format!("wkt -> geo: {}", e)))
}

/// Convert a [`Value`] to an `f64` numeric.
fn parse_numeric_arg(v: &Value) -> Result<f64> {
    match v {
        Value::Float64(f) => Ok(*f),
        Value::Float32(f) => Ok(*f as f64),
        Value::Int32(i) => Ok(*i as f64),
        Value::Int64(i) => Ok(*i as f64),
        other => Err(QueryError::TypeMismatch {
            expected: "numeric".to_string(),
            actual: format!("{:?}", other),
        }),
    }
}

/// Evaluate a 2-arg boolean predicate that takes two geometries.
fn binary_predicate<F>(name: &str, args: &[Value], pred: F) -> Result<Value>
where
    F: Fn(&Geometry<f64>, &Geometry<f64>) -> bool,
{
    check_arity(name, args, 2)?;
    let a = parse_geometry_arg(&args[0])?;
    let b = parse_geometry_arg(&args[1])?;
    Ok(Value::Boolean(pred(&a, &b)))
}

/// Coordinate-wise equality between two geometries (within `EQUALS_EPS`).
///
/// This is OGC `ST_Equals` semantics relaxed by `f64::EPSILON`: the two
/// geometries must share the same enum variant and produce identical coordinate
/// sequences when iterated in order.
fn geom_equals_within_eps(a: &Geometry<f64>, b: &Geometry<f64>) -> bool {
    use geo_types::Geometry as G;
    match (a, b) {
        (G::Point(p1), G::Point(p2)) => coord_eq(p1.0, p2.0),
        (G::Line(l1), G::Line(l2)) => coord_eq(l1.start, l2.start) && coord_eq(l1.end, l2.end),
        (G::LineString(ls1), G::LineString(ls2)) => coords_eq(&ls1.0, &ls2.0),
        (G::Polygon(p1), G::Polygon(p2)) => polygon_eq(p1, p2),
        (G::MultiPoint(m1), G::MultiPoint(m2)) => {
            m1.0.len() == m2.0.len() && m1.0.iter().zip(&m2.0).all(|(a, b)| coord_eq(a.0, b.0))
        }
        (G::MultiLineString(m1), G::MultiLineString(m2)) => {
            m1.0.len() == m2.0.len() && m1.0.iter().zip(&m2.0).all(|(a, b)| coords_eq(&a.0, &b.0))
        }
        (G::MultiPolygon(m1), G::MultiPolygon(m2)) => {
            m1.0.len() == m2.0.len() && m1.0.iter().zip(&m2.0).all(|(a, b)| polygon_eq(a, b))
        }
        (G::Rect(r1), G::Rect(r2)) => coord_eq(r1.min(), r2.min()) && coord_eq(r1.max(), r2.max()),
        (G::Triangle(t1), G::Triangle(t2)) => {
            coord_eq(t1.v1(), t2.v1()) && coord_eq(t1.v2(), t2.v2()) && coord_eq(t1.v3(), t2.v3())
        }
        _ => false,
    }
}

fn coord_eq(a: Coord<f64>, b: Coord<f64>) -> bool {
    (a.x - b.x).abs() <= EQUALS_EPS && (a.y - b.y).abs() <= EQUALS_EPS
}

fn coords_eq(a: &[Coord<f64>], b: &[Coord<f64>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| coord_eq(*x, *y))
}

fn polygon_eq(a: &Polygon<f64>, b: &Polygon<f64>) -> bool {
    coords_eq(&a.exterior().0, &b.exterior().0)
        && a.interiors().len() == b.interiors().len()
        && a.interiors()
            .iter()
            .zip(b.interiors())
            .all(|(x, y)| coords_eq(&x.0, &y.0))
}

// ─── Predicate / metric functions ───────────────────────────────────────────

fn st_dwithin(args: &[Value]) -> Result<Value> {
    check_arity("ST_DWithin", args, 3)?;
    let a = parse_geometry_arg(&args[0])?;
    let b = parse_geometry_arg(&args[1])?;
    let threshold = parse_numeric_arg(&args[2])?;
    if threshold.is_nan() || threshold < 0.0 {
        return Err(QueryError::InvalidArgument(format!(
            "ST_DWithin threshold must be a non-negative finite number, got {}",
            threshold
        )));
    }
    let dist = euclidean_geometry_distance(&a, &b);
    Ok(Value::Boolean(dist <= threshold))
}

fn st_distance(args: &[Value]) -> Result<Value> {
    check_arity("ST_Distance", args, 2)?;
    let a = parse_geometry_arg(&args[0])?;
    let b = parse_geometry_arg(&args[1])?;
    Ok(Value::Float64(euclidean_geometry_distance(&a, &b)))
}

fn st_area(args: &[Value]) -> Result<Value> {
    check_arity("ST_Area", args, 1)?;
    let g = parse_geometry_arg(&args[0])?;
    Ok(Value::Float64(g.unsigned_area()))
}

fn st_length(args: &[Value]) -> Result<Value> {
    check_arity("ST_Length", args, 1)?;
    let g = parse_geometry_arg(&args[0])?;
    Ok(Value::Float64(geometry_length(&g)))
}

/// Euclidean length of a geometry. Polygons / points return `0.0`.
fn geometry_length(g: &Geometry<f64>) -> f64 {
    use geo_types::Geometry as G;
    match g {
        G::Line(line) => Euclidean.length(line),
        G::LineString(ls) => Euclidean.length(ls),
        G::MultiLineString(mls) => Euclidean.length(mls),
        G::Triangle(t) => {
            let a = t.v1();
            let b = t.v2();
            let c = t.v3();
            edge_len(a, b) + edge_len(b, c) + edge_len(c, a)
        }
        G::Rect(r) => {
            let p = r.max() - r.min();
            2.0 * (p.x.abs() + p.y.abs())
        }
        G::Polygon(p) => polygon_perimeter(p),
        G::MultiPolygon(mp) => mp.0.iter().map(polygon_perimeter).sum(),
        G::Point(_) | G::MultiPoint(_) => 0.0,
        G::GeometryCollection(c) => c.0.iter().map(geometry_length).sum(),
    }
}

fn edge_len(a: Coord<f64>, b: Coord<f64>) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx.hypot(dy)
}

fn polygon_perimeter(p: &Polygon<f64>) -> f64 {
    let ext = Euclidean.length(p.exterior());
    let interior: f64 = p.interiors().iter().map(|r| Euclidean.length(r)).sum();
    ext + interior
}

/// Minimum Euclidean distance between two arbitrary geometries.
///
/// Delegates to [`Euclidean::distance`] which is implemented for
/// `(&Geometry<f64>, &Geometry<f64>)`.
fn euclidean_geometry_distance(a: &Geometry<f64>, b: &Geometry<f64>) -> f64 {
    use geo::line_measures::Distance;
    Euclidean.distance(a, b)
}

// ─── Constructors ───────────────────────────────────────────────────────────

fn st_centroid(args: &[Value]) -> Result<Value> {
    check_arity("ST_Centroid", args, 1)?;
    let g = parse_geometry_arg(&args[0])?;
    let pt = g.centroid().ok_or_else(|| {
        QueryError::execution("ST_Centroid: geometry has no centroid (empty geometry)")
    })?;
    Ok(Value::Geometry(Geometry::Point(pt)))
}

fn st_envelope(args: &[Value]) -> Result<Value> {
    check_arity("ST_Envelope", args, 1)?;
    let g = parse_geometry_arg(&args[0])?;
    let rect = g.bounding_rect().ok_or_else(|| {
        QueryError::execution("ST_Envelope: geometry has no bounding rectangle (empty geometry)")
    })?;
    // Materialise the AABB as a Polygon for OGC compatibility.
    let mn = rect.min();
    let mx = rect.max();
    let ls = LineString::from(vec![
        Coord { x: mn.x, y: mn.y },
        Coord { x: mx.x, y: mn.y },
        Coord { x: mx.x, y: mx.y },
        Coord { x: mn.x, y: mx.y },
        Coord { x: mn.x, y: mn.y },
    ]);
    Ok(Value::Geometry(Geometry::Polygon(Polygon::new(ls, vec![]))))
}

fn st_buffer(args: &[Value]) -> Result<Value> {
    check_arity("ST_Buffer", args, 2)?;
    let g = parse_geometry_arg(&args[0])?;
    let dist = parse_numeric_arg(&args[1])?;
    if !dist.is_finite() {
        return Err(QueryError::InvalidArgument(format!(
            "ST_Buffer distance must be finite, got {}",
            dist
        )));
    }
    // `geo::Buffer` is implemented for every concrete geometry type as well as
    // `Geometry<f64>` (see geo-0.33 `algorithm/buffer.rs`). The result is a
    // `MultiPolygon`.
    let mp = g.buffer(dist);
    Ok(Value::Geometry(Geometry::MultiPolygon(mp)))
}

// ─── Set operations ─────────────────────────────────────────────────────────

enum BoolOp {
    Intersection,
    Union,
    Difference,
}

fn st_bool_op(args: &[Value], op: BoolOp) -> Result<Value> {
    let name = match op {
        BoolOp::Intersection => "ST_Intersection",
        BoolOp::Union => "ST_Union",
        BoolOp::Difference => "ST_Difference",
    };
    check_arity(name, args, 2)?;
    let a = parse_geometry_arg(&args[0])?;
    let b = parse_geometry_arg(&args[1])?;
    // `geo::BooleanOps` is implemented only for `Polygon<T>` and
    // `MultiPolygon<T>` in geo 0.33; surface a typed error otherwise.
    let multi_a = to_multi_polygon(&a, name)?;
    let multi_b = to_multi_polygon(&b, name)?;
    let result = match op {
        BoolOp::Intersection => multi_a.intersection(&multi_b),
        BoolOp::Union => multi_a.union(&multi_b),
        BoolOp::Difference => multi_a.difference(&multi_b),
    };
    Ok(Value::Geometry(Geometry::MultiPolygon(result)))
}

fn to_multi_polygon(g: &Geometry<f64>, fn_name: &str) -> Result<geo_types::MultiPolygon<f64>> {
    use geo_types::{Geometry as G, MultiPolygon};
    match g {
        G::Polygon(p) => Ok(MultiPolygon::new(vec![p.clone()])),
        G::MultiPolygon(mp) => Ok(mp.clone()),
        other => Err(QueryError::Unsupported(format!(
            "{} requires Polygon/MultiPolygon operands, got {}",
            fn_name,
            geometry_kind(other)
        ))),
    }
}

fn geometry_kind(g: &Geometry<f64>) -> &'static str {
    use geo_types::Geometry as G;
    match g {
        G::Point(_) => "Point",
        G::Line(_) => "Line",
        G::LineString(_) => "LineString",
        G::Polygon(_) => "Polygon",
        G::MultiPoint(_) => "MultiPoint",
        G::MultiLineString(_) => "MultiLineString",
        G::MultiPolygon(_) => "MultiPolygon",
        G::Rect(_) => "Rect",
        G::Triangle(_) => "Triangle",
        G::GeometryCollection(_) => "GeometryCollection",
    }
}

// ─── Unit tests for helpers ─────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::panic)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn wkt(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn test_check_arity_ok_and_err() {
        assert!(check_arity("F", &[Value::Null], 1).is_ok());
        let err = check_arity("F", &[], 2).unwrap_err();
        assert!(matches!(err, QueryError::InvalidArgument(_)));
    }

    #[test]
    fn test_parse_geometry_from_wkt() {
        let g = parse_geometry_arg(&wkt("POINT(1 2)")).expect("parse");
        assert!(matches!(g, Geometry::Point(_)));
    }

    #[test]
    fn test_parse_geometry_type_mismatch() {
        let err = parse_geometry_arg(&Value::Int64(1)).unwrap_err();
        assert!(matches!(err, QueryError::TypeMismatch { .. }));
    }

    #[test]
    fn test_parse_numeric_int_and_float() {
        assert_eq!(parse_numeric_arg(&Value::Int64(7)).unwrap(), 7.0);
        assert_eq!(parse_numeric_arg(&Value::Float64(2.5)).unwrap(), 2.5);
    }
}
