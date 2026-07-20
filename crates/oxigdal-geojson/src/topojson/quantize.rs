//! Coordinate quantisation for TopoJSON encoding.
//!
//! Floating-point coordinates are mapped onto an integer grid of size `q × q`
//! (where `q` is the quantisation level, typically 10 000).  The transform
//! records the scale and translate parameters needed to reconstruct approximate
//! floating-point positions on decode.

use crate::parser::FeatureCollection;
use crate::types::GeoJsonGeometry;

// ─── Coordinate types ────────────────────────────────────────────────────────

/// An integer grid position produced by quantisation.
pub(crate) type QuantPoint = (i32, i32);

/// Floating-point bounding box of the input data.
pub(crate) struct QuantBbox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

// ─── Transform ───────────────────────────────────────────────────────────────

/// Affine transform that maps geographic coordinates to integer grid positions.
///
/// The forward transform is:
/// ```text
/// qx = round((x - translate_x) / scale_x)
/// qy = round((y - translate_y) / scale_y)
/// ```
///
/// The inverse is:
/// ```text
/// x = qx * scale_x + translate_x
/// y = qy * scale_y + translate_y
/// ```
pub(crate) struct QuantTransform {
    pub scale_x: f64,
    pub scale_y: f64,
    pub translate_x: f64,
    pub translate_y: f64,
}

impl QuantTransform {
    /// Build a transform that maps `(min_x, min_y)` → `(0, 0)` and
    /// `(max_x, max_y)` → `(q-1, q-1)`.
    pub fn from_bbox(bbox: &QuantBbox, q: u32) -> Self {
        let q_f = (q as f64) - 1.0;
        let width = bbox.max_x - bbox.min_x;
        let height = bbox.max_y - bbox.min_y;
        // Guard against degenerate (zero-extent) bboxes
        let safe_w = if width.abs() < 1e-10 { 1e-10 } else { width };
        let safe_h = if height.abs() < 1e-10 { 1e-10 } else { height };
        Self {
            scale_x: safe_w / q_f,
            scale_y: safe_h / q_f,
            translate_x: bbox.min_x,
            translate_y: bbox.min_y,
        }
    }

    /// Quantise a coordinate pair to integer grid positions.
    pub fn quantise(&self, x: f64, y: f64) -> QuantPoint {
        let qx = ((x - self.translate_x) / self.scale_x).round() as i32;
        let qy = ((y - self.translate_y) / self.scale_y).round() as i32;
        (qx, qy)
    }

    /// Dequantise a grid position back to approximate floating-point coordinates.
    #[allow(dead_code)]
    pub fn dequantise(&self, p: QuantPoint) -> (f64, f64) {
        (
            p.0 as f64 * self.scale_x + self.translate_x,
            p.1 as f64 * self.scale_y + self.translate_y,
        )
    }
}

// ─── Bbox computation ────────────────────────────────────────────────────────

/// Compute the bounding box of all coordinates in a `FeatureCollection`.
///
/// Returns `None` when the collection is empty or contains only null/point-less
/// geometries.
pub(crate) fn compute_fc_bbox(fc: &FeatureCollection) -> Option<QuantBbox> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut found_any = false;

    for feat in &fc.features {
        if let Some(geom) = &feat.geometry {
            visit_coords(geom, |x, y| {
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
                found_any = true;
            });
        }
    }

    if !found_any {
        return None;
    }

    // Ensure non-zero extent so the transform is always invertible
    if (max_x - min_x).abs() < 1e-10 {
        max_x += 1e-10;
        min_x -= 1e-10;
    }
    if (max_y - min_y).abs() < 1e-10 {
        max_y += 1e-10;
        min_y -= 1e-10;
    }

    Some(QuantBbox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

/// Visit every 2-D coordinate in a geometry, calling `f(x, y)`.
fn visit_coords<F: FnMut(f64, f64)>(geom: &GeoJsonGeometry, mut f: F) {
    visit_coords_inner(geom, &mut f);
}

fn visit_coords_inner<F: FnMut(f64, f64)>(geom: &GeoJsonGeometry, f: &mut F) {
    match geom {
        GeoJsonGeometry::Point([x, y]) => f(*x, *y),
        GeoJsonGeometry::PointZ([x, y, _]) => f(*x, *y),
        GeoJsonGeometry::LineString(pts) => {
            for [x, y] in pts {
                f(*x, *y);
            }
        }
        GeoJsonGeometry::LineStringZ(pts) => {
            for [x, y, _] in pts {
                f(*x, *y);
            }
        }
        GeoJsonGeometry::Polygon(rings) => {
            for ring in rings {
                for [x, y] in ring {
                    f(*x, *y);
                }
            }
        }
        GeoJsonGeometry::PolygonZ(rings) => {
            for ring in rings {
                for [x, y, _] in ring {
                    f(*x, *y);
                }
            }
        }
        GeoJsonGeometry::MultiPoint(pts) => {
            for [x, y] in pts {
                f(*x, *y);
            }
        }
        GeoJsonGeometry::MultiPointZ(pts) => {
            for [x, y, _] in pts {
                f(*x, *y);
            }
        }
        GeoJsonGeometry::MultiLineString(lines) => {
            for line in lines {
                for [x, y] in line {
                    f(*x, *y);
                }
            }
        }
        GeoJsonGeometry::MultiLineStringZ(lines) => {
            for line in lines {
                for [x, y, _] in line {
                    f(*x, *y);
                }
            }
        }
        GeoJsonGeometry::MultiPolygon(polys) => {
            for poly in polys {
                for ring in poly {
                    for [x, y] in ring {
                        f(*x, *y);
                    }
                }
            }
        }
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            for poly in polys {
                for ring in poly {
                    for [x, y, _] in ring {
                        f(*x, *y);
                    }
                }
            }
        }
        GeoJsonGeometry::GeometryCollection(geoms) => {
            for g in geoms {
                visit_coords_inner(g, f);
            }
        }
        GeoJsonGeometry::Null => {}
    }
}

