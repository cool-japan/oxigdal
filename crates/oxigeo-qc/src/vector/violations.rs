//! Topology violation types for vector quality control.
//!
//! This module defines the [`TopologyViolation`] enum and [`TopologyOptions`] struct
//! used by the real topology validation engine to report detected OGC Simple Features
//! constraint violations.

/// A single topology rule violation detected in a feature collection.
#[derive(Debug, Clone, PartialEq)]
pub enum TopologyViolation {
    /// A `LineString` or polygon ring intersects itself.
    SelfIntersection {
        /// Numeric feature identifier (index-based if the feature has no explicit id).
        feature_id: u64,
        /// Pairs of non-adjacent segment indices that cross.
        /// Each tuple `(i, j)` means segment `i` (from coord `i` to `i+1`) crosses
        /// segment `j` (from coord `j` to `j+1`), where `j > i + 1`.
        segments: Vec<(usize, usize)>,
    },

    /// A polygon ring has the wrong winding order.
    RingOrientation {
        /// Feature identifier.
        feature_id: u64,
        /// Ring index: 0 = exterior, 1+ = hole.
        ring_index: usize,
        /// `true` = the ring was expected to be CCW (exterior), `false` = expected CW (hole).
        expected_ccw: bool,
    },

    /// A polygon ring is not properly closed (first ≠ last within tolerance).
    UnclosedRing {
        /// Feature identifier.
        feature_id: u64,
        /// Ring index: 0 = exterior, 1+ = hole.
        ring_index: usize,
    },

    /// Two polygon features overlap (their interiors share an area).
    Overlap {
        /// First feature identifier.
        feature_a: u64,
        /// Second feature identifier.
        feature_b: u64,
        /// Estimated area of the overlap (bbox-intersection area as lower bound).
        area: f64,
    },

    /// Two adjacent polygon features have a gap between them.
    Gap {
        /// First feature identifier.
        feature_a: u64,
        /// Second feature identifier.
        feature_b: u64,
        /// Estimated area of the gap (0.0 = unknown, full gap geometry too expensive).
        area: f64,
    },

    /// Two distinct `LineString` features cross each other (as opposed to a
    /// single geometry's self-intersection, which is [`Self::SelfIntersection`]).
    Crossing {
        /// First feature identifier.
        feature_a: u64,
        /// Second feature identifier.
        feature_b: u64,
    },
}

/// Configuration options for [`super::topology`] rule checking.
#[derive(Debug, Clone)]
pub struct TopologyOptions {
    /// Tolerance used for ring-closure and gap checks.  Default: `1e-9`.
    pub tolerance: f64,
    /// Enable R6 overlap detection (default `true`; set to `false` when the
    /// caller's [`super::topology::TopologyConfig::topology_rules`] does not
    /// include [`super::topology::TopologyRule::MustNotOverlap`], so a
    /// caller who explicitly disabled overlap checking doesn't silently get
    /// overlap violations anyway).
    pub detect_overlaps: bool,
    /// Enable R8 line-crossing detection between distinct `LineString`
    /// features (default `true`; set to `false` when the caller's
    /// [`super::topology::TopologyConfig::topology_rules`] does not include
    /// [`super::topology::TopologyRule::MustNotCross`]).
    pub detect_crossings: bool,
    /// Enable R5 gap detection (expensive — default `false`).
    pub detect_gaps: bool,
    /// Enable R7 dangling-endpoint detection (expensive — default `false`).
    pub detect_dangles: bool,
}

impl Default for TopologyOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-9,
            detect_overlaps: true,
            detect_crossings: true,
            detect_gaps: false,
            detect_dangles: false,
        }
    }
}
