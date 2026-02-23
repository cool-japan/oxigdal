# OxiGDAL Cookbook Examples

Comprehensive, production-ready examples demonstrating real-world geospatial processing workflows with OxiGDAL. Each example is fully runnable and includes best practices, error handling, and performance optimization techniques.

## Overview

This cookbook contains 10 complete end-to-end examples covering the most common geospatial workflows:

| Example | Focus | Use Cases | LOC |
|---------|-------|-----------|-----|
| **satellite_processing.rs** | Landsat/Sentinel processing | Agricultural monitoring, vegetation indices | ~450 |
| **terrain_analysis.rs** | DEM analysis | Slope, aspect, hillshade, viewshed | ~350 |
| **change_detection.rs** | Multi-temporal analysis | Forest loss, urban growth, wetland changes | ~400 |
| **data_fusion.rs** | Multi-sensor integration | Pan-sharpening, data fusion | ~500 |
| **quality_assessment.rs** | QA/QC workflows | Data validation, quality control | ~600 |
| **batch_processing.rs** | High-volume processing | Processing 100+ files efficiently | ~450 |
| **custom_algorithms.rs** | Algorithm implementation | Filters, morphology, custom indices | ~550 |
| **ml_classification.rs** | Machine learning | Land cover classification with ONNX | ~650 |
| **cloud_etl_pipeline.rs** | Cloud workflows | S3 → Process → PostGIS pipeline | ~400 |
| **web_tile_server.rs** | Web mapping | Tile server setup and deployment | ~450 |

**Total: ~4,800 lines of practical, well-documented code**

## Getting Started

### Prerequisites

- Rust 1.85+
- OxiGDAL workspace compiled
- Optional: PostGIS, S3 bucket for cloud examples

### Running Examples

```bash
# Run any example
cargo run --example satellite_processing

# Run with release optimizations
cargo run --release --example batch_processing

# Build all examples
cargo build --examples
```

## Example Guides

### 1. Satellite Imagery Processing

**File:** `satellite_processing.rs`

Complete Landsat 8 / Sentinel-2 workflow including:
- Band loading and atmospheric correction
- Cloud masking
- Vegetation index calculation (NDVI, EVI, NDWI)
- Pan-sharpening
- Output visualization

**Key Concepts:**
- Radiometric normalization
- Index calculation
- Atmospheric effects handling
- STAC metadata

**Real-world application:** Agricultural monitoring, crop health assessment

```bash
cargo run --example satellite_processing
```

**Outputs:**
- `cloud_mask.tif` - Cloud contamination mask
- `ndvi.tif` - Normalized Difference Vegetation Index
- `evi.tif` - Enhanced Vegetation Index
- `ndwi.tif` - Normalized Difference Water Index

---

### 2. Terrain Analysis

**File:** `terrain_analysis.rs`

Complete DEM processing including:
- Slope calculation with classification
- Aspect determination
- Hillshade rendering (multiple illuminations)
- Viewshed analysis
- Contour generation
- Watershed delineation

**Key Concepts:**
- Derivative-based analysis
- Local spatial operations
- Line-of-sight computations
- Terrain classification

**Real-world application:** Land suitability analysis, infrastructure planning, hazard assessment

```bash
cargo run --example terrain_analysis
```

**Outputs:**
- `dem.tif` - Source elevation model
- `slope.tif` - Slope in degrees
- `aspect.tif` - Aspect (0-360°)
- `hillshade_*.tif` - Hillshade from multiple angles
- `viewshed.tif` - Visible areas from point
- `contours.shp` - Elevation contours

---

### 3. Change Detection

**File:** `change_detection.rs`

Multi-temporal change analysis:
- NDVI time series (2021-2023)
- Image differencing
- Annual change rates
- Trend detection with linear regression
- Change classification
- Statistical significance testing

**Key Concepts:**
- Time series analysis
- Statistical testing
- Change classification
- Acceleration detection
- Confidence intervals

**Real-world application:** Forest monitoring, urban expansion tracking, land cover changes

```bash
cargo run --example change_detection
```

**Outputs:**
- `change_2023_2021.tif` - Change map (2023 vs 2021)
- `change_class.tif` - Classified changes
- Trend statistics and significance tests

---

### 4. Multi-Sensor Data Fusion

**File:** `data_fusion.rs`

Pan-sharpening and data fusion:
- Multispectral-panchromatic fusion
- Brovey transform
- IHS (Intensity-Hue-Saturation) fusion
- Quality assessment (SAM, spatial correlation)
- Spectral preservation metrics