// ─── Ring collection ─────────────────────────────────────────────────────────

/// Metadata linking a path (polygon ring or line chain) back to its source
/// feature and geometry position.
#[derive(Debug, Clone)]
pub(crate) struct RingSource {
    /// Index of the feature in the `FeatureCollection`.
    pub feature_idx: usize,
    /// For polygon rings: which polygon within a MultiPolygon (always 0 for a
    /// plain Polygon).  For line chains: which line within a MultiLineString
    /// (always 0 for a plain LineString).
    pub poly_idx: usize,
    /// For polygon rings: which ring within the polygon (0 = exterior,
    /// 1+ = holes).  For line chains this is always 0 (a line is a single
    /// chain).
    pub ring_idx: usize,
    /// `true` when the path is a *closed* polygon ring; `false` when it is an
    /// *open* line chain (LineString / MultiLineString path).  Rings and chains
    /// are processed together for junction detection and arc sharing, but their
    /// cutting and normalisation rules differ.
    pub is_ring: bool,
    /// Path of GeometryCollection member indices from the feature's top-level
    /// geometry down to the geometry that emitted this path.
    ///
    /// Empty for a top-level (non-collection) geometry.  For a geometry that is
    /// the `k`-th member of the feature's top-level `GeometryCollection` the
    /// path is `[k]`; nested collections extend the path (`[k, j, …]`).  This
    /// disambiguates sibling members that would otherwise collide on
    /// `(feature_idx, poly_idx, ring_idx, is_ring)` — e.g. two `Polygon`
    /// members both reporting `poly_idx = 0, ring_idx = 0`.
    pub member_path: Vec<usize>,
}

/// Collect all polygon rings from the feature collection, quantised to integer
/// grid positions.  Also returns a parallel `Vec<RingSource>` mapping each
/// ring back to its origin.
pub(crate) fn collect_all_rings(
    fc: &FeatureCollection,
    transform: &QuantTransform,
) -> (Vec<Vec<QuantPoint>>, Vec<RingSource>) {
    let mut rings: Vec<Vec<QuantPoint>> = Vec::new();
    let mut sources: Vec<RingSource> = Vec::new();

    for (feat_idx, feat) in fc.features.iter().enumerate() {
        let geom = match &feat.geometry {
            Some(g) => g,
            None => continue,
        };
        collect_rings_from_geom(geom, feat_idx, &[], transform, &mut rings, &mut sources);
    }

    (rings, sources)
}

