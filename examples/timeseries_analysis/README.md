# Time-Series Analysis Example

Comprehensive temporal raster analysis for multi-temporal satellite imagery and environmental monitoring.

## Features

- **Temporal data cubes**: Organize multi-temporal rasters efficiently
- **Trend analysis**: Linear regression, Mann-Kendall, Sen's slope
- **Seasonality detection**: Decompose seasonal patterns
- **Anomaly detection**: Z-score, IQR, isolation forest methods
- **Change detection**: BFAST, CUSUM, EWMA breakpoint analysis
- **Forecasting**: Exponential smoothing, ARIMA, Prophet
- **Gap filling**: Linear, spline, harmonic interpolation
- **Temporal compositing**: Mean, median, max NDVI composites
- **Export formats**: GeoTIFF, Zarr, NetCDF

## Use Cases

- Agricultural monitoring (crop phenology, yield prediction)
- Forest change detection (deforestation, regrowth)
- Urban growth analysis
- Climate change impact assessment
- Water body dynamics
- Vegetation health monitoring
- Land cover change detection

## Sample Data

### Option 1: Download Landsat Time Series

Use [USGS EarthExplorer](https://earthexplorer.usgs.gov/):

1. Select area of interest
2. Choose date range (e.g., 2020-2024)
3. Select Landsat 8 Collection 2 Level-2
4. Download scenes (16-day revisit cycle)
5. Pre-process to NDVI:

```bash
# Calculate NDVI for each scene
for scene in LC08_*/; do
    gdal_calc.py -A ${scene}/*_SR_B5.TIF -B ${scene}/*_SR_B4.TIF \
        --outfile=data/timeseries/landsat_ndvi_$(date).tif \
        --calc="(A-B)/(A+B)"
done
```

### Option 2: Download MODIS Time Series

[NASA MODIS Data](https://modis.gsfc.nasa.gov/):

```bash
# MOD13Q1 - 16-day NDVI at 250m resolution
# Comes pre-calculated
wget https://e4ftl01.cr.usgs.gov/MOLT/MOD13Q1.061/...
```

### Option 3: Sentinel-2 Time Series

[Copernicus Open Access Hub](https://scihub.copernicus.eu/):

- 5-day revisit (per satellite)
- 10-20m resolution
- Pre-calculate indices from surface reflectance

### Option 4: Generate Synthetic Data

```bash
cargo run --example generate_test_timeseries -- \
    --observations 50 \
    --size 512x512 \
    --start-date 2020-01-01 \
    --frequency weekly \
    --trend linear \
    --seasonality true \
    --anomalies 5
```

## Usage

### Basic Time-Series Analysis

```bash
cargo run --release --example timeseries_analysis
```

### Custom Configuration

Edit `main.rs`:

```rust
let config = AnalysisConfig {
    input_pattern: "data/timeseries/landsat_ndvi_*.tif",

    time_range: TimeRange {
        start: NaiveDate::from_ymd_opt(2020, 1, 1)?,
        end: NaiveDate::from_ymd_opt(2024, 12, 31)?,
    },

    temporal_resolution: TemporalResolution::BiWeekly,
    index_type: SpectralIndex::Ndvi,

    analyses: vec![
        TimeSeriesAnalysis::TrendDetection {
            method: TrendMethod::MannKendall,
        },
        TimeSeriesAnalysis::SeasonalDecomposition {
            period: 12,  // Monthly seasonality
        },
        TimeSeriesAnalysis::AnomalyDetection {
            method: AnomalyMethod::ZScore { threshold: 3.0 },
        },
    ],

    gap_filling: GapFillingConfig {
        enabled: true,
        method: GapFillingMethod::HarmonicInterpolation,
        max_gap_size: 3,
    },

    export_format: ExportFormat::Zarr,
    generate_plots: true,
};
```

## Analysis Types

### 1. Trend Detection

Identify long-term changes:

**Linear Regression:**
- Simple linear fit
- Fast computation
- Assumes linear trend

**Mann-Kendall Test:**
- Non-parametric
- Robust to outliers
- Tests for monotonic trend
- Returns p-value for significance

**Sen's Slope:**
- Median of all pairwise slopes
- Resistant to outliers
- More robust than linear regression

Example output:
```
Trend statistics:
  Pixels with significant trend: 125432/250000 (50.2%)
  Mean slope: 0.002341 per year
  Positive trends: 65.3%
  Negative trends: 34.7%
```

### 2. Seasonality Detection

Decompose time series into components:

- **Trend**: Long-term direction
- **Seasonal**: Repeating patterns
- **Residual**: Unexplained variation

Methods:
- Classical decomposition (additive/multiplicative)
- STL (Seasonal-Trend decomposition using Loess)
- Fourier analysis

Applications:
- Crop phenology
- Snow cover dynamics
- Seasonal rainfall patterns

### 3. Anomaly Detection

Identify unusual observations:

**Z-Score Method:**
```rust
AnomalyMethod::ZScore { threshold: 3.0 }  // 99.7% confidence
```
- Simple and fast
- Assumes normal distribution
- Good for outliers

**IQR (Interquartile Range):**
```rust
AnomalyMethod::Iqr { multiplier: 1.5 }
```
- Non-parametric
- Robust to distribution
- Based on quartiles

**Isolation Forest:**
```rust
AnomalyMethod::IsolationForest
```
- Machine learning approach
- Detects complex anomalies
- No distribution assumptions

Applications:
- Drought detection
- Flood events
- Fire scars
- Cloud contamination

### 4. Change Detection

Detect breakpoints in time series:

**BFAST (Breaks For Additive Season and Trend):**
- Detects multiple breakpoints
- Separates trend and seasonal changes
- Automatic breakpoint detection

**CUSUM (Cumulative Sum):**
- Sequential change detection
- Fast and efficient
- Good for abrupt changes

**EWMA (Exponentially Weighted Moving Average):**
- Weighted recent observations
- Detects gradual changes
- Smooths noise

Applications:
- Deforestation
- Urban expansion
- Land use change
- Recovery after disturbance

### 5. Forecasting

Predict future values:

**Exponential Smoothing:**
```rust
ForecastMethod::ExponentialSmoothing
```
- Simple and fast
- Works well for short-term
- No complex assumptions

**ARIMA (AutoRegressive Integrated Moving Average):**
```rust
ForecastMethod::Arima { p: 1, d: 1, q: 1 }
```
- Classical time series model
- Handles trends and seasonality
- Requires parameter tuning

**Prophet (Facebook):**
```rust
ForecastMethod::Prophet
```
- Handles multiple seasonalities
- Robust to missing data
- Automatic parameter selection

Applications:
- Crop yield prediction
- Drought forecasting
- Vegetation growth projection

## Gap Filling

Handle missing observations:

### Linear Interpolation
```rust
GapFillingMethod::LinearInterpolation
```
- Fast and simple
- Good for short gaps
- Assumes constant rate of change

### Spline Interpolation
```rust
GapFillingMethod::SplineInterpolation
```
- Smooth curves
- Better for longer gaps
- More realistic transitions

### Harmonic Interpolation
```rust
GapFillingMethod::HarmonicInterpolation
```
- Captures seasonal patterns
- Best for regular gaps (e.g., cloud cover)
- Preserves phenology

Example:
```
Found 15 temporal gaps
  Gap: 2021-03-15 to 2021-04-12
  Gap: 2022-07-08 to 2022-07-24
  ...
Filled 15 gaps
```

## Temporal Compositing

Reduce data volume while preserving information:

### Median Composite
```rust
CompositeMethod::Median
```
- Removes outliers
- Cloud-resistant
- Most common for satellite data

### Mean Composite
```rust
CompositeMethod::Mean
```
- Smooth average
- Reduces noise
- Better for continuous surfaces

### Max NDVI Composite
```rust
CompositeMethod::MaxNdvi
```
- Selects greenest pixel
- Standard for vegetation monitoring
- Minimizes cloud/shadow effects

### Max Value Composite
```rust
CompositeMethod::Max
```
- Highest value per pixel
- Good for snow/water detection

Example:
```
Generating temporal composites
  Window size: 30 days
  Method: Median
Created 48 composite images (monthly for 4 years)
```

## Export Formats

### GeoTIFF
```rust
export_format: ExportFormat::GeoTiff,
```

Outputs individual rasters:
- `trend_slope.tif` - Trend slope values
- `trend_pvalue.tif` - Statistical significance
- `seasonal_amplitude.tif` - Seasonal strength
- `change_magnitude.tif` - Change intensity

### Zarr
```rust
export_format: ExportFormat::Zarr,
```

Cloud-native data cube format:
- Chunked storage for efficient access
- Compression support
- Metadata preservation
- Works with Dask, Xarray

Structure:
```
timeseries_analysis.zarr/
├── .zarray
├── .zattrs
├── data_cube/
├── trend_slope/
├── trend_pvalue/
└── ...
```

### NetCDF
```rust
export_format: ExportFormat::NetCdf,
```

Scientific data format:
- Self-describing
- Multiple variables
- CF conventions compliant
- Widely supported

## Visualization

### Time Series Plots

Generated automatically when `generate_plots: true`:

**timeseries_plot.png:**
- Mean NDVI over time
- Confidence intervals
- Detected anomalies marked
- Change points highlighted

**trend_map.png:**
- Spatial map of trend slopes
- Red = decreasing, Green = increasing
- Significance overlay

**seasonal_plot.png:**
- Seasonal component visualization
- Amplitude and phase
- By month/season

**anomaly_plot.png:**
- Timeline with anomaly scores
- Threshold lines
- Event annotations

### Custom Visualization

Using Python (Xarray + Matplotlib):

```python
import xarray as xr
import matplotlib.pyplot as plt

# Load Zarr cube
ds = xr.open_zarr('output/timeseries/timeseries_analysis.zarr')

# Plot time series for a pixel
ds.sel(x=100, y=100).plot()

# Plot trend map
ds['trend_slope'].plot()

# Plot seasonal cycle
ds.groupby('time.month').mean().plot()
```

## Performance

Typical processing times (50 observations, 512×512 pixels):

| Analysis | Time | Memory |
|----------|------|--------|
| Load data cube | 15s | 500 MB |
| Gap filling (harmonic) | 8s | 800 MB |
| Trend analysis (Mann-Kendall) | 45s | 600 MB |
| Seasonal decomposition | 25s | 700 MB |
| Anomaly detection (Z-score) | 5s | 400 MB |
| Change detection (BFAST) | 120s | 1.2 GB |
| Forecasting (12 periods) | 180s | 900 MB |

Optimization tips:
- Use Zarr for large datasets
- Process by tiles for huge images
- Parallel processing for independent pixels
- Cloud storage for archives

## Real-World Examples

### Agricultural Monitoring

```rust
// Monitor crop growth over season
let config = AnalysisConfig {
    time_range: TimeRange {
        start: NaiveDate::from_ymd_opt(2024, 4, 1)?,  // Planting
        end: NaiveDate::from_ymd_opt(2024, 10, 31)?,   // Harvest
    },
    temporal_resolution: TemporalResolution::Weekly,
    analyses: vec![
        TimeSeriesAnalysis::SeasonalDecomposition { period: 26 },  // Weekly
        TimeSeriesAnalysis::AnomalyDetection {
            method: AnomalyMethod::ZScore { threshold: 2.5 },
        },
        TimeSeriesAnalysis::Forecasting {
            horizon: 4,  // 4 weeks ahead
            method: ForecastMethod::Prophet,
        },
    ],
    // ...
};
```

### Forest Change Detection

```rust
// Detect deforestation events
let config = AnalysisConfig {
    time_range: TimeRange {
        start: NaiveDate::from_ymd_opt(2020, 1, 1)?,
        end: NaiveDate::from_ymd_opt(2024, 12, 31)?,
    },
    analyses: vec![
        TimeSeriesAnalysis::ChangeDetection {
            method: ChangeMethod::Bfast,
        },
        TimeSeriesAnalysis::TrendDetection {
            method: TrendMethod::SenSlope,
        },
    ],
    // ...
};
```

### Drought Monitoring

```rust
// Track vegetation stress
let config = AnalysisConfig {
    index_type: SpectralIndex::Ndvi,
    analyses: vec![
        TimeSeriesAnalysis::AnomalyDetection {
            method: AnomalyMethod::Iqr { multiplier: 1.5 },
        },
        TimeSeriesAnalysis::TrendDetection {
            method: TrendMethod::MannKendall,
        },
    ],
    compositing: CompositingConfig {
        enabled: true,
        window_size: 30,
        method: CompositeMethod::Median,
    },
    // ...
};
```

## Integration

### With Xarray/Dask (Python)

```python
import xarray as xr
import dask.array as da

# Load Zarr cube
ds = xr.open_zarr('output/timeseries/timeseries_analysis.zarr',
                   chunks={'time': 10, 'x': 256, 'y': 256})

# Compute custom statistics
mean_by_year = ds.groupby('time.year').mean()
std_by_month = ds.groupby('time.month').std()

# Machine learning with scikit-learn
from sklearn.decomposition import PCA

# Reshape for PCA
data = ds['ndvi'].values.reshape(n_time, -1).T
pca = PCA(n_components=3)
components = pca.fit_transform(data)
```

### With Google Earth Engine

```javascript
// Load time series from GEE
var timeseries = ee.ImageCollection('LANDSAT/LC08/C02/T1_L2')
    .filterBounds(roi)
    .filterDate('2020-01-01', '2024-12-31')
    .map(calculateNDVI);

// Export for local analysis
Export.image.toDrive({
    image: timeseries.toBands(),
    description: 'landsat_ndvi_timeseries',
    region: roi,
    scale: 30
});
```

## Troubleshooting

### Insufficient Memory

Reduce spatial extent or use tiling:

```rust
// Process in tiles
for tile in spatial_tiles(4, 4) {  // 4x4 grid
    process_tile(tile)?;
}
```

### Missing Observations

Adjust gap filling parameters:

```rust
gap_filling: GapFillingConfig {
    enabled: true,
    max_gap_size: 5,  // Allow larger gaps
    method: GapFillingMethod::HarmonicInterpolation,
}
```

### Noisy Data

Increase compositing window:

```rust
compositing: CompositingConfig {
    window_size: 60,  // 2-month windows
    method: CompositeMethod::Median,
}
```

## References

- [BFAST](https://bfast2.github.io/) - Breaks For Additive Season and Trend
- [Prophet](https://facebook.github.io/prophet/) - Facebook forecasting
- [Xarray](https://xarray.dev/) - N-D labeled arrays
- [Time Series Analysis (NIST)](https://www.itl.nist.gov/div898/handbook/pmc/section4/pmc4.htm)

## License

Apache-2.0 (COOLJAPAN OU / Team Kitasan)
