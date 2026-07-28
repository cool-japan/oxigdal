//! Ring-winding utilities shared by the Shapefile reader and writer.
//!
//! The ESRI Shapefile specification distinguishes exterior (outer) rings from
//! interior (hole) rings by their **winding direction**, not by their position
//! in the shape record's part list: outer-ring vertices are ordered *clockwise*,
//! hole vertices *counter-clockwise*. A single `Polygon` shape record may therefore
//! contain the rings of *several* logical polygons (e.g. a country with offshore
//! islands), each island being its own clockwise exterior ring optionally followed
//! by its counter-clockwise holes.
//!
//! This module provides the winding math plus the two spec-conformant transforms:
//!
//! * [`normalize_polygon_rings`] / [`normalize_multipolygon_rings`] — used by the
//!   writer to force exteriors clockwise and holes counter-clockwise before the
//!   rings are flattened into a single multi-part `Shape::Polygon*` record, so the
//!   files this crate produces are spec-conformant for other readers (GDAL/OGR,
//!   QGIS, the reference flatgeobuf/shapefile implementations).
//! * [`assemble_polygons`] — used by the reader to regroup a flat list of rings
//!   back into one or more polygons using winding direction plus point-in-polygon
//!   containment, returning a `MultiPolygon` whenever more than one exterior ring
//!   is present.

use crate::error::{Result, ShapefileError};
use oxigeo_core::vector::{Coordinate, Geometry, LineString, MultiPolygon, Polygon};

/// Twice the signed area of a ring (shoelace formula).
///
/// In a y-up coordinate frame a positive result means the ring is wound
/// counter-clockwise; a negative result means clockwise. Rings are assumed to be
/// closed (first vertex repeated as the last); the closing edge contributes zero
/// so it does not affect the sign.
#[must_use]
pub(crate) fn signed_area2(ring: &[Coordinate]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = &ring[i];
        let b = &ring[(i + 1) % n];
        sum += a.x.mul_add(b.y, -(b.x * a.y));
    }
    sum
}

/// Returns `true` when the ring is wound clockwise (the ESRI exterior-ring
/// convention).
#[must_use]
pub(crate) fn is_clockwise(ring: &[Coordinate]) -> bool {
    signed_area2(ring) < 0.0
}

/// Returns the ring's vertices reordered so its winding matches
/// `want_clockwise`, reversing in place only when necessary.
fn oriented(ring: &[Coordinate], want_clockwise: bool) -> Vec<Coordinate> {
    if is_clockwise(ring) == want_clockwise {
        ring.to_vec()
    } else {
        ring.iter().rev().copied().collect()
    }
}

/// Produces the winding-normalized rings of a single polygon: exterior forced
/// clockwise, then each hole forced counter-clockwise (ESRI convention).
#[must_use]
pub(crate) fn normalize_polygon_rings(polygon: &Polygon) -> Vec<Vec<Coordinate>> {
    let mut rings = Vec::with_capacity(1 + polygon.interiors.len());
    rings.push(oriented(&polygon.exterior.coords, true));
    for hole in &polygon.interiors {
        rings.push(oriented(&hole.coords, false));
    }
    rings
}

/// Produces the winding-normalized rings for every polygon in a multi-polygon,
/// concatenated in order (each polygon's clockwise exterior followed by its
/// counter-clockwise holes).
#[must_use]
pub(crate) fn normalize_multipolygon_rings(multipolygon: &MultiPolygon) -> Vec<Vec<Coordinate>> {
    let mut rings = Vec::new();
    for polygon in &multipolygon.polygons {
        rings.extend(normalize_polygon_rings(polygon));
    }
    rings
}

