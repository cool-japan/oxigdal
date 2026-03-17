# TODO: oxigdal-temporal

## High Priority
- [ ] Implement actual lazy-loading for TimeSeriesRaster (currently loads all in memory)
- [ ] Add cloud-backed time series (stream rasters from S3/GCS via oxigdal-rs3gw)
- [ ] Implement STL (Seasonal-Trend-Loess) decomposition
- [ ] Add CCDC (Continuous Change Detection and Classification) algorithm
- [ ] Implement Zarr-backed DataCube with chunked temporal axis
- [ ] Add parallel pixel-wise processing for trend/change detection

## Medium Priority
- [ ] Implement Whittaker smoother for time series smoothing
- [ ] Add Savitzky-Golay filter for vegetation index time series
- [ ] Implement harmonic regression for phenology extraction
- [ ] Add LandTrendr segmentation algorithm (beyond stub)
- [ ] Implement cumulative sum (CUSUM) change detection
- [ ] Add temporal mosaicking with per-pixel best-observation selection
- [ ] Implement Holt-Winters exponential smoothing forecasting
- [ ] Add NetCDF time dimension read/write support
- [ ] Implement cross-correlation between two time series rasters

## Low Priority / Future
- [ ] Add BFAST Monitor for near-real-time disturbance detection
- [ ] Implement temporal data cube slicing with xarray-like syntax
- [ ] Add time series animation/GIF export
- [ ] Implement temporal resampling between different observation frequencies
- [ ] Add integration with oxigdal-stac for temporal catalog queries
- [ ] Implement multi-sensor fusion (Landsat + Sentinel-2 harmonization)
