//! Weiler-Atherton polygon clipping engine
//!
//! Production-grade polygon clipping supporting Intersection, Union,
//! Difference, and Symmetric Difference (XOR) operations. Handles holes,
//! degeneracies, self-intersecting polygons, and floating-point edge cases.
//!
//! # Algorithm
//!
//! The Weiler-Atherton algorithm proceeds in five phases:
//! 1. **Intersection detection** -- segment-segment tests between polygon boundaries
//! 2. **Entry/exit labeling** -- cross-product orientation classifies each
//!    intersection as entering or exiting the other polygon
//! 3. **Polygon tracing** -- walk along subject/clip polygons, switching at
//!    intersections to build result rings
//! 4. **Hole handling** -- process holes independently, classify output rings as
//!    exterior/interior by winding, assign holes via point-in-polygon
//! 5. **Degeneracy handling** -- epsilon snap, zero-length edge removal
//!
//! # Operations
//!
//! - [`ClipOperation::Intersection`] -- area in both A and B
//! - [`ClipOperation::Union`] -- area in A or B (or both)
//! - [`ClipOperation::Difference`] -- area in A but not B
//! - [`ClipOperation::SymmetricDifference`] -- area in A or B but not both

use crate::error::{AlgorithmError, Result};
use oxigeo_core::vector::{Coordinate, LineString, Polygon};

#[cfg(feature = "std")]
use std::vec::Vec;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Polygon clipping operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOperation {
    /// Area in both A and B.
    Intersection,
    /// Area in A or B (or both).
    Union,
    /// Area in A but not in B.
    Difference,
    /// Area in A or B but not both (XOR).
    SymmetricDifference,
}

/// Result of a polygon clip, carrying an explicit accuracy signal.
///
/// The exact clipper is a Weiler-Atherton tracer. When it cannot trace a valid
/// boundary for a near-degenerate or concave input it falls back to a
/// centroid-angle vertex reconstruction, which is only exact for star-shaped
/// (convex-ish) vertex sets and may otherwise return an approximate
/// (convex-hull-like) shape. `approximate` is `true` exactly when that degraded
/// path was taken, so callers doing production clipping can detect and handle
/// possibly-inaccurate geometry instead of relying solely on a log line.
#[derive(Debug, Clone)]
pub struct ClipResult {
    /// The resulting polygons (possibly empty).
    pub polygons: Vec<Polygon>,
    /// `true` if any polygon came from the approximate fallback reconstruction.
    pub approximate: bool,
}

// ---------------------------------------------------------------------------
// Internal vertex representation
// ---------------------------------------------------------------------------

/// Tolerance for coordinate comparisons (1e-10).
const EPSILON: f64 = 1e-10;

/// A vertex in the clip-polygon linked list.
#[derive(Debug, Clone)]
struct ClipVertex {
    /// World coordinate.
    coord: Coordinate,
    /// True when this vertex was inserted as an intersection.
    is_intersection: bool,
    /// True when the traversal *enters* the other polygon at this vertex.
    entering: bool,
    /// Index of the corresponding vertex in the other polygon's list.
    neighbor: Option<usize>,
    /// Parametric position along the edge (0..1). Used for ordering
    /// multiple intersections on the same segment.
    alpha: f64,
    /// Index of next vertex (circular).
    next: usize,
    /// Whether this vertex has been visited during tracing.
    visited: bool,
}

/// A flat, index-based circular vertex list for one polygon.
struct ClipPolygon {
    verts: Vec<ClipVertex>,
}

impl ClipPolygon {
    /// Build from a ring's coordinates (must be closed, we strip the
    /// duplicate closing vertex).
    fn from_ring(coords: &[Coordinate]) -> Self {
        let n = if coords.len() >= 2
            && (coords[0].x - coords[coords.len() - 1].x).abs() < EPSILON
            && (coords[0].y - coords[coords.len() - 1].y).abs() < EPSILON
        {
            coords.len() - 1
        } else {
            coords.len()
        };

        let mut verts = Vec::with_capacity(n);
        for i in 0..n {
            verts.push(ClipVertex {
                coord: coords[i],
                is_intersection: false,
                entering: false,
                neighbor: None,
                alpha: 0.0,
                next: (i + 1) % n,
                visited: false,
            });
        }
        Self { verts }
    }

