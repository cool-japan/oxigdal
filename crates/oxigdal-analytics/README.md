# oxigdal-analytics

[![Crates.io](https://img.shields.io/crates/v/oxigdal-analytics.svg)](https://crates.io/crates/oxigdal-analytics)
[![Documentation](https://docs.rs/oxigdal-analytics/badge.svg)](https://docs.rs/oxigdal-analytics)
[![License](https://img.shields.io/crates/l/oxigdal-analytics.svg)](LICENSE)

Advanced geospatial analytics for enterprise workflows — part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) ecosystem.

## Features

- **Hotspot Analysis**: Getis-Ord Gi* and Moran's I spatial autocorrelation
- **Spatial Clustering**: K-means and DBSCAN for image classification and outlier detection
- **Time Series Analysis**: Trend detection (Mann-Kendall), anomaly detection, seasonal decomposition
- **Change Detection**: CVA (Change Vector Analysis), PCA-based, and multi-temporal analysis
- **Interpolation**: IDW (Inverse Distance Weighting) and Kriging for spatial surface generation
- **Zonal Statistics**: Weighted and multi-band zonal statistics by polygon zones
- **Performance Profiling**: Built-in timing and throughput metrics

## Installation

```toml
[dependencies]
oxigdal-analytics = "0.1.3"
```

## Usage

### Hotspot Analysis (Getis-Ord Gi*)

```rust
use oxigdal_analytics::spatial::{getis_ord_gi_star, SpatialWeights};

let values = vec![1.0, 2.5, 8.0, 9.5, 2.0, 1.5];
let weights = SpatialWeights::queen_contiguity(&geometries)?;
let hotspots = getis_ord_gi_star(&values, &weights, 0.05)?;

for h in &hotspots {
    println!("Cell {}: z={:.3}, p={:.4}, cluster={:?}", h.id, h.z_score, h.p_value, h.cluster_type);
}
```

### Spatial Clustering

```rust
use oxigdal_analytics::clustering::{KMeans, Dbscan};

// K-means for land cover classification
let kmeans = KMeans::new(5, 100)?;  // 5 classes, 100 iterations
let labels = kmeans.fit_predict(&pixel_features)?;

// DBSCAN for hotspot outlier detection
let dbscan = Dbscan::new(0.5, 3)?;  // eps=0.5, min_points=3
let clusters = dbscan.fit_predict(&point_features)?;
```

### Time Series Trend Detection

```rust
use oxigdal_analytics::timeseries::{detect_trend, TrendMethod};

// Mann-Kendall trend test on NDVI time series
let trend = detect_trend(&ndvi_series, TrendMethod::MannKendall)?;
println!("Trend: {:?}, tau={:.3}, p={:.4}", trend.direction, trend.tau, trend.p_value);
```

### Change Detection

```rust
use oxigdal_analytics::change::{detect_changes, ChangeMethod};

// Change Vector Analysis between two epochs
let changes = detect_changes(&before, &after, ChangeMethod::CVA)?;
println!("Changed pixels: {}/{}", changes.changed_count(), changes.total());
```

### Interpolation

```rust
use oxigdal_analytics::interpolation::{idw, Kriging, KrigingModel};

// IDW interpolation from sparse sample points
let grid = idw(&sample_points, &sample_values, &target_grid, power: 2.0)?;

// Ordinary Kriging
let kriging = Kriging::new(KrigingModel::Spherical)?;
kriging.fit(&sample_points, &sample_values)?;
let surface = kriging.predict(&target_grid)?;
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `parallel` | no | Rayon-based parallelism for large datasets |
| `simd` | no | SIMD-accelerated statistical operations |
| `arrow` | no | Apache Arrow integration for columnar analytics |

## COOLJAPAN Policies

- Pure Rust — no C/Fortran dependencies
- No `unwrap()` — all errors handled via `Result<T, OxiError>`
- Uses SciRS2-Core for scientific computing (not ndarray directly)
- Workspace dependencies via `*.workspace = true`

## License

Apache-2.0 — Copyright (c) COOLJAPAN OU (Team Kitasan)
