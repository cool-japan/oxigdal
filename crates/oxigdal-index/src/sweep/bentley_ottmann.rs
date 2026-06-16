//! Bentley-Ottmann sweep-line algorithm for batch segment intersection detection.
//!
//! # Algorithm
//!
//! The Bentley-Ottmann algorithm finds all k pairwise intersections among n
//! line segments in O((n + k) log n) time.  We sweep a vertical line left to
//! right, maintaining a status structure of active segments ordered by their
//! y-coordinate at the current sweep-line x.
//!
//! # Implementation strategy
//!
//! Classic Bentley-Ottmann only checks adjacent segment pairs in the status,
//! which is correct because non-adjacent pairs can only cross after being made
//! adjacent by earlier crossing events.  However, **vertical and near-vertical
//! segments** break this invariant: a vertical segment at x = c simultaneously
//! intersects every active horizontal segment whose y-range overlaps [p0.y,
//! p1.y], yet none of those horizontals need to pass through adjacency with
//! each other before the vertical arrives.
//!
//! Our fix: when inserting a near-vertical segment (|dx| < epsilon), we scan
//! the entire status and schedule crossings against every active segment whose
//! y-at-x lies within the vertical's y-interval.  For normal segments we keep
//! the O((n+k) log n) adjacent-pair check.
//!
//! # Limitations
//!
//! * Collinear overlapping segments are **not** reported (deferred; documented
//!   in the module header).
//! * Each pair is reported exactly once with `seg_a < seg_b` (caller `idx`).
//!
//! # References
//!
//! * Bentley & Ottmann (1979). IEEE Trans. Computers 28(9).
//! * de Berg et al. (2008). Computational Geometry, 3rd ed. Springer. Ch. 2.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// A directed line segment with a caller-assigned index.
///
/// Endpoints are stored so that `p0.x <= p1.x` (tie-break: `p0.y <= p1.y`).
#[derive(Debug, Clone)]
pub struct Segment {
    /// Caller-assigned identifier returned verbatim in [`IntersectionPoint`].
    pub idx: usize,
    /// Left endpoint (lower x; tie-break: lower y).
    pub p0: (f64, f64),
    /// Right endpoint (higher x; tie-break: higher y).
    pub p1: (f64, f64),
}

impl Segment {
    /// Construct a new [`Segment`], normalising endpoint order.
    pub fn new(idx: usize, a: (f64, f64), b: (f64, f64)) -> Self {
        let (p0, p1) = if (a.0, a.1) <= (b.0, b.1) {
            (a, b)
        } else {
            (b, a)
        };
        Self { idx, p0, p1 }
    }

    /// Return `true` when this segment is effectively vertical (`|dx| < 1e-12`).
    fn is_vertical(&self) -> bool {
        (self.p1.0 - self.p0.0).abs() < 1e-12
    }

    /// The y-extent of this segment as `(y_min, y_max)`.
    fn y_range(&self) -> (f64, f64) {
        if self.p0.1 <= self.p1.1 {
            (self.p0.1, self.p1.1)
        } else {
            (self.p1.1, self.p0.1)
        }
    }
}

/// A single pairwise intersection discovered by the sweep.
///
/// `seg_a < seg_b` is always guaranteed (caller `idx`).
#[derive(Debug, Clone)]
pub struct IntersectionPoint {
    /// X coordinate of the intersection.
    pub x: f64,
    /// Y coordinate of the intersection.
    pub y: f64,
    /// Index of the first segment (always `< seg_b`).
    pub seg_a: usize,
    /// Index of the second segment (always `> seg_a`).
    pub seg_b: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: total-order f64 wrapper
// ─────────────────────────────────────────────────────────────────────────────

/// Total-order wrapper around `f64` using `total_cmp` — never panics on NaN.
#[derive(Debug, Clone, Copy)]
struct OrdF64(f64);

impl PartialEq for OrdF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}
impl Eq for OrdF64 {}
impl PartialOrd for OrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrdF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: event types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum EventKind {
    /// Left endpoint — insert segment. Priority 0 (processed first).
    Insert = 0,
    /// Crossing — swap two segments. Priority 1.
    Cross = 1,
    /// Right endpoint — remove segment. Priority 2 (processed last).
    Remove = 2,
}

#[derive(Debug, Clone)]
struct Event {
    x: f64,
    y: f64,
    kind: EventKind,
    seg_a: usize,
    seg_b: Option<usize>,
}