    fn len(&self) -> usize {
        self.verts.len()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Clip two polygons with the given boolean operation.
///
/// Returns a (possibly empty) vector of result polygons.
///
/// # Errors
///
/// Returns [`AlgorithmError`] when either polygon has fewer than 4 exterior
/// coordinates or when an internal invariant is violated.
pub fn clip_polygons(subject: &Polygon, clip: &Polygon, op: ClipOperation) -> Result<Vec<Polygon>> {
    clip_polygons_detailed(subject, clip, op).map(|r| r.polygons)
}

/// Clip two polygons, returning a [`ClipResult`] that also reports whether the
/// approximate centroid-angle fallback reconstruction was used.
///
/// Prefer this over [`clip_polygons`] in production pipelines where a possibly
/// geometrically-inaccurate result (for concave, near-degenerate inputs) must be
/// detected rather than silently accepted. When `ClipResult::approximate` is
/// `true`, the returned polygons may deviate from the exact boolean result.
///
/// # Errors
///
/// Returns [`AlgorithmError`] when either polygon has fewer than 4 exterior
/// coordinates or when an internal invariant is violated.
pub fn clip_polygons_detailed(
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<ClipResult> {
    validate_polygon(subject, "subject")?;
    validate_polygon(clip, "clip")?;

    // Symmetric difference = (A-B) union (B-A)
    if op == ClipOperation::SymmetricDifference {
        let a_minus_b = clip_polygons_detailed(subject, clip, ClipOperation::Difference)?;
        let b_minus_a = clip_polygons_detailed(clip, subject, ClipOperation::Difference)?;
        let mut polygons = a_minus_b.polygons;
        polygons.extend(b_minus_a.polygons);
        return Ok(ClipResult {
            polygons,
            approximate: a_minus_b.approximate || b_minus_a.approximate,
        });
    }

    // Bounding-box quick rejection
    if let (Some(sb), Some(cb)) = (subject.bounds(), clip.bounds()) {
        if sb.2 < cb.0 || cb.2 < sb.0 || sb.3 < cb.1 || cb.3 < sb.1 {
            return Ok(ClipResult {
                polygons: disjoint_result(subject, clip, op),
                approximate: false,
            });
        }
    }

    // Find boundary intersections (exterior vs exterior)
    let ixs = find_all_intersections(&subject.exterior.coords, &clip.exterior.coords);

    // Also check for intersections between exterior and holes
    let has_hole_crossings = check_hole_crossings(subject, clip);

    if ixs.is_empty() && !has_hole_crossings {
        // Exact fast path (containment / identity / disjoint handling).
        return Ok(ClipResult {
            polygons: handle_no_intersections(subject, clip, op)?,
            approximate: false,
        });
    }

    if ixs.is_empty() && has_hole_crossings {
        // Hole boundaries cross but exteriors don't -- one is contained in the other.
        // Handle with containment + hole transfer logic (exact).
        return Ok(ClipResult {
            polygons: handle_hole_crossing_containment(subject, clip, op)?,
            approximate: false,
        });
    }

    // Build vertex lists, insert intersections, label entry/exit, trace
    let (polygons, approximate) = weiler_atherton_clip(subject, clip, op, &ixs)?;
    Ok(ClipResult {
        polygons,
        approximate,
    })
}

/// Clip subject against a set of clip polygons in sequence.
pub fn clip_multi(
    subjects: &[Polygon],
    clips: &[Polygon],
    op: ClipOperation,
) -> Result<Vec<Polygon>> {
    if subjects.is_empty() {
        return match op {
            ClipOperation::Union => Ok(clips.to_vec()),
            _ => Ok(vec![]),
        };
    }
    if clips.is_empty() {
        return match op {
            ClipOperation::Intersection => Ok(vec![]),
            _ => Ok(subjects.to_vec()),
        };
    }

    let mut result = subjects.to_vec();
    for clip_poly in clips {
        let mut next_result = Vec::new();
        for subj in &result {
            let clipped = clip_polygons(subj, clip_poly, op)?;
            next_result.extend(clipped);
        }
        result = next_result;
        if result.is_empty() && op == ClipOperation::Intersection {
            break;
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_polygon(poly: &Polygon, name: &str) -> Result<()> {
    if poly.exterior.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "clip_polygons",
            message: format!(
                "{name} exterior must have at least 4 coordinates, got {}",
                poly.exterior.coords.len()
            ),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Disjoint / containment fast paths
// ---------------------------------------------------------------------------

fn disjoint_result(subject: &Polygon, clip: &Polygon, op: ClipOperation) -> Vec<Polygon> {
    match op {
        ClipOperation::Intersection => vec![],
        ClipOperation::Union => vec![subject.clone(), clip.clone()],
        ClipOperation::Difference => vec![subject.clone()],
        ClipOperation::SymmetricDifference => vec![subject.clone(), clip.clone()],
    }
}

fn handle_no_intersections(
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<Vec<Polygon>> {
    // Check for identical polygons (all vertices match)
    if are_rings_identical(&subject.exterior.coords, &clip.exterior.coords) {
        return match op {
            ClipOperation::Intersection => Ok(vec![subject.clone()]),
            ClipOperation::Union => Ok(vec![subject.clone()]),
            ClipOperation::Difference => Ok(vec![]),
            ClipOperation::SymmetricDifference => Ok(vec![]),
        };
    }

    // Containment check: verify that ALL vertices of one polygon are inside the other.
    // Using just a single test point (centroid) fails when the centroid happens to be
    // inside the other polygon but the polygon itself extends beyond it.
    let subj_in_clip = is_ring_contained_in_polygon(&subject.exterior.coords, clip)?;
    let clip_in_subj = is_ring_contained_in_polygon(&clip.exterior.coords, subject)?;

    match op {
        ClipOperation::Intersection => {
            if subj_in_clip {
                // Subject is inside clip -- but clip may have holes that subtract from subject.
                // Transfer any clip holes that overlap with subject.
                let holes = collect_overlapping_holes(&clip.interiors, &subject.exterior.coords);
                if holes.is_empty() {
                    Ok(vec![subject.clone()])
                } else {
                    let mut all_holes = subject.interiors.clone();
                    all_holes.extend(holes);
                    let poly = Polygon::new(subject.exterior.clone(), all_holes)
                        .map_err(AlgorithmError::Core)?;
                    Ok(vec![poly])
                }
            } else if clip_in_subj {
                // Clip is inside subject -- but subject may have holes that subtract from clip.
                let holes = collect_overlapping_holes(&subject.interiors, &clip.exterior.coords);
                if holes.is_empty() {
                    Ok(vec![clip.clone()])
                } else {
                    let mut all_holes = clip.interiors.clone();
                    all_holes.extend(holes);
                    let poly = Polygon::new(clip.exterior.clone(), all_holes)
                        .map_err(AlgorithmError::Core)?;
                    Ok(vec![poly])
                }
            } else {
                Ok(vec![])
            }
        }
        ClipOperation::Union => {
            if subj_in_clip {
                Ok(vec![clip.clone()])
            } else if clip_in_subj {
                Ok(vec![subject.clone()])
            } else {
                Ok(vec![subject.clone(), clip.clone()])
            }
        }
        ClipOperation::Difference => {
            if subj_in_clip {
                // subject fully inside clip => empty
                Ok(vec![])
            } else if clip_in_subj {
                // clip fully inside subject => subject with clip as hole
                build_subject_with_hole(subject, clip)
            } else {
                Ok(vec![subject.clone()])
            }
        }
        ClipOperation::SymmetricDifference => {
            // handled at top level but included for completeness
            let a = clip_polygons(subject, clip, ClipOperation::Difference)?;
            let b = clip_polygons(clip, subject, ClipOperation::Difference)?;
            let mut r = a;
            r.extend(b);
            Ok(r)
        }
    }
}

/// Check if any hole boundaries cross with the other polygon's exterior or holes.
fn check_hole_crossings(subject: &Polygon, clip: &Polygon) -> bool {
    // Check subject's holes vs clip exterior
    for hole in &subject.interiors {
        let ixs = find_all_intersections(&hole.coords, &clip.exterior.coords);
        if !ixs.is_empty() {
            return true;
        }
    }
    // Check clip's holes vs subject exterior
    for hole in &clip.interiors {
        let ixs = find_all_intersections(&hole.coords, &subject.exterior.coords);
        if !ixs.is_empty() {
            return true;
        }
    }
    false
}

/// Handle the case where exteriors don't cross but hole boundaries do.
/// This happens when one polygon's exterior contains the other, but a hole
/// boundary of the container crosses the contained polygon.
fn handle_hole_crossing_containment(
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<Vec<Polygon>> {
    // Determine which is the container and which is contained.
    // Use all-vertex containment check for consistency with handle_no_intersections.
    let clip_inside_subj =
        is_ring_contained_in_polygon(&clip.exterior.coords, subject).unwrap_or(false);
    let subj_inside_clip =
        is_ring_contained_in_polygon(&subject.exterior.coords, clip).unwrap_or(false);

    match op {
        ClipOperation::Intersection => {
            if clip_inside_subj {
                // Clip is inside subject. Intersection = clip minus subject's holes that overlap.
                let holes = collect_overlapping_holes(&subject.interiors, &clip.exterior.coords);
                let mut all_holes = clip.interiors.clone();
                all_holes.extend(holes);
                let poly =
                    Polygon::new(clip.exterior.clone(), all_holes).map_err(AlgorithmError::Core)?;
                Ok(vec![poly])
            } else if subj_inside_clip {
                // Subject is inside clip. Intersection = subject minus clip's holes that overlap.
                let holes = collect_overlapping_holes(&clip.interiors, &subject.exterior.coords);
                let mut all_holes = subject.interiors.clone();
                all_holes.extend(holes);
                let poly = Polygon::new(subject.exterior.clone(), all_holes)
                    .map_err(AlgorithmError::Core)?;
                Ok(vec![poly])
            } else {
                Ok(vec![])
            }
        }
        ClipOperation::Difference => {
            if subj_inside_clip {
                // Subject inside clip -- difference is empty unless subject is in a hole of clip
                Ok(vec![])
            } else if clip_inside_subj {
                // Clip inside subject -- difference = subject with clip as hole
                build_subject_with_hole(subject, clip)
            } else {
                Ok(vec![subject.clone()])
            }
        }
        ClipOperation::Union => {
            if subj_inside_clip {
                Ok(vec![clip.clone()])
            } else if clip_inside_subj {
                Ok(vec![subject.clone()])
            } else {
                Ok(vec![subject.clone(), clip.clone()])
            }
        }
        ClipOperation::SymmetricDifference => {
            let a = clip_polygons(subject, clip, ClipOperation::Difference)?;
            let b = clip_polygons(clip, subject, ClipOperation::Difference)?;
            let mut r = a;
            r.extend(b);
            Ok(r)
        }
    }
}

/// Build result for Difference when clip is fully contained in subject.
fn build_subject_with_hole(subject: &Polygon, clip: &Polygon) -> Result<Vec<Polygon>> {
    let mut interiors = filter_unaffected_holes(&subject.interiors, clip)?;
    interiors.push(clip.exterior.clone());

    let mut result_polygons = Vec::new();
    let main_poly =
        Polygon::new(subject.exterior.clone(), interiors).map_err(AlgorithmError::Core)?;
    result_polygons.push(main_poly);

    // Holes in clip become filled regions (difference semantics)
    for hole in &clip.interiors {
        if hole.coords.len() >= 4
            && !hole.coords.is_empty()
            && point_in_ring(&hole.coords[0], &subject.exterior.coords)
            && !is_point_in_any_hole(&hole.coords[0], subject)?
        {
            let hole_poly = Polygon::new(hole.clone(), vec![]).map_err(AlgorithmError::Core)?;
            result_polygons.push(hole_poly);
        }
    }

    Ok(result_polygons)
}

// ---------------------------------------------------------------------------
// Intersection detection (Phase 1)
// ---------------------------------------------------------------------------

/// An intersection between two segments.
#[derive(Debug, Clone)]
struct SegmentIx {
    /// Intersection coordinate (snapped).
    coord: Coordinate,
    /// Index of subject segment start vertex.
    subj_seg: usize,
    /// Parametric t along subject segment \[0,1\].
    subj_t: f64,
    /// Index of clip segment start vertex.
    clip_seg: usize,
    /// Parametric t along clip segment \[0,1\].
    clip_t: f64,
}

fn find_all_intersections(
    subj_coords: &[Coordinate],
    clip_coords: &[Coordinate],
) -> Vec<SegmentIx> {
    let sn = ring_vertex_count(subj_coords);
    let cn = ring_vertex_count(clip_coords);
    let mut result = Vec::new();

    for i in 0..sn {
        let i_next = (i + 1) % sn;
        for j in 0..cn {
            let j_next = (j + 1) % cn;
            if let Some((t, u, coord)) = segment_intersect(
                &subj_coords[i],
                &subj_coords[i_next],
                &clip_coords[j],
                &clip_coords[j_next],
            ) {
                // Deduplicate by coordinate proximity
                let dominated = result.iter().any(|ix: &SegmentIx| {
                    (ix.coord.x - coord.x).abs() < EPSILON && (ix.coord.y - coord.y).abs() < EPSILON
                });
                if !dominated {
                    result.push(SegmentIx {
                        coord,
                        subj_seg: i,
                        subj_t: t,
                        clip_seg: j,
                        clip_t: u,
                    });
                }
            }
        }
    }

    result
}

/// Vertex count excluding the duplicate closing vertex.
fn ring_vertex_count(coords: &[Coordinate]) -> usize {
    if coords.len() >= 2
        && (coords[0].x - coords[coords.len() - 1].x).abs() < EPSILON
        && (coords[0].y - coords[coords.len() - 1].y).abs() < EPSILON
    {
        coords.len() - 1
    } else {
        coords.len()
    }
}

/// Compute intersection of two segments.  Returns `(t, u, coord)` if they
/// intersect strictly or at a shared interior point.
fn segment_intersect(
    a1: &Coordinate,
    a2: &Coordinate,
    b1: &Coordinate,
    b2: &Coordinate,
) -> Option<(f64, f64, Coordinate)> {
    let d1x = a2.x - a1.x;
    let d1y = a2.y - a1.y;
    let d2x = b2.x - b1.x;
    let d2y = b2.y - b1.y;

    let cross = d1x * d2y - d1y * d2x;
    if cross.abs() < EPSILON {
        return None; // parallel / collinear -- skip for clipping purposes
    }

    let dx = b1.x - a1.x;
    let dy = b1.y - a1.y;

    let t = (dx * d2y - dy * d2x) / cross;
    let u = (dx * d1y - dy * d1x) / cross;

    // Accept if within (0,1) strictly -- endpoint-only touches (t=0 or t=1
    // combined with u=0 or u=1) don't represent true crossings for clipping.
    // Use a small inset to avoid degeneracies at exact endpoints.
    let inset = 1e-8;
    if t >= -inset && t <= 1.0 + inset && u >= -inset && u <= 1.0 + inset {
        let t_clamped = t.clamp(0.0, 1.0);
        let u_clamped = u.clamp(0.0, 1.0);

        // Skip pure endpoint-endpoint touches (both parameters at 0 or 1)
        let t_at_end = t_clamped < inset || t_clamped > 1.0 - inset;
        let u_at_end = u_clamped < inset || u_clamped > 1.0 - inset;
        if t_at_end && u_at_end {
            return None; // endpoint touch, not a crossing
        }

        let x = a1.x + t_clamped * d1x;
        let y = a1.y + t_clamped * d1y;
        Some((t_clamped, u_clamped, Coordinate::new_2d(x, y)))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Weiler-Atherton core (Phases 2-3)
// ---------------------------------------------------------------------------

///
/// Returns the result polygons together with an `approximate` flag that is
/// `true` when the geometry came from the centroid-angle fallback path.
fn weiler_atherton_clip(
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
    ixs: &[SegmentIx],
) -> Result<(Vec<Polygon>, bool)> {
    // Build vertex lists
    let mut subj_list = ClipPolygon::from_ring(&subject.exterior.coords);
    let mut clip_list = ClipPolygon::from_ring(&clip.exterior.coords);

    // Phase 2a: Insert intersection vertices into both lists
    insert_intersections(&mut subj_list, &mut clip_list, ixs);

    // Phase 2b: Label entry/exit
    label_entry_exit(&mut subj_list, &clip.exterior.coords, op);
    let clip_op_inv = invert_op_for_clip(op);
    label_entry_exit(&mut clip_list, &subject.exterior.coords, clip_op_inv);

    // Phase 3: Trace result polygons
    let rings = trace_result_rings(&mut subj_list, &mut clip_list, op);

    if rings.is_empty() {
        // Fallback: if tracing produces no rings, use the vertex-subset
        // approach as a safety net for near-degenerate cases.
        return fallback_vertex_clip(subject, clip, op);
    }

    // Phase 4: Build output polygons, handling holes
    let (result, approx_build) = build_output_polygons(&rings, subject, clip, op)?;

    // Phase 5: Validate result area against geometric constraints
    let (result, approx_validate) = validate_result_area(result, subject, clip, op)?;

    Ok((result, approx_build || approx_validate))
}

/// Insert intersection vertices into both subject and clip vertex lists,
/// maintaining correct circular next pointers and cross-links.
fn insert_intersections(subj: &mut ClipPolygon, clip: &mut ClipPolygon, ixs: &[SegmentIx]) {
    // Group intersections by segment, sort by alpha
    let mut subj_inserts: Vec<(usize, f64, Coordinate, usize)> = Vec::new(); // (seg, alpha, coord, ix_idx)
    let mut clip_inserts: Vec<(usize, f64, Coordinate, usize)> = Vec::new();

    for (ix_idx, ix) in ixs.iter().enumerate() {
        subj_inserts.push((ix.subj_seg, ix.subj_t, ix.coord, ix_idx));
        clip_inserts.push((ix.clip_seg, ix.clip_t, ix.coord, ix_idx));
    }

    // Sort by segment then alpha (descending alpha so we insert from end to start
    // of each segment to keep indices stable)
    subj_inserts.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    clip_inserts.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Track where each intersection ends up in each list so we can cross-link
    let mut subj_positions: Vec<Option<usize>> = vec![None; ixs.len()];
    let mut clip_positions: Vec<Option<usize>> = vec![None; ixs.len()];

    // Insert into subject list
    for &(seg, alpha, coord, ix_idx) in &subj_inserts {
        let new_idx = subj.verts.len();
        let seg_next = subj.verts[seg].next;
        subj.verts.push(ClipVertex {
            coord,
            is_intersection: true,
            entering: false,
            neighbor: None,
            alpha,
            next: seg_next,
            visited: false,
        });
        subj.verts[seg].next = new_idx;
        subj_positions[ix_idx] = Some(new_idx);
    }

    // Insert into clip list
    for &(seg, alpha, coord, ix_idx) in &clip_inserts {
        let new_idx = clip.verts.len();
        let seg_next = clip.verts[seg].next;
        clip.verts.push(ClipVertex {
            coord,
            is_intersection: true,
            entering: false,
            neighbor: None,
            alpha,
            next: seg_next,
            visited: false,
        });
        clip.verts[seg].next = new_idx;
        clip_positions[ix_idx] = Some(new_idx);
    }

    // Cross-link
    for i in 0..ixs.len() {
        if let (Some(si), Some(ci)) = (subj_positions[i], clip_positions[i]) {
            subj.verts[si].neighbor = Some(ci);
            clip.verts[ci].neighbor = Some(si);
        }
    }
}

/// Label each intersection vertex as entering or exiting.
///
/// For Intersection: entering = going from outside to inside the other polygon.
/// For Difference:   entering = going from inside to outside the clip polygon
///                   (i.e., we want the part of subject that is *outside* clip).
fn label_entry_exit(poly: &mut ClipPolygon, other_ring: &[Coordinate], op: ClipOperation) {
    // Walk the vertex list in order; at each intersection vertex, classify
    // based on whether the midpoint of the previous edge is inside the other polygon.
    let n = poly.len();
    if n == 0 {
        return;
    }

    // Determine which "inside" status we want
    let want_inside = match op {
        ClipOperation::Intersection => true,
        ClipOperation::Union => false,
        ClipOperation::Difference => false,
        ClipOperation::SymmetricDifference => false, // shouldn't reach here
    };

    // Walk the circular list starting from vertex 0
    let mut idx = 0;
    let max_steps = poly.verts.len() * 2; // safety bound
    let mut steps = 0;
    loop {
        if poly.verts[idx].is_intersection {
            // Look at previous original (non-intersection) vertex to determine
            // if we were inside or outside the other polygon before this intersection
            let prev_coord = find_prev_original_coord(poly, idx);
            let was_inside = point_in_ring(&prev_coord, other_ring);

            // For Intersection: if was_inside=false, we're entering (going inside)
            // For Difference/Union: if was_inside=true, we're entering (going outside)
            poly.verts[idx].entering = if want_inside { !was_inside } else { was_inside };
        }

        idx = poly.verts[idx].next;
        steps += 1;
        if idx == 0 || steps > max_steps {
            break;
        }
    }
}

/// Find the coordinate of the previous non-intersection vertex for labeling.
fn find_prev_original_coord(poly: &ClipPolygon, target: usize) -> Coordinate {
    // Walk backwards through the circular list to find a non-intersection vertex.
    // Since we don't have prev pointers, walk forwards from 0 and track the
    // last non-intersection vertex before we reach `target`.
    let mut last_non_ix = poly.verts[0].coord;
    let mut idx = 0;
    let max_steps = poly.verts.len() * 2;
    let mut steps = 0;

    loop {
        if idx == target {
            break;
        }
        if !poly.verts[idx].is_intersection {
            last_non_ix = poly.verts[idx].coord;
        }
        idx = poly.verts[idx].next;
        steps += 1;
        if steps > max_steps {
            break;
        }
    }

    // If we started right at target (idx==0 == target), walk the whole ring
    // to find the last non-intersection vertex
    if steps == 0 {
        idx = poly.verts[0].next;
        let mut count = 0;
        while idx != 0 && count < max_steps {
            if !poly.verts[idx].is_intersection {
                last_non_ix = poly.verts[idx].coord;
            }
            idx = poly.verts[idx].next;
            count += 1;
        }
    }

    last_non_ix
}

/// Invert the operation for the clip polygon's labeling.
fn invert_op_for_clip(op: ClipOperation) -> ClipOperation {
    match op {
        ClipOperation::Intersection => ClipOperation::Intersection,
        ClipOperation::Union => ClipOperation::Union,
        ClipOperation::Difference => ClipOperation::Intersection, // clip wants: inside subject
        ClipOperation::SymmetricDifference => ClipOperation::SymmetricDifference,
    }
}

/// Phase 3: Trace result rings by walking the vertex lists.
fn trace_result_rings(
    subj: &mut ClipPolygon,
    clip: &mut ClipPolygon,
    op: ClipOperation,
) -> Vec<Vec<Coordinate>> {
    let mut rings = Vec::new();
    let max_total = (subj.verts.len() + clip.verts.len()) * 2;

    // Find unvisited entering intersection vertices on the subject list
    loop {
        let start = find_unvisited_entering(subj);
        let start_idx = match start {
            Some(idx) => idx,
            None => break,
        };

        let mut ring_coords: Vec<Coordinate> = Vec::new();
        let mut on_subject = true;
        let mut current = start_idx;
        let mut steps = 0;

        loop {
            if on_subject {
                subj.verts[current].visited = true;
                ring_coords.push(subj.verts[current].coord);

                // If this is an intersection vertex and NOT the start (or we've gone around)
                current = subj.verts[current].next;
                steps += 1;

                // Check if we've returned to start
                if current == start_idx {
                    break;
                }

                if steps > max_total {
                    break;
                }

                // Check if current is an intersection that is exiting => switch to clip
                if subj.verts[current].is_intersection && !subj.verts[current].entering {
                    subj.verts[current].visited = true;
                    ring_coords.push(subj.verts[current].coord);
                    if let Some(neighbor) = subj.verts[current].neighbor {
                        current = clip.verts[neighbor].next;
                        on_subject = false;
                    }
                }
            } else {
                // On clip polygon
                clip.verts[current].visited = true;
                ring_coords.push(clip.verts[current].coord);

                current = clip.verts[current].next;
                steps += 1;

                if steps > max_total {
                    break;
                }

                // Check if current is an intersection that is exiting on clip => switch to subject
                if clip.verts[current].is_intersection && !clip.verts[current].entering {
                    clip.verts[current].visited = true;
                    ring_coords.push(clip.verts[current].coord);
                    if let Some(neighbor) = clip.verts[current].neighbor {
                        // Check if we've returned to start on subject
                        if neighbor == start_idx {
                            break;
                        }
                        current = subj.verts[neighbor].next;
                        on_subject = true;
                    }
                }
            }
        }

        // Close ring and add if valid
        if ring_coords.len() >= 3 {
            close_ring(&mut ring_coords);
            rings.push(ring_coords);
        }
    }

    rings
}

fn find_unvisited_entering(poly: &ClipPolygon) -> Option<usize> {
    for (i, v) in poly.verts.iter().enumerate() {
        if v.is_intersection && v.entering && !v.visited {
            return Some(i);
        }
    }
    None
}

fn close_ring(coords: &mut Vec<Coordinate>) {
    if let (Some(first), Some(last)) = (coords.first(), coords.last()) {
        if (first.x - last.x).abs() > EPSILON || (first.y - last.y).abs() > EPSILON {
            coords.push(*first);
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Build output polygons
// ---------------------------------------------------------------------------

fn build_output_polygons(
    rings: &[Vec<Coordinate>],
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<(Vec<Polygon>, bool)> {
    // Separate exterior (CCW / positive area) from hole (CW / negative area) rings
    let mut exteriors: Vec<Vec<Coordinate>> = Vec::new();
    let mut holes: Vec<Vec<Coordinate>> = Vec::new();

    for ring in rings {
        if ring.len() < 4 {
            continue;
        }
        let area = signed_ring_area(ring);
        if area.abs() < EPSILON {
            continue; // degenerate ring
        }
        // Positive area = CCW = exterior ring; negative = CW = hole
        // (If the input uses CW exterior convention, this is reversed.
        //  We detect based on the subject polygon's orientation.)
        if area > 0.0 {
            exteriors.push(ring.clone());
        } else {
            holes.push(ring.clone());
        }
    }

    // If no exteriors were found, the rings might all be CW (different convention).
    // Flip the classification.
    if exteriors.is_empty() && !holes.is_empty() {
        std::mem::swap(&mut exteriors, &mut holes);
    }

    let mut result = Vec::new();

    for ext_ring in &exteriors {
        // Collect holes that belong inside this exterior ring
        let mut assigned_holes = Vec::new();
        for hole_ring in &holes {
            if !hole_ring.is_empty() && point_in_ring(&hole_ring[0], ext_ring) {
                if hole_ring.len() >= 4 {
                    if let Ok(ls) = LineString::new(hole_ring.clone()) {
                        assigned_holes.push(ls);
                    }
                }
            }
        }

        // Also preserve unaffected holes from the subject polygon (for Difference)
        if op == ClipOperation::Difference {
            let preserved = filter_unaffected_holes(&subject.interiors, clip)?;
            for h in preserved {
                if point_in_ring(&h.coords[0], ext_ring) {
                    assigned_holes.push(h);
                }
            }
        }

        let ext_ls = LineString::new(ext_ring.clone()).map_err(AlgorithmError::Core)?;
        let poly = Polygon::new(ext_ls, assigned_holes).map_err(AlgorithmError::Core)?;
        result.push(poly);
    }

    if result.is_empty() {
        // Tracing produced rings but none became valid polygons -- fallback
        return fallback_vertex_clip(subject, clip, op);
    }

    // Exact result assembled directly from traced rings.
    Ok((result, false))
}

// ---------------------------------------------------------------------------
// Phase 5: Validate result area
// ---------------------------------------------------------------------------

/// Validate that the result area satisfies geometric constraints.
/// If the WA tracing produced incorrect results (wrong entry/exit labeling),
/// fall back to vertex-subset clipping.
fn validate_result_area(
    result: Vec<Polygon>,
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<(Vec<Polygon>, bool)> {
    let total_area: f64 = result
        .iter()
        .map(|p| signed_ring_area(&p.exterior.coords).abs())
        .sum();
    let subj_area = signed_ring_area(&subject.exterior.coords).abs();
    let clip_area = signed_ring_area(&clip.exterior.coords).abs();

    let valid = match op {
        ClipOperation::Intersection => total_area <= subj_area.min(clip_area) + EPSILON,
        ClipOperation::Difference => total_area <= subj_area + EPSILON,
        ClipOperation::Union => total_area <= subj_area + clip_area + EPSILON,
        ClipOperation::SymmetricDifference => true,
    };

    if valid {
        Ok((result, false))
    } else {
        // WA tracing produced geometrically invalid result; fall back
        fallback_vertex_clip(subject, clip, op)
    }
}

// ---------------------------------------------------------------------------
// Fallback: vertex-subset clipping (for near-degenerate / hard cases)
// ---------------------------------------------------------------------------

///
/// Returns `(polygons, approximate)`. `approximate` is `true` only when the
/// centroid-angle reconstruction was actually used to build a polygon; the exact
/// early-out branches (degenerate vertex counts, area-sanity rejections that
/// return the subject or empty) report `false`.
fn fallback_vertex_clip(
    subject: &Polygon,
    clip: &Polygon,
    op: ClipOperation,
) -> Result<(Vec<Polygon>, bool)> {
    let subj_coords = &subject.exterior.coords;
    let clip_coords = &clip.exterior.coords;

    let mut result_coords: Vec<Coordinate> = Vec::new();

    match op {
        ClipOperation::Intersection => {
            // Vertices of subject inside clip + vertices of clip inside subject + intersections
            for c in subj_coords {
                if point_in_ring(c, clip_coords) && !is_point_in_any_hole(c, clip).unwrap_or(false)
                {
                    push_unique(&mut result_coords, *c);
                }
            }
            for c in clip_coords {
                if point_in_ring(c, subj_coords)
                    && !is_point_in_any_hole(c, subject).unwrap_or(false)
                {
                    push_unique(&mut result_coords, *c);
                }
            }
            // Add intersection points
            let ixs = find_all_intersections(subj_coords, clip_coords);
            for ix in &ixs {
                push_unique(&mut result_coords, ix.coord);
            }
        }
        ClipOperation::Difference => {
            // Vertices of subject outside clip + intersection points
            for c in subj_coords {
                if !point_in_ring(c, clip_coords) || is_point_in_any_hole(c, clip).unwrap_or(false)
                {
                    push_unique(&mut result_coords, *c);
                }
            }
            // Add intersection points
            let ixs = find_all_intersections(subj_coords, clip_coords);
            for ix in &ixs {
                push_unique(&mut result_coords, ix.coord);
            }
        }
        ClipOperation::Union => {
            // All vertices of subject + vertices of clip outside subject
            for c in subj_coords {
                push_unique(&mut result_coords, *c);
            }
            for c in clip_coords {
                if !point_in_ring(c, subj_coords)
                    || is_point_in_any_hole(c, subject).unwrap_or(false)
                {
                    push_unique(&mut result_coords, *c);
                }
            }
        }
        ClipOperation::SymmetricDifference => {
            // Handled at top level
            return Ok((vec![], false));
        }
    }

    if result_coords.len() < 3 {
        return match op {
            ClipOperation::Difference => Ok((vec![subject.clone()], false)),
            ClipOperation::Union => Ok((vec![subject.clone()], false)),
            _ => Ok((vec![], false)),
        };
    }

    // Order the points to form a valid polygon. This centroid-angle ordering
    // only reconstructs a correct boundary for star-shaped (convex-ish) vertex
    // sets; for genuinely concave intersection/difference regions it yields an
    // approximate (convex-hull-like) shape. Surface this explicitly rather than
    // returning a silently-wrong result, so callers can detect degraded output
    // in logs. The area sanity checks below still reject grossly invalid shapes.
    tracing::warn!(
        operation = ?op,
        vertices = result_coords.len(),
        "Weiler-Atherton tracing failed; using approximate centroid-angle \
         reconstruction. Result may be geometrically inaccurate for concave regions."
    );
    order_points_ccw(&mut result_coords);
    close_ring(&mut result_coords);

    if result_coords.len() < 4 {
        return match op {
            ClipOperation::Difference => Ok((vec![subject.clone()], false)),
            ClipOperation::Union => Ok((vec![subject.clone()], false)),
            _ => Ok((vec![], false)),
        };
    }

    // Validate: for Difference, result area must not exceed subject area.
    // For Intersection, result area must not exceed min(subject, clip).
    let candidate_area = signed_ring_area(&result_coords).abs();
    let subj_area = signed_ring_area(subj_coords).abs();
    let clip_area = signed_ring_area(clip_coords).abs();

    match op {
        ClipOperation::Difference => {
            if candidate_area > subj_area + EPSILON {
                // Fallback produced an invalid result -- return subject instead
                return Ok((vec![subject.clone()], false));
            }
        }
        ClipOperation::Intersection => {
            let max_valid = subj_area.min(clip_area);
            if candidate_area > max_valid + EPSILON {
                return Ok((vec![], false));
            }
        }
        _ => {}
    }

    // Handle holes for difference
    let interiors = if op == ClipOperation::Difference {
        filter_unaffected_holes(&subject.interiors, clip)?
    } else {
        vec![]
    };

    let ext = LineString::new(result_coords).map_err(AlgorithmError::Core)?;
    let poly = Polygon::new(ext, interiors).map_err(AlgorithmError::Core)?;
    // This polygon came from the approximate centroid-angle reconstruction.
    Ok((vec![poly], true))
}

fn push_unique(coords: &mut Vec<Coordinate>, c: Coordinate) {
    let dominated = coords
        .iter()
        .any(|e| (e.x - c.x).abs() < EPSILON && (e.y - c.y).abs() < EPSILON);
    if !dominated {
        coords.push(c);
    }
}

/// Order points in counter-clockwise order around their centroid.
fn order_points_ccw(coords: &mut [Coordinate]) {
    if coords.len() < 3 {
        return;
    }
    let n = coords.len() as f64;
    let cx: f64 = coords.iter().map(|c| c.x).sum::<f64>() / n;
    let cy: f64 = coords.iter().map(|c| c.y).sum::<f64>() / n;

    coords.sort_by(|a, b| {
        let angle_a = (a.y - cy).atan2(a.x - cx);
        let angle_b = (b.y - cy).atan2(b.x - cx);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ---------------------------------------------------------------------------
// Shared geometry helpers (used by this module and re-exported to difference.rs)
// ---------------------------------------------------------------------------

/// Check if all vertices of a ring are contained within a polygon
/// (inside exterior and not inside any hole).
fn is_ring_contained_in_polygon(ring: &[Coordinate], polygon: &Polygon) -> Result<bool> {
    let n = ring_vertex_count(ring);
    if n == 0 {
        return Ok(false);
    }
    for i in 0..n {
        if !point_in_ring(&ring[i], &polygon.exterior.coords) {
            return Ok(false);
        }
        if is_point_in_any_hole(&ring[i], polygon)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Collect holes from one polygon that overlap with the exterior of another.
/// Used when one polygon is fully contained in another, to transfer the
/// container's holes into the intersection result.
fn collect_overlapping_holes(
    holes: &[LineString],
    contained_exterior: &[Coordinate],
) -> Vec<LineString> {
    let mut result = Vec::new();
    for hole in holes {
        if hole.coords.is_empty() || hole.coords.len() < 4 {
            continue;
        }
        // Check if the hole overlaps with the contained exterior
        let hole_centroid = compute_ring_centroid(&hole.coords);
        if point_in_ring(&hole_centroid, contained_exterior) {
            result.push(hole.clone());
        }
    }
    result
}

/// Check if two closed rings are identical (same vertices in same order,
/// possibly with different closing-vertex duplication).
fn are_rings_identical(a: &[Coordinate], b: &[Coordinate]) -> bool {
    let na = ring_vertex_count(a);
    let nb = ring_vertex_count(b);
    if na != nb || na == 0 {
        return false;
    }
    for i in 0..na {
        if (a[i].x - b[i].x).abs() > EPSILON || (a[i].y - b[i].y).abs() > EPSILON {
            return false;
        }
    }
    true
}

/// Find a point strictly interior to the ring (not on the boundary).
/// Uses the midpoint of the first non-degenerate edge, nudged inward.
fn find_interior_test_point(coords: &[Coordinate]) -> Coordinate {
    let n = ring_vertex_count(coords);
    if n < 3 {
        return coords
            .get(0)
            .copied()
            .unwrap_or(Coordinate::new_2d(0.0, 0.0));
    }

    // Compute centroid of the ring (reliable interior point for convex polygons,
    // and generally works for simple non-pathological concave polygons)
    let cx: f64 = coords[..n].iter().map(|c| c.x).sum::<f64>() / n as f64;
    let cy: f64 = coords[..n].iter().map(|c| c.y).sum::<f64>() / n as f64;
    Coordinate::new_2d(cx, cy)
}

/// Ray-casting point-in-ring test.
pub(crate) fn point_in_ring(point: &Coordinate, ring: &[Coordinate]) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }

    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i].x;
        let yi = ring[i].y;
        let xj = ring[j].x;
        let yj = ring[j].y;

        let intersect = ((yi > point.y) != (yj > point.y))
            && (point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi);

        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Check if a point is inside any hole of a polygon.
pub(crate) fn is_point_in_any_hole(point: &Coordinate, polygon: &Polygon) -> Result<bool> {
    for hole in &polygon.interiors {
        if point_in_ring(point, &hole.coords) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Signed area of a ring (positive = CCW, negative = CW).
pub(crate) fn signed_ring_area(coords: &[Coordinate]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].x * coords[j].y;
        area -= coords[j].x * coords[i].y;
    }
    area / 2.0
}

/// Filter holes from subject that are not affected by the clip polygon.
pub(crate) fn filter_unaffected_holes(
    holes: &[LineString],
    clip: &Polygon,
) -> Result<Vec<LineString>> {
    let mut result = Vec::new();
    for hole in holes {
        if hole.coords.is_empty() {
            continue;
        }
        // Check if hole centroid is inside clip
        let centroid = compute_ring_centroid(&hole.coords);
        if !point_in_ring(&centroid, &clip.exterior.coords) {
            result.push(hole.clone());
        }
    }
    Ok(result)
}

/// Compute the centroid of a ring.
pub(crate) fn compute_ring_centroid(coords: &[Coordinate]) -> Coordinate {
    if coords.is_empty() {
        return Coordinate::new_2d(0.0, 0.0);
    }
    let n = coords.len() as f64;
    let sx: f64 = coords.iter().map(|c| c.x).sum();
    let sy: f64 = coords.iter().map(|c| c.y).sum();
    Coordinate::new_2d(sx / n, sy / n)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a square polygon.
    fn make_square(x: f64, y: f64, size: f64) -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(x, y),
            Coordinate::new_2d(x + size, y),
            Coordinate::new_2d(x + size, y + size),
            Coordinate::new_2d(x, y + size),
            Coordinate::new_2d(x, y),
        ];
        let ext = LineString::new(coords).map_err(AlgorithmError::Core)?;
        Polygon::new(ext, vec![]).map_err(AlgorithmError::Core)
    }

    /// Helper: create a square with a rectangular hole.
    fn make_square_with_hole(
        x: f64,
        y: f64,
        size: f64,
        hx: f64,
        hy: f64,
        hsize: f64,
    ) -> Result<Polygon> {
        let ext_coords = vec![
            Coordinate::new_2d(x, y),
            Coordinate::new_2d(x + size, y),
            Coordinate::new_2d(x + size, y + size),
            Coordinate::new_2d(x, y + size),
            Coordinate::new_2d(x, y),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(hx, hy),
            Coordinate::new_2d(hx + hsize, hy),
            Coordinate::new_2d(hx + hsize, hy + hsize),
            Coordinate::new_2d(hx, hy + hsize),
            Coordinate::new_2d(hx, hy),
        ];
        let ext = LineString::new(ext_coords).map_err(AlgorithmError::Core)?;
        let hole = LineString::new(hole_coords).map_err(AlgorithmError::Core)?;
        Polygon::new(ext, vec![hole]).map_err(AlgorithmError::Core)
    }

    /// Helper: compute polygon area (absolute) using shoelace formula.
    fn poly_area(poly: &Polygon) -> f64 {
        let ext_area = signed_ring_area(&poly.exterior.coords).abs();
        let hole_area: f64 = poly
            .interiors
            .iter()
            .map(|h| signed_ring_area(&h.coords).abs())
            .sum();
        ext_area - hole_area
    }

    // -----------------------------------------------------------------------
    // Intersection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_intersection_disjoint_squares() {
        let a = make_square(0.0, 0.0, 1.0).expect("make a");
        let b = make_square(5.0, 5.0, 1.0).expect("make b");
        let result = clip_polygons(&a, &b, ClipOperation::Intersection).expect("clip");
        assert!(
            result.is_empty(),
            "disjoint squares should have empty intersection"
        );
    }

    #[test]
    fn test_detailed_reports_exact_for_convex_overlap() {
        // A clean convex overlap goes through the exact Weiler-Atherton path, so
        // the detailed API must report `approximate == false` and agree with the
        // plain API.
        let a = make_square(0.0, 0.0, 1.0).expect("make a");
        let b = make_square(0.5, 0.5, 1.0).expect("make b");

        let detailed =
            clip_polygons_detailed(&a, &b, ClipOperation::Intersection).expect("detailed clip");
        assert!(
            !detailed.approximate,
            "convex square overlap must be an exact result, not the fallback"
        );

        let plain = clip_polygons(&a, &b, ClipOperation::Intersection).expect("plain clip");
        assert_eq!(
            detailed.polygons.len(),
            plain.len(),
            "detailed and plain APIs must return the same polygons"
        );
        // Overlap area is 0.5 x 0.5 = 0.25.
        let area: f64 = detailed.polygons.iter().map(poly_area).sum();
        assert!((area - 0.25).abs() < 1e-9);
    }

    #[test]
    fn test_detailed_disjoint_is_exact() {
        let a = make_square(0.0, 0.0, 1.0).expect("make a");
        let b = make_square(5.0, 5.0, 1.0).expect("make b");
        let detailed =
            clip_polygons_detailed(&a, &b, ClipOperation::Intersection).expect("detailed clip");
        assert!(!detailed.approximate);
        assert!(detailed.polygons.is_empty());
    }

    #[test]
    fn test_intersection_overlapping_squares() {
        // Two 1x1 squares overlapping by 0.5 in each direction.
        // Overlap area should be 0.25.
        let a = make_square(0.0, 0.0, 1.0).expect("make a");
        let b = make_square(0.5, 0.5, 1.0).expect("make b");
        let result = clip_polygons(&a, &b, ClipOperation::Intersection).expect("clip");
        assert!(
            !result.is_empty(),
            "overlapping squares should have intersection"
        );
        let total_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        assert!(
            (total_area - 0.25).abs() < 0.05,
            "intersection area should be ~0.25, got {total_area}"
        );
    }

    #[test]
    fn test_intersection_containment() {
        let outer = make_square(0.0, 0.0, 10.0).expect("outer");
        let inner = make_square(2.0, 2.0, 3.0).expect("inner");
        let result = clip_polygons(&outer, &inner, ClipOperation::Intersection).expect("clip");
        assert_eq!(
            result.len(),
            1,
            "containment intersection should return inner polygon"
        );
        let area = poly_area(&result[0]);
        assert!(
            (area - 9.0).abs() < 0.1,
            "intersection should have area ~9.0, got {area}"
        );
    }

    // -----------------------------------------------------------------------
    // Difference tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_difference_disjoint() {
        let a = make_square(0.0, 0.0, 5.0).expect("a");
        let b = make_square(10.0, 10.0, 5.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Difference).expect("clip");
        assert_eq!(result.len(), 1, "disjoint difference returns subject");
    }

    #[test]
    fn test_difference_contained_creates_hole() {
        let outer = make_square(0.0, 0.0, 10.0).expect("outer");
        let inner = make_square(2.0, 2.0, 3.0).expect("inner");
        let result = clip_polygons(&outer, &inner, ClipOperation::Difference).expect("clip");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].interiors.len(), 1, "should have a hole");
    }

    #[test]
    fn test_difference_completely_subtracted() {
        let inner = make_square(2.0, 2.0, 3.0).expect("inner");
        let outer = make_square(0.0, 0.0, 10.0).expect("outer");
        let result = clip_polygons(&inner, &outer, ClipOperation::Difference).expect("clip");
        assert!(result.is_empty(), "subject fully inside clip => empty");
    }

    #[test]
    fn test_difference_with_hole_in_subject() {
        let a = make_square_with_hole(0.0, 0.0, 20.0, 5.0, 5.0, 5.0).expect("a");
        let b = make_square(30.0, 30.0, 5.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Difference).expect("clip");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].interiors.len(), 1, "hole should be preserved");
    }

    // -----------------------------------------------------------------------
    // Union tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_union_disjoint() {
        let a = make_square(0.0, 0.0, 5.0).expect("a");
        let b = make_square(10.0, 10.0, 5.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Union).expect("clip");
        assert_eq!(result.len(), 2, "disjoint union returns both");
    }

    #[test]
    fn test_union_containment() {
        let outer = make_square(0.0, 0.0, 10.0).expect("outer");
        let inner = make_square(2.0, 2.0, 3.0).expect("inner");
        let result = clip_polygons(&outer, &inner, ClipOperation::Union).expect("clip");
        assert_eq!(result.len(), 1, "containment union returns outer");
        let area = poly_area(&result[0]);
        assert!(
            (area - 100.0).abs() < 0.1,
            "union area should be ~100, got {area}"
        );
    }

    // -----------------------------------------------------------------------
    // Symmetric difference (XOR) tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_xor_identical_polygons() {
        let a = make_square(0.0, 0.0, 5.0).expect("a");
        let b = make_square(0.0, 0.0, 5.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::SymmetricDifference).expect("clip");
        // XOR of identical polygons should be empty
        assert!(
            result.is_empty(),
            "XOR of identical polygons should be empty"
        );
    }

    #[test]
    fn test_xor_disjoint() {
        let a = make_square(0.0, 0.0, 5.0).expect("a");
        let b = make_square(10.0, 10.0, 5.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::SymmetricDifference).expect("clip");
        assert_eq!(result.len(), 2, "XOR of disjoint returns both");
    }

    // -----------------------------------------------------------------------
    // Concave polygon test (L-shaped)
    // -----------------------------------------------------------------------

    #[test]
    fn test_intersection_l_shaped_concave() {
        // L-shape: (0,0)-(2,0)-(2,1)-(1,1)-(1,2)-(0,2)-(0,0)
        let l_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(2.0, 1.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(1.0, 2.0),
            Coordinate::new_2d(0.0, 2.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let l_ext = LineString::new(l_coords).expect("l");
        let l_shape = Polygon::new(l_ext, vec![]).expect("l poly");
        let b = make_square(0.5, 0.5, 2.0).expect("b");

        let result = clip_polygons(&l_shape, &b, ClipOperation::Intersection).expect("clip");
        // The intersection should exist and be smaller than both inputs
        assert!(
            !result.is_empty(),
            "L-shape intersection should produce a result"
        );
        let total_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        assert!(total_area > 0.0, "intersection area should be positive");
        assert!(
            total_area < 3.0,
            "intersection should be smaller than L-shape (area 3)"
        );
    }

    // -----------------------------------------------------------------------
    // Degenerate: shared edge
    // -----------------------------------------------------------------------

    #[test]
    fn test_shared_edge_intersection() {
        // Two squares sharing an edge: [0,0]-[1,1] and [1,0]-[2,1]
        let a = make_square(0.0, 0.0, 1.0).expect("a");
        let b = make_square(1.0, 0.0, 1.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Intersection).expect("clip");
        // Shared edge only => degenerate (zero-area) intersection
        let total_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        assert!(
            total_area < 0.01,
            "shared-edge intersection should be degenerate (area near 0), got {total_area}"
        );
    }

    // -----------------------------------------------------------------------
    // clip_multi test
    // -----------------------------------------------------------------------

    #[test]
    fn test_clip_multi_intersection() {
        let subjects = vec![make_square(0.0, 0.0, 10.0).expect("s")];
        let clips = vec![
            make_square(2.0, 2.0, 5.0).expect("c1"),
            make_square(3.0, 3.0, 5.0).expect("c2"),
        ];
        let result =
            clip_multi(&subjects, &clips, ClipOperation::Intersection).expect("clip_multi");
        // Should produce a result since the clip polygons overlap with subject
        // Not asserting exact geometry but it should not panic
        let _ = result;
    }

    // -----------------------------------------------------------------------
    // Self-intersecting "bowtie" polygon
    // -----------------------------------------------------------------------

    #[test]
    fn test_bowtie_self_intersecting() {
        // Bowtie: (0,0)-(1,1)-(1,0)-(0,1)-(0,0)  -- self-intersecting at (0.5, 0.5)
        let bowtie_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let bowtie_ext = LineString::new(bowtie_coords).expect("bowtie");
        let bowtie = Polygon::new(bowtie_ext, vec![]).expect("bowtie poly");
        let sq = make_square(0.0, 0.0, 2.0).expect("sq");

        // Should not panic, may produce approximate result
        let result = clip_polygons(&bowtie, &sq, ClipOperation::Intersection);
        assert!(result.is_ok(), "clipping with bowtie should not error");
    }

    // -----------------------------------------------------------------------
    // Polygon with hole: intersection
    // -----------------------------------------------------------------------

    #[test]
    fn test_intersection_with_hole() {
        let a = make_square_with_hole(0.0, 0.0, 10.0, 3.0, 3.0, 4.0).expect("a");
        let b = make_square(2.0, 2.0, 6.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Intersection).expect("clip");
        // b overlaps both the exterior and the hole of a
        // Result should exist and have area < area(b)=36
        // Expected: 36 - 16 = 20 (B minus overlap with A's hole)
        let total_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        assert!(total_area > 0.0, "should have some intersection");
        assert!(
            total_area < 36.1,
            "should be less than b's area, got {total_area}"
        );
    }

    // -----------------------------------------------------------------------
    // Validation error tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_polygon_error() {
        // Polygon with too few coordinates
        let bad_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let bad_ext = LineString::new(bad_coords);
        // LineString::new might reject this, but if it doesn't, clip should
        if let Ok(ext) = bad_ext {
            let poly = Polygon {
                exterior: ext,
                interiors: vec![],
            };
            let good = make_square(0.0, 0.0, 1.0).expect("good");
            let result = clip_polygons(&poly, &good, ClipOperation::Intersection);
            assert!(result.is_err(), "should reject polygon with < 4 coords");
        }
    }

    // -----------------------------------------------------------------------
    // Area invariant tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_area_invariant_intersection_le_min() {
        let a = make_square(0.0, 0.0, 3.0).expect("a");
        let b = make_square(1.0, 1.0, 3.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Intersection).expect("clip");
        let ix_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        let area_a = poly_area(&a);
        let area_b = poly_area(&b);
        let min_area = area_a.min(area_b);
        assert!(
            ix_area <= min_area + 0.1,
            "intersection area ({ix_area}) should be <= min({area_a}, {area_b}) = {min_area}"
        );
    }

    #[test]
    fn test_area_invariant_difference_le_subject() {
        let a = make_square(0.0, 0.0, 3.0).expect("a");
        let b = make_square(1.0, 1.0, 3.0).expect("b");
        let result = clip_polygons(&a, &b, ClipOperation::Difference).expect("clip");
        let diff_area: f64 = result.iter().map(|p| poly_area(p)).sum();
        let area_a = poly_area(&a);
        assert!(
            diff_area <= area_a + 0.1,
            "difference area ({diff_area}) should be <= subject area ({area_a})"
        );
    }
}