**Key Concepts:**
- Image fusion techniques
- Spatial/spectral tradeoffs
- Quality metrics
- RGB compositing
- Interpolation methods

**Real-world application:** High-resolution imagery production, multisource integration

```bash
cargo run --example data_fusion
```

**Outputs:**
- `brovey_*.tif` - Brovey pan-sharpened bands
- `ihs_*.tif` - IHS pan-sharpened bands
- Quality comparison metrics

---

### 5. Quality Assessment & Quality Control

**File:** `quality_assessment.rs`

Comprehensive QA/QC workflow:
- Completeness assessment
- Consistency validation
- Accuracy metrics (RMSE, MAE, bias)
- Spatial coherence analysis
- Data quality issues
- Automatic fixes
- Final acceptance checklist

**Key Concepts:**
- Data validation
- Quality metrics
- Error detection
- Interpolation
- Outlier removal
- Statistical significance

**Real-world application:** Dataset validation, vendor data acceptance, publication verification

```bash
cargo run --example quality_assessment
```

**Outputs:**
- `quality_report.txt` - Comprehensive quality report
- Fixed raster with improvements
- Before/after comparisons

---

### 6. Batch Processing

**File:** `batch_processing.rs`

Efficient processing of 100+ files:
- Sequential vs parallel processing comparison
- Rayon-based parallelization
- Performance benchmarking
- Progress tracking
- Error handling
- Memory efficiency
- Throughput analysis

**Key Concepts:**
- Parallel processing
- Performance metrics
- Batch orchestration
- Resource management
- Scalability

**Real-world application:** Landsat archive processing, operational pipelines, distributed workflows

```bash
cargo run --release --example batch_processing
```

**Outputs:**
- NDVI results for all 100 scenes
- `batch_report.txt` - Performance metrics
- Speedup measurements

**Performance Tips:**
- Use `--release` for optimization
- Adjust thread pool size via RAYON_NUM_THREADS
- Monitor memory usage on large batches

---

### 7. Custom Raster Algorithms

**File:** `custom_algorithms.rs`

Implementing custom image processing:
- Gaussian blur / convolution
- Sobel edge detection
- Custom vegetation indices (SAVI)
- Morphological operations (dilation, erosion)
- Directional derivatives
- Multi-band arithmetic
- Local statistics (variance)
- Complex pipelines

**Key Concepts:**
- Algorithm implementation
- Performance optimization
- Windowed processing
- Tiling strategies
- Numerical stability
- Edge effects handling

**Real-world application:** Domain-specific algorithms, custom indices, specialized filters

```bash
cargo run --example custom_algorithms
```

**Outputs:**
- Processed rasters for each algorithm
- Performance benchmarks
- Algorithm output analysis

---

### 8. Machine Learning Classification

**File:** `ml_classification.rs`

Land cover classification with ML:
- Sentinel-2 band loading
- Spectral index calculation
- ONNX model inference (simulated)
- Probability map generation
- Classification from probabilities
- Confidence filtering
- Accuracy assessment
- Confusion matrix
- Per-class metrics

**Key Concepts:**
- Feature engineering
- Model inference
- Post-processing
- Accuracy metrics
- Confusion matrices
- Kappa coefficient

**Real-world application:** Land cover mapping, crop classification, urban analysis

```bash
cargo run --example ml_classification
```

**Outputs:**
- `classification.tif` - Land cover map
- `confidence.tif` - Classification confidence
- `probability_*.tif` - Per-class probabilities
- `classification_report.txt` - Accuracy metrics

---

### 9. Cloud ETL Pipeline

**File:** `cloud_etl_pipeline.rs`

End-to-end cloud workflows:
- Extract from S3 / GCS / Azure
- Data validation
- Processing and transformation
- Load into PostGIS
- Spatial indexing
- Verification
- Performance reporting

**Key Concepts:**
- Cloud storage integration
- ETL orchestration
- Database operations
- Spatial indexing
- Error recovery
- Throughput analysis

**Real-world application:** Data warehouse ingestion, operational monitoring, distributed systems

```bash
cargo run --example cloud_etl_pipeline
```

**Outputs:**
- `etl_report.txt` - Pipeline execution metrics
- Database ingestion logs
- Performance statistics

---

### 10. Web Tile Server

**File:** `web_tile_server.rs`

Interactive mapping tile server:
- Server configuration
- Data source setup
- Tile pyramid definition
- Caching strategies
- Custom styling
- Performance optimization
- API endpoints
- Client integration