fn collect_rings_from_geom(
    geom: &GeoJsonGeometry,
    feat_idx: usize,
    member_path: &[usize],
    transform: &QuantTransform,
    rings: &mut Vec<Vec<QuantPoint>>,
    sources: &mut Vec<RingSource>,
) {
    match geom {
        GeoJsonGeometry::Polygon(poly_rings) => {
            for (ring_idx, ring) in poly_rings.iter().enumerate() {
                let qring: Vec<QuantPoint> = ring
                    .iter()
                    .map(|[x, y]| transform.quantise(*x, *y))
                    .collect();
                rings.push(qring);
                sources.push(RingSource {
                    feature_idx: feat_idx,
                    poly_idx: 0,
                    ring_idx,
                    is_ring: true,
                    member_path: member_path.to_vec(),
                });
            }
        }
        GeoJsonGeometry::PolygonZ(poly_rings) => {
            for (ring_idx, ring) in poly_rings.iter().enumerate() {
                let qring: Vec<QuantPoint> = ring
                    .iter()
                    .map(|[x, y, _]| transform.quantise(*x, *y))
                    .collect();
                rings.push(qring);
                sources.push(RingSource {
                    feature_idx: feat_idx,
                    poly_idx: 0,
                    ring_idx,
                    is_ring: true,
                    member_path: member_path.to_vec(),
                });
            }
        }
        GeoJsonGeometry::MultiPolygon(polys) => {
            for (poly_idx, poly_rings) in polys.iter().enumerate() {
                for (ring_idx, ring) in poly_rings.iter().enumerate() {
                    let qring: Vec<QuantPoint> = ring
                        .iter()
                        .map(|[x, y]| transform.quantise(*x, *y))
                        .collect();
                    rings.push(qring);
                    sources.push(RingSource {
                        feature_idx: feat_idx,
                        poly_idx,
                        ring_idx,
                        is_ring: true,
                        member_path: member_path.to_vec(),
                    });
                }
            }
        }
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            for (poly_idx, poly_rings) in polys.iter().enumerate() {
                for (ring_idx, ring) in poly_rings.iter().enumerate() {
                    let qring: Vec<QuantPoint> = ring
                        .iter()
                        .map(|[x, y, _]| transform.quantise(*x, *y))
                        .collect();
                    rings.push(qring);
                    sources.push(RingSource {
                        feature_idx: feat_idx,
                        poly_idx,
                        ring_idx,
                        is_ring: true,
                        member_path: member_path.to_vec(),
                    });
                }
            }
        }
        GeoJsonGeometry::LineString(pts) => {
            let chain: Vec<QuantPoint> = pts
                .iter()
                .map(|[x, y]| transform.quantise(*x, *y))
                .collect();
            rings.push(chain);
            sources.push(RingSource {
                feature_idx: feat_idx,
                poly_idx: 0,
                ring_idx: 0,
                is_ring: false,
                member_path: member_path.to_vec(),
            });
        }
        GeoJsonGeometry::LineStringZ(pts) => {
            let chain: Vec<QuantPoint> = pts
                .iter()
                .map(|[x, y, _]| transform.quantise(*x, *y))
                .collect();
            rings.push(chain);
            sources.push(RingSource {
                feature_idx: feat_idx,
                poly_idx: 0,
                ring_idx: 0,
                is_ring: false,
                member_path: member_path.to_vec(),
            });
        }
        GeoJsonGeometry::MultiLineString(lines) => {
            for (line_idx, line) in lines.iter().enumerate() {
                let chain: Vec<QuantPoint> = line
                    .iter()
                    .map(|[x, y]| transform.quantise(*x, *y))
                    .collect();
                rings.push(chain);
                sources.push(RingSource {
                    feature_idx: feat_idx,
                    poly_idx: line_idx,
                    ring_idx: 0,
                    is_ring: false,
                    member_path: member_path.to_vec(),
                });
            }
        }
        GeoJsonGeometry::MultiLineStringZ(lines) => {
            for (line_idx, line) in lines.iter().enumerate() {
                let chain: Vec<QuantPoint> = line
                    .iter()
                    .map(|[x, y, _]| transform.quantise(*x, *y))
                    .collect();
                rings.push(chain);
                sources.push(RingSource {
                    feature_idx: feat_idx,
                    poly_idx: line_idx,
                    ring_idx: 0,
                    is_ring: false,
                    member_path: member_path.to_vec(),
                });
            }
        }
        GeoJsonGeometry::GeometryCollection(geoms) => {
            for (member_idx, g) in geoms.iter().enumerate() {
                let mut child_path = member_path.to_vec();
                child_path.push(member_idx);
                collect_rings_from_geom(g, feat_idx, &child_path, transform, rings, sources);
            }
        }
        // Point / MultiPoint — no arcs to track.
        GeoJsonGeometry::Point(_)
        | GeoJsonGeometry::PointZ(_)
        | GeoJsonGeometry::MultiPoint(_)
        | GeoJsonGeometry::MultiPointZ(_)
        | GeoJsonGeometry::Null => {}
    }
}