/// Ray-casting point-in-ring test. Returns `true` when `(px, py)` lies inside
/// the (closed) `ring`.
#[must_use]
fn point_in_ring(px: f64, py: f64, ring: &[Coordinate]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[j].x, ring[j].y);
        let intersects = ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi);
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Picks a representative interior-ish point for a ring: its centroid falls back
/// to the first vertex for degenerate rings. Used only to test which exterior
/// ring encloses a given hole.
fn representative_point(ring: &[Coordinate]) -> (f64, f64) {
    // The first vertex of a hole is a genuine vertex of that hole and therefore
    // lies inside (or on the boundary of) whichever exterior ring encloses it,
    // which is sufficient for containment matching against the *outer* rings.
    match ring.first() {
        Some(c) => (c.x, c.y),
        None => (0.0, 0.0),
    }
}

/// Reassembles a flat list of rings (in shape-record part order) into one or more
/// polygons using the ESRI winding convention.
///
/// * Clockwise rings start a new exterior polygon.
/// * Counter-clockwise rings are holes, assigned to the smallest-area exterior
///   ring that geometrically contains them (falling back to their own polygon if
///   no exterior encloses them).
///
/// Returns `Geometry::Polygon` when exactly one exterior ring is found and
/// `Geometry::MultiPolygon` when several are, or `Ok(None)` when no usable ring
/// remains after filtering degenerate parts.
pub(crate) fn assemble_polygons(rings: Vec<Vec<Coordinate>>) -> Result<Option<Geometry>> {
    // A valid shapefile ring is closed and has at least 4 vertices.
    let rings: Vec<Vec<Coordinate>> = rings.into_iter().filter(|r| r.len() >= 4).collect();
    if rings.is_empty() {
        return Ok(None);
    }

    let areas: Vec<f64> = rings.iter().map(|r| signed_area2(r)).collect();

    let mut outers: Vec<usize> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    for (i, &area) in areas.iter().enumerate() {
        if area < 0.0 {
            outers.push(i);
        } else {
            holes.push(i);
        }
    }

    // Defensive fallback: a file whose rings are all wound counter-clockwise
    // (some non-conformant producers do this) has no detectable exterior. Rather
    // than dropping the geometry, treat every ring as its own exterior.
    if outers.is_empty() {
        outers = (0..rings.len()).collect();
        holes.clear();
    }

    let mut outer_holes: Vec<Vec<usize>> = vec![Vec::new(); outers.len()];
    let mut orphan_holes: Vec<usize> = Vec::new();
    for &h in &holes {
        let (px, py) = representative_point(&rings[h]);
        let mut best: Option<(usize, f64)> = None;
        for (pos, &oi) in outers.iter().enumerate() {
            if point_in_ring(px, py, &rings[oi]) {
                let abs_area = areas[oi].abs();
                if best.is_none_or(|(_, best_area)| abs_area < best_area) {
                    best = Some((pos, abs_area));
                }
            }
        }
        match best {
            Some((pos, _)) => outer_holes[pos].push(h),
            None => orphan_holes.push(h),
        }
    }

    let mut polygons: Vec<Polygon> = Vec::with_capacity(outers.len());
    for (pos, &oi) in outers.iter().enumerate() {
        let exterior = LineString::new(rings[oi].clone())
            .map_err(|e| ShapefileError::invalid_geometry(format!("Invalid exterior ring: {e}")))?;
        let mut interiors = Vec::with_capacity(outer_holes[pos].len());
        for &h in &outer_holes[pos] {
            if let Ok(interior) = LineString::new(rings[h].clone()) {
                interiors.push(interior);
            }
        }
        let polygon = Polygon::new(exterior, interiors)
            .map_err(|e| ShapefileError::invalid_geometry(format!("Invalid polygon: {e}")))?;
        polygons.push(polygon);
    }

    // Holes enclosed by no exterior are surfaced as standalone polygons so their
    // geometry is not silently lost.
    for &h in &orphan_holes {
        if let Ok(exterior) = LineString::new(rings[h].clone())
            && let Ok(polygon) = Polygon::new(exterior, Vec::new())
        {
            polygons.push(polygon);
        }
    }

    match polygons.len() {
        0 => Ok(None),
        1 => match polygons.into_iter().next() {
            Some(polygon) => Ok(Some(Geometry::Polygon(polygon))),
            None => Ok(None),
        },
        _ => Ok(Some(Geometry::MultiPolygon(MultiPolygon::new(polygons)))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn ring(pts: &[(f64, f64)]) -> Vec<Coordinate> {
        pts.iter().map(|&(x, y)| Coordinate::new_2d(x, y)).collect()
    }

    // Clockwise unit square (ESRI exterior) around the given origin.
    fn cw_square(ox: f64, oy: f64, s: f64) -> Vec<Coordinate> {
        ring(&[
            (ox, oy),
            (ox, oy + s),
            (ox + s, oy + s),
            (ox + s, oy),
            (ox, oy),
        ])
    }

    // Counter-clockwise square (ESRI hole).
    fn ccw_square(ox: f64, oy: f64, s: f64) -> Vec<Coordinate> {
        ring(&[
            (ox, oy),
            (ox + s, oy),
            (ox + s, oy + s),
            (ox, oy + s),
            (ox, oy),
        ])
    }

    #[test]
    fn winding_detection() {
        assert!(is_clockwise(&cw_square(0.0, 0.0, 1.0)));
        assert!(!is_clockwise(&ccw_square(0.0, 0.0, 1.0)));
    }

    #[test]
    fn single_exterior_yields_polygon() {
        let g = assemble_polygons(vec![cw_square(0.0, 0.0, 10.0)])
            .expect("assemble")
            .expect("some geometry");
        assert!(matches!(g, Geometry::Polygon(_)));
    }

    #[test]
    fn two_exteriors_yield_multipolygon() {
        let g = assemble_polygons(vec![cw_square(0.0, 0.0, 5.0), cw_square(100.0, 0.0, 5.0)])
            .expect("assemble")
            .expect("some geometry");
        match g {
            Geometry::MultiPolygon(mp) => assert_eq!(mp.polygons.len(), 2),
            other => panic!("expected MultiPolygon, got {other:?}"),
        }
    }

    #[test]
    fn hole_assigned_to_containing_exterior() {
        // Big exterior with a hole inside, plus a distant second island.
        let big = cw_square(0.0, 0.0, 100.0);
        let hole = ccw_square(10.0, 10.0, 5.0);
        let island = cw_square(1000.0, 0.0, 10.0);
        let g = assemble_polygons(vec![big, hole, island])
            .expect("assemble")
            .expect("some geometry");
        match g {
            Geometry::MultiPolygon(mp) => {
                assert_eq!(mp.polygons.len(), 2);
                // The polygon covering the origin must own the hole.
                let with_hole = mp
                    .polygons
                    .iter()
                    .find(|p| !p.interiors.is_empty())
                    .expect("one polygon must have the hole");
                assert_eq!(with_hole.interiors.len(), 1);
                let without = mp
                    .polygons
                    .iter()
                    .filter(|p| p.interiors.is_empty())
                    .count();
                assert_eq!(without, 1);
            }
            other => panic!("expected MultiPolygon, got {other:?}"),
        }
    }

    #[test]
    fn all_ccw_rings_fall_back_to_separate_exteriors() {
        // No clockwise ring at all: must not drop the geometry.
        let g = assemble_polygons(vec![ccw_square(0.0, 0.0, 5.0), ccw_square(100.0, 0.0, 5.0)])
            .expect("assemble")
            .expect("some geometry");
        assert!(matches!(g, Geometry::MultiPolygon(_)));
    }

    #[test]
    fn normalize_forces_esri_winding() {
        // Build an OGC-wound polygon (exterior CCW, hole CW) and confirm the
        // normalizer flips both to ESRI orientation.
        let exterior = LineString::new(ccw_square(0.0, 0.0, 100.0)).expect("exterior");
        let hole = LineString::new(cw_square(10.0, 10.0, 5.0)).expect("hole");
        let poly = Polygon::new(exterior, vec![hole]).expect("polygon");
        let rings = normalize_polygon_rings(&poly);
        assert_eq!(rings.len(), 2);
        assert!(is_clockwise(&rings[0]), "exterior must be clockwise");
        assert!(!is_clockwise(&rings[1]), "hole must be counter-clockwise");
    }
}
