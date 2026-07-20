//! Spatial clustering algorithms for point data
//!
//! This module provides various clustering algorithms optimized for geospatial data:
//!
//! - **DBSCAN**: Density-based spatial clustering
//! - **K-means**: Centroid-based partitioning
//! - **Hierarchical**: Agglomerative hierarchical clustering
//!
//! All algorithms handle geographic coordinates and provide specialized distance metrics.

mod dbscan;
mod hierarchical;
mod kmeans;

pub use dbscan::{DbscanOptions, DbscanResult, DistanceMetric, dbscan_cluster};
pub use hierarchical::{
    HierarchicalOptions, HierarchicalResult, LinkageMethod, hierarchical_cluster,
};
pub use kmeans::{InitMethod, KmeansOptions, KmeansResult, kmeans_cluster, kmeans_plus_plus_init};