**Key Concepts:**
- Tile server architecture
- Caching strategies
- Web service APIs
- Performance optimization
- Deployment patterns

**Real-world application:** Web mapping, interactive dashboards, basemap infrastructure

```bash
cargo run --example web_tile_server
```

**Outputs:**
- `tile_server_config.toml` - Configuration file
- `client.html` - Example web client
- Server statistics

---

## Common Patterns and Best Practices

### 1. Error Handling

All examples use idiomatic Rust error handling with `Result<T, Box<dyn std::error::Error>>`:

```rust
fn process_data() -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = RasterBuffer::from_vec(values, width, height, RasterDataType::Float32)?;
    // ... processing
    Ok(data)
}
```

### 2. Temporary File Management

Examples use `std::env::temp_dir()` for safe temporary file handling:

```rust
let temp_dir = env::temp_dir();
let output_dir = temp_dir.join("output");
std::fs::create_dir_all(&output_dir)?;
```

### 3. Performance Optimization

- **Release builds:** Use `--release` for 10-100x speedup
- **Parallelization:** Rayon for multi-threaded processing
- **Caching:** Memory and disk caches for repeated operations
- **Streaming:** Process large files in chunks

### 4. Memory Efficiency

- Process large rasters in tiles
- Use appropriate data types (f32 vs f64)
- Avoid unnecessary copies
- Stream processing when possible

### 5. Geospatial Best Practices

- **Validation:** Check data bounds and metadata
- **Projection:** Use consistent coordinate reference systems
- **Metadata:** Preserve georeferencing and attributes
- **Quality:** Validate outputs against reference data

## Running Tests

```bash
# Test a specific example compiles
cargo build --example satellite_processing

# Run with verbose output
RUST_LOG=debug cargo run --example change_detection

# Benchmark batch processing
cargo run --release --example batch_processing
```

## Performance Characteristics

### Sequential vs Parallel Processing

From `batch_processing.rs` (100 scenes, 256x256 pixels):

| Processing Mode | Time | Throughput |
|-----------------|------|-----------|
| Sequential | ~15s | 6.7 scenes/sec |
| Parallel (4 cores) | ~4.5s | 22 scenes/sec |
| **Speedup** | **3.3x** | **3.3x** |

### Algorithm Performance

From `custom_algorithms.rs` (512x512 raster):

| Algorithm | Time | Speed |
|-----------|------|-------|
| Gaussian Blur | 8ms | 33M px/s |
| Sobel Detection | 12ms | 22M px/s |
| SAVI Calculation | 2ms | 131M px/s |
| Dilation | 45ms | 5.8M px/s |

## Integration with OxiGDAL Ecosystem

These examples demonstrate integration with:

- **oxigdal-core:** Buffer management, types, geotransforms
- **oxigdal-geotiff:** GeoTIFF I/O
- **oxigdal-analytics:** Time series, clustering, hotspot analysis
- **oxigdal-cloud:** S3, GCS, Azure integration
- **oxigdal-postgis:** Database operations
- **oxigdal-ml:** ONNX model inference
- **oxigdal-qc:** Quality checking
- **oxigdal-metadata:** Metadata management

## Extending These Examples

### Adding New Algorithms

1. Create a new example file in `cookbook/`
2. Import necessary OxiGDAL crates
3. Create synthetic data for testing
4. Implement algorithm with comments
5. Add performance benchmarks
6. Document use cases and parameters

### Adapting for Production

- Replace synthetic data with actual data sources
- Add proper error logging (tracing crate)
- Implement progress callbacks
- Add configuration file support
- Set up database connections
- Deploy with Docker/Kubernetes

## Resources

### Documentation
- [OxiGDAL Main Documentation](https://docs.rs/oxigdal)
- [Individual Crate Docs](../crates)
- [GDAL Documentation](https://gdal.org)

### Related Topics
- [Geospatial Processing](https://en.wikipedia.org/wiki/Geospatial_analysis)
- [Remote Sensing](https://en.wikipedia.org/wiki/Remote_sensing)
- [Web Mapping](https://wiki.osgeo.org/wiki/Category:Web_Mapping)

## Contributing

To contribute new examples:

1. Follow the existing structure and naming
2. Include comprehensive documentation
3. Add 300-500 lines of code with comments
4. Test with synthetic and real data
5. Document performance characteristics
6. Include error handling and validation

## License

All examples are provided under the Apache 2.0 license, consistent with OxiGDAL.

---

**Happy geospatial processing with OxiGDAL!**

For issues or questions, please refer to the main OxiGDAL repository.
