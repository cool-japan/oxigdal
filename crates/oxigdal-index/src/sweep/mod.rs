//! Sweep-line algorithms for segment intersection detection.
//!
//! # Modules
//!
//! * `bentley_ottmann` — Bentley-Ottmann O((n+k) log n) algorithm for finding
//!   all pairwise intersections among a set of line segments.

mod bentley_ottmann;
pub use bentley_ottmann::{IntersectionPoint, Segment, find_all_intersections};
