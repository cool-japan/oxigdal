# OxiGDAL Examples

Comprehensive, production-ready examples demonstrating real-world geospatial workflows with OxiGDAL.

## Overview

This directory contains 5 complete example applications covering the most common geospatial data processing scenarios:

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[satellite_processing](satellite_processing/)** | Real-world satellite data processing | Atmospheric correction, spectral indices, pan-sharpening, change detection |
| **[cog_pipeline](cog_pipeline/)** | COG generation at scale | Compression comparison, validation, parallel processing, cloud upload |
| **[vector_postgis](vector_postgis/)** | Advanced spatial vector analysis | PostGIS integration, spatial joins, hotspot analysis, network analysis |
| **[timeseries_analysis](timeseries_analysis/)** | Multi-temporal raster analysis | Trend detection, seasonality, anomaly detection, forecasting |
| **[ml_inference](ml_inference/)** | Machine learning inference | ONNX models, segmentation, classification, GPU acceleration |

## Quick Start

### Prerequisites

```bash
# Rust 1.85+
rustup update

# Optional: PostGIS for vector example
docker run -d --name postgis -p 5432:5432 \
    -e POSTGRES_PASSWORD=postgres postgis/postgis:15-3.3

# Optional: Sample data
mkdir -p data
# See individual example READMEs for data download instructions
```

### Run an Example

```bash
# Navigate to example directory
cd examples/satellite_processing

# Run with default configuration
cargo run --release

# Or run directly from workspace root
cargo run --release --example satellite_processing
```

## Example Details

### 1. Satellite Processing

**Path:** `satellite_processing/`

Process Landsat 8/9 or Sentinel-2 satellite imagery with production-grade corrections and analysis.

**What you'll learn:**
- Loading multi-band satellite imagery
- Radiometric calibration (DN to TOA reflectance)
- Atmospheric correction (DOS1 method)
- Calculating spectral indices (NDVI, NDWI, EVI, SAVI)
- Cloud masking from QA bands
- Pan-sharpening for enhanced resolution
- Exporting as Cloud Optimized GeoTIFFs
- Uploading results to S3

**Key technologies:**
- `oxigdal-sensors` - Satellite sensor support
- `oxigdal-algorithms` - Raster calculations
- `oxigdal-geotiff` - COG export
- `oxigdal-cloud` - S3 integration

**Time to run:** ~2-5 minutes (full Landsat scene)

**[Read full documentation →](satellite_processing/README.md)**

---

### 2. COG Pipeline

**Path:** `cog_pipeline/`

Convert any geospatial raster to Cloud Optimized GeoTIFF at scale with intelligent optimization.

**What you'll learn:**
- Multi-format input support (GeoTIFF, JPEG2000, HDF5, NetCDF)
- Automatic tile size optimization
- Compression algorithm comparison
- COG validation against specification
- Parallel batch processing
- Direct cloud storage upload
- Performance profiling

**Key technologies:**
- `oxigdal-geotiff` - COG creation
- `oxigdal-qc` - Validation
- `oxigdal-cloud` - S3/Azure/GCS upload
- `rayon` - Parallel processing

**Time to run:** ~30 seconds per file (varies by size)

**[Read full documentation →](cog_pipeline/README.md)**

---

### 3. Vector Analysis with PostGIS

**Path:** `vector_postgis/`

Advanced spatial vector analysis using PostGIS database integration.

**What you'll learn:**
- Connecting to PostGIS database
- Importing multi-format vector data
- Creating spatial indexes
- Topology validation and repair
- Proximity analysis
- Spatial joins (intersects, contains, within)
- Buffer analysis
- Hotspot detection (Getis-Ord Gi*)
- Network analysis (shortest path, service areas)
- Exporting to GeoJSON/GeoParquet

**Key technologies:**
- `oxigdal-postgis` - Database integration
- `oxigdal-algorithms` - Hotspot analysis
- `oxigdal-geojson` - GeoJSON I/O
- `oxigdal-geoparquet` - Efficient vector storage

**Time to run:** ~1-3 minutes (100k features)