impl Event {
    /// Comparison key: (x asc, kind priority asc, y asc).
    fn sort_key(&self) -> (OrdF64, u8, OrdF64) {
        (OrdF64(self.x), self.kind.clone() as u8, OrdF64(self.y))
    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.sort_key() == other.sort_key()
    }
}
impl Eq for Event {}
impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: geometry helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Y-coordinate of `seg` at x-position `x`.
///
/// For vertical segments, returns the midpoint y (a stable proxy key).
fn y_at_x(seg: &Segment, x: f64) -> f64 {
    let dx = seg.p1.0 - seg.p0.0;
    if dx.abs() < 1e-14 {
        (seg.p0.1 + seg.p1.1) * 0.5
    } else {
        let t = (x - seg.p0.0) / dx;
        seg.p0.1 + t * (seg.p1.1 - seg.p0.1)
    }
}

/// Parametric segment-segment intersection.
///
/// Returns `Some((x, y))` when t, u ∈ [0, 1] with ε = 1e-10 tolerance.
/// Returns `None` for parallel/collinear pairs or when segments don't overlap.
fn segment_intersect(
    (ax, ay): (f64, f64),
    (bx, by): (f64, f64),
    (cx, cy): (f64, f64),
    (dx, dy): (f64, f64),
) -> Option<(f64, f64)> {
    let r_x = bx - ax;
    let r_y = by - ay;
    let s_x = dx - cx;
    let s_y = dy - cy;
    let det = r_x * s_y - r_y * s_x;
    if det.abs() < 1e-10 {
        return None;
    }
    let q_x = cx - ax;
    let q_y = cy - ay;
    let t = (q_x * s_y - q_y * s_x) / det;
    let u = (q_x * r_y - q_y * r_x) / det;
    let eps = 1e-10;
    if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
        Some((ax + t * r_x, ay + t * r_y))
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: status structure
// ─────────────────────────────────────────────────────────────────────────────

/// Sweep-line status: sorted list of active segment indices by y-at-current-x.
///
/// A `Vec` is used rather than a self-balancing BST because we need
/// predecessor/successor by position and O(1) swaps.  The list is kept sorted
/// by lazily re-sorting at each event using fresh y-at-x values.
struct Status<'a> {
    active: Vec<usize>,
    segs: &'a [Segment],
    x: f64,
}

impl<'a> Status<'a> {
    fn new(segs: &'a [Segment]) -> Self {
        Self {
            active: Vec::new(),
            segs,
            x: 0.0,
        }
    }

    fn y_key(&self, idx: usize) -> f64 {
        y_at_x(&self.segs[idx], self.x)
    }

    /// Insert `seg_idx` maintaining sorted order by y-at-current-x.
    fn insert(&mut self, seg_idx: usize) {
        let y = self.y_key(seg_idx);
        let pos = self
            .active
            .partition_point(|&s| self.y_key(s).total_cmp(&y) != std::cmp::Ordering::Greater);
        self.active.insert(pos, seg_idx);
    }

    /// Re-sort the entire active list by y-at-current-x.
    fn resort(&mut self) {
        let segs = self.segs;
        let x = self.x;
        self.active
            .sort_unstable_by(|&a, &b| y_at_x(&segs[a], x).total_cmp(&y_at_x(&segs[b], x)));
    }

    /// Remove segment `seg_idx`. Returns `true` if present.
    fn remove(&mut self, seg_idx: usize) -> bool {
        if let Some(p) = self.active.iter().position(|&s| s == seg_idx) {
            self.active.remove(p);
            true
        } else {
            false
        }
    }

    fn position_of(&self, seg_idx: usize) -> Option<usize> {
        self.active.iter().position(|&s| s == seg_idx)
    }

    fn predecessor_at(&self, pos: usize) -> Option<usize> {
        if pos > 0 {
            Some(self.active[pos - 1])
        } else {
            None
        }
    }

    fn successor_at(&self, pos: usize) -> Option<usize> {
        self.active.get(pos + 1).copied()
    }

