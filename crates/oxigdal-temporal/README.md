# OxiGDAL Temporal

Multi-temporal raster analysis for OxiGDAL - Pure Rust implementation for time series analysis, change detection, and phenology.

## Features

- **Time Series Raster Collections**: Time-indexed raster management with lazy loading
- **Temporal Compositing**: Maximum value composite (MVC), median, mean, quality-weighted
- **Temporal Interpolation**: Linear, cubic spline, seasonal, gap filling
- **Temporal Aggregation**: Daily, weekly, monthly, yearly, and rolling window aggregations
- **Change Detection**: BFAST, LandTrendr, CCDC, simple differencing, CUSUM
- **Trend Analysis**: Linear trends, Mann-Kendall test, Sen's slope estimator
- **Phenology Analysis**: Growing season detection, peak timing, amplitude calculation
- **Data Cube Operations**: Multi-dimensional (x, y, time, bands) operations with Zarr integration

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-temporal = "0.1"

# Enable specific features
oxigdal-temporal = { version = "0.1", features = ["all_temporal"] }
```

## Usage Examples

### Creating a Time Series

```rust
use oxigdal_temporal::timeseries::{TimeSeriesRaster, TemporalMetadata};
use chrono::{DateTime, NaiveDate, Utc};
use scirs2_core::ndarray::Array3;

let mut ts = TimeSeriesRaster::new();

let dt = DateTime::from_timestamp(1640995200, 0)?;
let date = NaiveDate::from_ymd_opt(2022, 1, 1)?;
let metadata = TemporalMetadata::new(dt, date)
    .with_cloud_cover(15.0)
    .with_quality_score(0.95);

let data = Array3::zeros((100, 100, 3));
ts.add_raster(metadata, data)?;
```

### Temporal Compositing

```rust
use oxigdal_temporal::compositing::{TemporalCompositor, CompositingConfig, CompositingMethod};

let config = CompositingConfig {
    method: CompositingMethod::Median,
    max_cloud_cover: Some(20.0),
    min_quality: Some(0.7),
    ..Default::default()
};

let composite = TemporalCompositor::composite(&ts, &config)?;
println!("Composite shape: {:?}", composite.composite.shape());
```

### Change Detection

```rust
use oxigdal_temporal::change::{ChangeDetector, ChangeDetectionConfig, ChangeDetectionMethod};

let config = ChangeDetectionConfig {
    method: ChangeDetectionMethod::SimpleDifference,
    threshold: Some(0.1),
    ..Default::default()
};

let changes = ChangeDetector::detect(&ts, &config)?;
println!("Change magnitude: {:?}", changes.magnitude.shape());
```

### Temporal Aggregation

```rust
use oxigdal_temporal::aggregation::{
    TemporalAggregator, AggregationConfig, TemporalWindow, AggregationStatistic
};

let config = AggregationConfig {
    window: TemporalWindow::Monthly,
    statistics: vec![
        AggregationStatistic::Mean,
        AggregationStatistic::Max,
        AggregationStatistic::StdDev,
    ],
    ..Default::default()
};

let result = TemporalAggregator::aggregate(&ts, &config)?;
let monthly_mean = result.get("Mean")
    .ok_or(TemporalError::StatisticNotFound("Mean".into()))?;
```

### Trend Analysis

```rust
use oxigdal_temporal::trend::{TrendAnalyzer, TrendMethod};

let trend = TrendAnalyzer::analyze(&ts, TrendMethod::Linear)?;
println!("Slope: {:?}", trend.slope[[50, 50, 0]]);
println!("Direction: {:?}", trend.direction[[50, 50, 0]]);
```

### Phenology Extraction

```rust
use oxigdal_temporal::phenology::{PhenologyExtractor, PhenologyConfig};

let config = PhenologyConfig::default();
let metrics = PhenologyExtractor::extract(&ts, &config)?;

println!("Growing season start: {:?}", metrics.season_start[[50, 50, 0]]);
println!("Peak time: {:?}", metrics.peak_time[[50, 50, 0]]);
println!("Amplitude: {:?}", metrics.amplitude[[50, 50, 0]]);
```

### Data Cube Operations

```rust
use oxigdal_temporal::datacube::DataCube;

let cube = DataCube::from_stack(stack, time_coords)?;

// Subset operations
let temporal_subset = cube.select_time_range(0, 10)?;
let band_subset = cube.select_bands(&[0, 1, 2])?;
let spatial_subset = cube.spatial_subset(0, 100, 0, 100)?;

// Apply temporal function
let mean = cube.apply_temporal(|values| {
    values.iter().sum::<f64>() / values.len() as f64
})?;
```

## Features

Enable specific features as needed:

- `timeseries` - Time series raster collections
- `compositing` - Temporal compositing
- `interpolation` - Temporal interpolation
- `aggregation` - Temporal aggregation
- `change_detection` - Change detection
- `trend_analysis` - Trend analysis
- `phenology` - Phenology extraction
- `datacube` - Data cube operations
- `zarr` - Zarr storage integration
- `parallel` - Parallel processing with rayon
- `all_temporal` - Enable all temporal features

## Performance

- Zero-copy operations where possible
- Lazy loading for large time series
- Optional parallel processing with rayon
- SIMD optimizations for statistical operations
- Efficient memory management with ndarray

## COOLJAPAN Policies

This crate follows COOLJAPAN standards:
- Pure Rust implementation (no C/Fortran dependencies by default)
- No `unwrap()` or `panic!` in production code
- Comprehensive error handling with `thiserror`
- Uses SciRS2-Core for scientific computing
- Integration with oxigdal-zarr for cloud-native storage
- Files kept under 2000 lines for maintainability

## License

Apache-2.0

## Contributing

Contributions welcome! This crate is part of the OxiGDAL ecosystem.

## Authors

COOLJAPAN OU (Team Kitasan)
