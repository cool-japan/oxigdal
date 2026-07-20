//! Spatial clustering algorithms for `oxigeo-index`.
//!
//! Currently provides:
//!
//! * [`dbscan`] — R-tree-accelerated DBSCAN (density-based clustering of
//!   points with noise).  Replaces the O(N²) brute-force range query with
//!   an R-tree window prefilter + exact Euclidean distance check, achieving
//!   O(N log N) average-case performance.

pub mod dbscan;

pub use dbscan::{
    ClusterLabel, DbscanOptions, DbscanResult, NOISE, UNVISITED, dbscan_rtree, dbscan_with_rtree,
    range_query_eps,
};