    /// All active segment indices whose y-at-current-x lies within `[y_lo, y_hi]`.
    fn segments_in_y_range(&self, y_lo: f64, y_hi: f64, exclude: usize) -> Vec<usize> {
        self.active
            .iter()
            .copied()
            .filter(|&s| {
                if s == exclude {
                    return false;
                }
                let y = self.y_key(s);
                y >= y_lo - 1e-9 && y <= y_hi + 1e-9
            })
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: scheduling helper
// ─────────────────────────────────────────────────────────────────────────────

/// Try to schedule a Cross event for (a_idx, b_idx) if they intersect at
/// x >= sweep_x and the pair isn't already scheduled or reported.
fn maybe_schedule_cross(
    a_idx: usize,
    b_idx: usize,
    segments: &[Segment],
    sweep_x: f64,
    heap: &mut BinaryHeap<Reverse<Event>>,
    scheduled: &mut HashSet<(usize, usize)>,
    reported: &HashSet<(usize, usize)>,
) {
    let key = (a_idx.min(b_idx), a_idx.max(b_idx));
    if scheduled.contains(&key) || reported.contains(&key) {
        return;
    }
    let sa = &segments[a_idx];
    let sb = &segments[b_idx];
    if let Some((ix, iy)) = segment_intersect(sa.p0, sa.p1, sb.p0, sb.p1)
        && ix >= sweep_x - 1e-10
    {
        scheduled.insert(key);
        heap.push(Reverse(Event {
            x: ix,
            y: iy,
            kind: EventKind::Cross,
            seg_a: a_idx,
            seg_b: Some(b_idx),
        }));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Find all pairwise intersections among `segments` using Bentley-Ottmann.
///
/// # Guarantees
///
/// * Each intersecting pair is reported **exactly once**.
/// * `result[i].seg_a < result[i].seg_b` (caller `idx`).
/// * Collinear overlapping segments are **not** reported.
///
/// # Complexity
///
/// O((n + k) log n) for inputs in general position.  Near-vertical segments
/// trigger an O(n) scan at their insert event, so worst-case complexity for
/// inputs with many near-vertical segments is O(n²).
pub fn find_all_intersections(segments: &[Segment]) -> Vec<IntersectionPoint> {
    if segments.is_empty() {
        return Vec::new();
    }

    // ── Build initial event queue ─────────────────────────────────────────────
    let mut heap: BinaryHeap<Reverse<Event>> = BinaryHeap::new();

    for (i, seg) in segments.iter().enumerate() {
        let (p0, p1) = if (seg.p0.0, seg.p0.1) <= (seg.p1.0, seg.p1.1) {
            (seg.p0, seg.p1)
        } else {
            (seg.p1, seg.p0)
        };
        heap.push(Reverse(Event {
            x: p0.0,
            y: p0.1,
            kind: EventKind::Insert,
            seg_a: i,
            seg_b: None,
        }));
        heap.push(Reverse(Event {
            x: p1.0,
            y: p1.1,
            kind: EventKind::Remove,
            seg_a: i,
            seg_b: None,
        }));
    }

    // ── Sweep ─────────────────────────────────────────────────────────────────
    let mut status = Status::new(segments);
    let mut results: Vec<IntersectionPoint> = Vec::new();
    let mut reported: HashSet<(usize, usize)> = HashSet::new();
    let mut scheduled: HashSet<(usize, usize)> = HashSet::new();
    let mut pending: Vec<Event> = Vec::new();

    while !heap.is_empty() || !pending.is_empty() {
        // Determine the current batch x.
        let batch_x = if !pending.is_empty() {
            pending[0].x
        } else {
            heap.peek().map(|Reverse(e)| e.x).unwrap_or(f64::INFINITY)
        };

        // Collect all events at `batch_x` from the heap.
        while let Some(Reverse(ev)) = heap.peek() {
            if (ev.x - batch_x).abs() < 1e-12 {
                if let Some(Reverse(ev)) = heap.pop() {
                    pending.push(ev);
                }
            } else {
                break;
            }
        }

        // Sort the batch: Insert < Cross < Remove, then by y.
        pending.sort_unstable_by_key(|a| a.sort_key());

        // Set the sweep x for this batch.
        status.x = batch_x;

        let batch = std::mem::take(&mut pending);

        for ev in &batch {
            match ev.kind {
                EventKind::Insert => {
                    let seg = &segments[ev.seg_a];
                    status.insert(ev.seg_a);
                    // Re-sort to ensure consistent ordering after insertion
                    // (important when multiple segments are inserted at the same x).
                    status.resort();

                    if seg.is_vertical() {
                        // Vertical segment: intersects ALL active segments whose
                        // y-at-current-x lies within the vertical's y-range.
                        let (y_lo, y_hi) = seg.y_range();
                        let candidates = status.segments_in_y_range(y_lo, y_hi, ev.seg_a);
                        for cand in candidates {
                            maybe_schedule_cross(
                                ev.seg_a,
                                cand,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                    } else {
                        // Normal segment: check adjacent neighbours only.
                        let Some(pos) = status.position_of(ev.seg_a) else {
                            continue;
                        };
                        if let Some(pred) = status.predecessor_at(pos) {
                            maybe_schedule_cross(
                                ev.seg_a,
                                pred,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                        if let Some(succ) = status.successor_at(pos) {
                            maybe_schedule_cross(
                                ev.seg_a,
                                succ,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                    }
                }

                EventKind::Remove => {
                    let pos = status.position_of(ev.seg_a);
                    if let Some(pos) = pos {
                        let pred = status.predecessor_at(pos);
                        let succ = status.successor_at(pos);
                        if let (Some(p), Some(s)) = (pred, succ) {
                            maybe_schedule_cross(
                                p,
                                s,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                    }
                    status.remove(ev.seg_a);
                }

                EventKind::Cross => {
                    let a_idx = ev.seg_a;
                    let b_idx = match ev.seg_b {
                        Some(b) => b,
                        None => continue,
                    };

                    let sched_key = (a_idx.min(b_idx), a_idx.max(b_idx));
                    scheduled.remove(&sched_key);

                    // Report intersection.
                    let sa = &segments[a_idx];
                    let sb = &segments[b_idx];
                    if let Some((ix, iy)) = segment_intersect(sa.p0, sa.p1, sb.p0, sb.p1) {
                        let rep_key = (sa.idx.min(sb.idx), sa.idx.max(sb.idx));
                        if !reported.contains(&rep_key) {
                            reported.insert(rep_key);
                            results.push(IntersectionPoint {
                                x: ix,
                                y: iy,
                                seg_a: rep_key.0,
                                seg_b: rep_key.1,
                            });
                        }
                    }

                    // After crossing, advance sweep x slightly so resort reflects
                    // the swapped ordering, then check new outer neighbours.
                    status.x = batch_x + 1e-9;
                    status.resort();
                    status.x = batch_x;

                    let pos_a = status.position_of(a_idx);
                    let pos_b = status.position_of(b_idx);

                    if let Some(pa) = pos_a {
                        if let Some(pred) = status.predecessor_at(pa)
                            && pred != b_idx
                        {
                            maybe_schedule_cross(
                                a_idx,
                                pred,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                        if let Some(succ) = status.successor_at(pa)
                            && succ != b_idx
                        {
                            maybe_schedule_cross(
                                a_idx,
                                succ,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                    }
                    if let Some(pb) = pos_b {
                        if let Some(pred) = status.predecessor_at(pb)
                            && pred != a_idx
                        {
                            maybe_schedule_cross(
                                b_idx,
                                pred,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                        if let Some(succ) = status.successor_at(pb)
                            && succ != a_idx
                        {
                            maybe_schedule_cross(
                                b_idx,
                                succ,
                                segments,
                                batch_x,
                                &mut heap,
                                &mut scheduled,
                                &reported,
                            );
                        }
                    }
                }
            }
        }

        // After processing the batch, do a final resort with a slightly
        // advanced x so that segments that just crossed are in correct order
        // for the next batch.
        status.x = batch_x + 1e-9;
        status.resort();
        // Restore batch_x so future events using this x are not confused.
        status.x = batch_x;
    }

    results
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_new_normalises_order() {
        let s = Segment::new(0, (3.0, 4.0), (1.0, 2.0));
        assert_eq!(s.p0, (1.0, 2.0));
        assert_eq!(s.p1, (3.0, 4.0));
    }

    #[test]
    fn segment_intersect_cross() {
        let pt = segment_intersect((-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0));
        let (x, y) = pt.unwrap();
        assert!((x).abs() < 1e-9, "x = {x}");
        assert!((y).abs() < 1e-9, "y = {y}");
    }

    #[test]
    fn segment_intersect_parallel_returns_none() {
        let pt = segment_intersect((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0));
        assert!(pt.is_none());
    }

    #[test]
    fn y_at_x_horizontal() {
        let s = Segment::new(0, (0.0, 5.0), (10.0, 5.0));
        assert!((y_at_x(&s, 5.0) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn ord_f64_total_order_nan() {
        let a = OrdF64(1.0);
        let b = OrdF64(2.0);
        let c = OrdF64(f64::NAN);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn vertical_segment_detected() {
        let s = Segment::new(0, (5.0, 0.0), (5.0, 10.0));
        assert!(s.is_vertical());
        assert_eq!(s.y_range(), (0.0, 10.0));
    }
}
