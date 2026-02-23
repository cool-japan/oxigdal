# Real-World Satellite Processing Example

This example demonstrates a complete, production-ready satellite data processing pipeline using OxiGDAL.

## Features

- **Multi-sensor support**: Landsat 8/9, Sentinel-2
- **Radiometric calibration**: DN to TOA reflectance conversion
- **Atmospheric correction**: DOS1 (Dark Object Subtraction) method
- **Spectral indices**: NDVI, NDWI, EVI, SAVI, and more
- **Cloud masking**: Automated cloud detection from QA bands
- **Pan-sharpening**: Enhance resolution using panchromatic band
- **Change detection**: Compare temporal imagery
- **COG export**: Cloud Optimized GeoTIFF output
- **Cloud storage**: Direct upload to AWS S3

## Requirements

- Rust 1.85+
- AWS credentials (optional, for S3 upload)

## Sample Data

### Option 1: Download Landsat 8/9 Data

1. Go to [USGS EarthExplorer](https://earthexplorer.usgs.gov/)
2. Search for a location and date range
3. Select Landsat 8 or 9 Level-1 data
4. Download the scene (typically ~1GB compressed)
5. Extract to `data/landsat8/` directory

Required bands for full functionality:
- `LC08_*_B2.TIF` - Blue
- `LC08_*_B3.TIF` - Green
- `LC08_*_B4.TIF` - Red
- `LC08_*_B5.TIF` - Near-Infrared (NIR)
- `LC08_*_B6.TIF` - SWIR 1 (optional)
- `LC08_*_B7.TIF` - SWIR 2 (optional)
- `LC08_*_B8.TIF` - Panchromatic (optional, for pan-sharpening)
- `LC08_*_QA_PIXEL.TIF` - Quality assessment (for cloud masking)
- `LC08_*_MTL.txt` - Metadata file

### Option 2: Download Sentinel-2 Data

1. Go to [Copernicus Open Access Hub](https://scihub.copernicus.eu/)
2. Search for a location and date
3. Download Sentinel-2 Level-1C or Level-2A product
4. Extract to `data/sentinel2/` directory

Required bands:
- `T*_B02.jp2` - Blue (10m)
- `T*_B03.jp2` - Green (10m)
- `T*_B04.jp2` - Red (10m)
- `T*_B08.jp2` - NIR (10m)
- `T*_B11.jp2` - SWIR 1 (20m, optional)
- `T*_B12.jp2` - SWIR 2 (20m, optional)
- `T*_QA60.jp2` - Cloud mask

### Option 3: Use Synthetic Test Data

For testing without downloading large datasets:

```bash
# Generate synthetic Landsat-like data
cargo run --example generate_test_satellite_data
```

This creates synthetic bands in `data/landsat8/` suitable for testing the pipeline.

## Usage

### Basic Processing

```bash
# Process with default settings
cargo run --release --example satellite_processing
```

### With Cloud Upload

1. Set AWS credentials:
```bash
export AWS_ACCESS_KEY_ID="your_access_key"
export AWS_SECRET_ACCESS_KEY="your_secret_key"
export AWS_REGION="us-west-2"
```

2. Run with cloud upload enabled:
```bash
cargo run --release --example satellite_processing -- --upload-s3
```

### Custom Configuration

```bash
# Specify input/output directories
cargo run --release --example satellite_processing -- \
    --input data/my_scene \
    --output output/my_results \
    --cloud-threshold 15.0 \
    --skip-atmos-correction
```

## Configuration Options

Modify the `ProcessingConfig` in `main.rs`:

```rust
let config = ProcessingConfig {
    input_dir: Path::new("data/landsat8"),
    output_dir: Path::new("output/processed"),
    sensor_type: SensorType::Landsat8,  // or SensorType::Sentinel2
    apply_atmospheric_correction: true,
    cloud_threshold: 20.0,  // Maximum acceptable cloud cover %
    calculate_indices: vec![
        SpectralIndex::Ndvi,
        SpectralIndex::Ndwi,
        SpectralIndex::Evi,
        SpectralIndex::Savi { soil_brightness: 0.5 },
    ],
    apply_pansharpening: true,
    export_cog: true,
    upload_to_cloud: false,
};
```

## Output

The pipeline generates:

1. **Spectral Index GeoTIFFs**: `{scene_id}_{index_name}.tif`
   - NDVI: Vegetation health (-1 to +1)
   - NDWI: Water content (-1 to +1)
   - EVI: Enhanced vegetation index
   - SAVI: Soil-adjusted vegetation index

2. **Cloud Mask**: `{scene_id}_cloudmask.tif`
   - Binary mask: 0 = clear, 1 = cloud

3. **Pan-sharpened Image**: `{scene_id}_pansharp.tif` (if panchromatic band available)
   - Enhanced spatial resolution

4. **Processing Report**: `{scene_id}_report.json`
   - Metadata and statistics

All outputs are Cloud Optimized GeoTIFFs (COGs) with:
- 512x512 tiling
- DEFLATE compression
- Overviews at 2x, 4x, 8x, 16x
- Suitable for cloud-native workflows

## Real-World Applications

### 1. Agricultural Monitoring
```rust
// Monitor crop health over growing season
let indices = vec![
    SpectralIndex::Ndvi,  // Overall vegetation
    SpectralIndex::Evi,   // Chlorophyll content
    SpectralIndex::Savi { soil_brightness: 0.5 },  // Early growth stage
];
```

### 2. Water Resource Management
```rust
// Track water bodies and moisture
let indices = vec![
    SpectralIndex::Ndwi,   // Water detection
    SpectralIndex::Mndwi,  // Modified NDWI for urban areas
    SpectralIndex::Awei,   // Automated water extraction
];
```

### 3. Forest Change Detection
```rust
// Detect deforestation or regrowth
let before = process_scene("2020-01-01")?;
let after = process_scene("2023-01-01")?;
let change = ChangeDetection::ndvi_difference(&before, &after)?;
```

### 4. Urban Heat Island Analysis
```rust
// Use thermal bands for temperature analysis
let indices = vec![
    SpectralIndex::Ndvi,   // Vegetation cooling effect
    SpectralIndex::Ndbi,   // Built-up area index
    SpectralIndex::Ui,     // Urban index
];
```

## Performance

Typical processing times (on M1 Mac / AMD Ryzen 9):

| Scene Size | Processing Time | Memory Usage |
|------------|----------------|--------------|
| Landsat 8 (Full scene ~7000x7000) | ~2-3 minutes | ~4 GB |
| Sentinel-2 (10m resolution) | ~5-8 minutes | ~8 GB |
| Small ROI (1000x1000) | ~10-20 seconds | ~500 MB |

Optimizations:
- Parallel band processing
- SIMD-accelerated calculations
- Streaming for large datasets
- Tile-based processing

## Troubleshooting

### Memory Issues

If processing large scenes causes OOM errors:

```rust
// Process in tiles instead of loading entire scene
let tile_size = 2048;
for tile in scene.tiles(tile_size) {
    process_tile(tile)?;
}
```

### Missing Bands

Not all spectral indices require all bands:

- **NDVI**: Requires NIR + Red only
- **NDWI**: Requires Green + NIR only
- **EVI**: Requires NIR + Red + Blue
- **SAVI**: Requires NIR + Red only

### Cloud Cover Too High

Adjust `cloud_threshold` or use cloud-free compositing:

```rust
// Create median composite from multiple scenes
let scenes = load_time_series(date_range)?;
let composite = CloudFreeComposite::median(&scenes)?;
```

## Advanced Usage

### Custom Spectral Index

Implement your own indices:

```rust
// Custom formula: (B5 - B4) / (B5 + B4 + B2)
let custom_index = calculator.apply(
    "(nir - red) / (nir + red + blue)",
    &[("nir", &nir_band), ("red", &red_band), ("blue", &blue_band)]
)?;
```

### Integration with ML Models

```rust
use oxigdal_ml::OnnxModel;

// Load pre-trained segmentation model
let model = OnnxModel::from_file("models/landcover_segmentation.onnx")?;

// Run inference on processed indices
let classification = model.predict(&[ndvi, ndwi, evi])?;
```

## References

- [Landsat 8-9 Data Users Handbook](https://www.usgs.gov/landsat-missions/landsat-8-data-users-handbook)
- [Sentinel-2 User Handbook](https://sentinel.esa.int/web/sentinel/user-guides/sentinel-2-msi)
- [Remote Sensing Index Database](https://www.indexdatabase.de/)
- [Cloud Optimized GeoTIFF Specification](https://www.cogeo.org/)

## License

Apache-2.0 (COOLJAPAN OU / Team Kitasan)