**[Read full documentation →](vector_postgis/README.md)**

---

### 4. Time-Series Analysis

**Path:** `timeseries_analysis/`

Comprehensive temporal raster analysis for change detection and forecasting.

**What you'll learn:**
- Building temporal data cubes
- Trend analysis (Linear, Mann-Kendall, Sen's slope)
- Seasonal decomposition
- Anomaly detection (Z-score, IQR, isolation forest)
- Change detection (BFAST, CUSUM)
- Time-series forecasting (ARIMA, Prophet)
- Gap filling (linear, spline, harmonic interpolation)
- Temporal compositing (median, mean, max NDVI)
- Exporting as Zarr/NetCDF

**Key technologies:**
- `oxigdal-temporal` - Time-series operations
- `oxigdal-zarr` - Cloud-native data cubes
- `oxigdal-analytics` - Change detection
- `scirs2-core` - Statistical analysis

**Time to run:** ~2-10 minutes (50 observations, 512×512)

**[Read full documentation →](timeseries_analysis/README.md)**

---

### 5. ML Inference with ONNX

**Path:** `ml_inference/`

Run machine learning models on geospatial data with ONNX Runtime.

**What you'll learn:**
- Loading pre-trained ONNX models
- Preprocessing for ML input (normalization, padding)
- Tiled processing for large images
- Running inference (segmentation, classification, detection)
- GPU acceleration (CUDA, TensorRT, DirectML, CoreML)
- Postprocessing (confidence filtering, smoothing)
- Vectorizing raster predictions
- Performance profiling

**Key technologies:**
- `oxigdal-ml` - ONNX integration
- `ort` - ONNX Runtime
- `oxigdal-algorithms` - Image processing
- `oxigdal-geojson` - Vector export

**Time to run:** ~10 seconds (GPU), ~3 minutes (CPU) for 10k×10k image

**[Read full documentation →](ml_inference/README.md)**

---

## Sample Data

Each example includes instructions for obtaining sample data:

### Quick Test Data

Generate synthetic data for testing:

```bash
# Satellite data
cargo run --example generate_test_satellite_data

# Time-series
cargo run --example generate_test_timeseries

# Vector data
cargo run --example generate_test_vector_data
```

### Real-World Data Sources

**Satellite Imagery:**
- [USGS EarthExplorer](https://earthexplorer.usgs.gov/) - Landsat, Sentinel
- [Copernicus Open Access Hub](https://scihub.copernicus.eu/) - Sentinel-2
- [NASA MODIS](https://modis.gsfc.nasa.gov/) - Global coverage

**Vector Data:**
- [Natural Earth](https://www.naturalearthdata.com/) - Free vector datasets
- [OpenStreetMap](https://www.openstreetmap.org/) - Worldwide features
- [GADM](https://gadm.org/) - Administrative boundaries

**ML Models:**
- [Hugging Face](https://huggingface.co/models) - Pre-trained geospatial models
- [ONNX Model Zoo](https://github.com/onnx/models) - Standard ML models
- [TorchGeo](https://github.com/microsoft/torchgeo) - Geospatial PyTorch models

## Common Patterns

### Error Handling

All examples use robust error handling:

```rust
async fn process() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(path).await?;

    // Process with proper error propagation
    let result = process_dataset(&dataset).await?;

    Ok(())
}
```

### Logging

Examples use `tracing` for structured logging:

```rust
use tracing::{info, warn, error};

info!("Processing started");
warn!("Cloud cover exceeds threshold");
error!("Failed to load dataset: {}", e);
```

Set log level:
```bash
RUST_LOG=debug cargo run --release --example satellite_processing
```

### Configuration

Examples use configuration structs for flexibility:

```rust
let config = ProcessingConfig {
    input_dir: Path::new("data/input"),
    output_dir: Path::new("output/results"),
    parallel_jobs: num_cpus::get(),
    // ... more settings
};
```

### Progress Reporting

Long-running operations report progress:

```rust
for (idx, tile) in tiles.enumerate() {
    if idx % 100 == 0 {
        info!("Processing tile {}/{}", idx + 1, total);
    }
    process_tile(tile)?;
}
```

## Integration Examples

### With QGIS

```python
# Load results in QGIS
from qgis.core import QgsRasterLayer

layer = QgsRasterLayer(
    'output/processed/ndvi.tif',
    'NDVI'
)
QgsProject.instance().addMapLayer(layer)
```

### With Python (Rasterio/GeoPandas)

```python
import rasterio
import geopandas as gpd

# Load raster
with rasterio.open('output/processed/ndvi.tif') as src:
    data = src.read(1)

# Load vector
gdf = gpd.read_file('output/vectors/results.geojson')
```

### With JavaScript (Leaflet/OpenLayers)

```javascript
// Load GeoJSON
fetch('output/vectors/results.geojson')
  .then(r => r.json())
  .then(data => {
    L.geoJSON(data).addTo(map);
  });
```

## Performance Tips

### For Raster Processing

1. **Use appropriate data types**: F32 for calculations, U8 for final output
2. **Process in tiles**: Avoid loading entire image into memory
3. **Parallel processing**: Use `rayon` for CPU-bound operations
4. **SIMD**: Enable target-cpu=native for automatic vectorization

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### For Vector Processing

1. **Spatial indexing**: Always create indexes on geometry columns
2. **Batch operations**: Process features in batches
3. **Use GeoParquet**: Much faster than GeoJSON for large datasets
4. **Connection pooling**: Reuse database connections

### For ML Inference

1. **GPU acceleration**: Use CUDA/TensorRT when available
2. **Batch processing**: Process multiple tiles simultaneously
3. **Model optimization**: Convert to TensorRT for production
4. **Tiled inference**: Balance tile size with memory

## Troubleshooting

### Memory Issues

**Symptoms:** OOM errors, system slowdown

**Solutions:**
- Reduce tile size
- Process in smaller batches
- Use streaming I/O
- Lower parallel job count

```rust
// Before
tile_size: 1024,
parallel_jobs: 16,

// After
tile_size: 512,
parallel_jobs: 4,
```

### Performance Issues

**Symptoms:** Slow processing

**Solutions:**
- Enable release mode: `cargo run --release`
- Profile bottlenecks: `cargo flamegraph`
- Use GPU acceleration
- Optimize data access patterns

### Database Connection Issues

**Symptoms:** Connection refused, timeout

**Solutions:**
- Verify PostgreSQL/PostGIS is running
- Check connection parameters
- Test with `psql` first
- Verify network connectivity

```bash
# Test connection
psql -h localhost -U postgres -d gis_analysis -c "SELECT PostGIS_Version();"
```

## Contributing

Found an issue or have an improvement? Contributions welcome!

1. File an issue describing the problem
2. Submit a pull request with fixes
3. Add tests for new functionality
4. Update documentation

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

All examples are licensed under Apache-2.0.

Copyright: COOLJAPAN OU (Team Kitasan)

## Additional Resources

### Documentation
- [OxiGDAL Core API](../crates/oxigdal-core/README.md)
- [Algorithm Reference](../crates/oxigdal-algorithms/README.md)
- [Tutorial Series](../docs/tutorials/)
- [Cookbook](../docs/cookbook/)

### External Resources
- [GDAL Documentation](https://gdal.org/)
- [PostGIS Manual](https://postgis.net/documentation/)
- [ONNX Runtime](https://onnxruntime.ai/)
- [Cloud Optimized GeoTIFF](https://www.cogeo.org/)

### Community
- GitHub Issues: https://github.com/cool-japan/oxigdal/issues
- Discussions: https://github.com/cool-japan/oxigdal/discussions

## Citation

If you use OxiGDAL in your research, please cite:

```bibtex
@software{oxigdal2026,
  title = {OxiGDAL: Pure Rust Geospatial Data Abstraction Library},
  author = {COOLJAPAN OU (Team Kitasan)},
  year = {2026},
  url = {https://github.com/cool-japan/oxigdal}
}
```

---

**Happy Coding!** For questions or issues, please open a GitHub issue or discussion.
