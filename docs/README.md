# OxiGDAL Documentation

> 日本語版: [README.ja.md](README.ja.md)

## Getting Started

- [Quickstart](QUICKSTART.md) — Get up and running with OxiGDAL in 5 minutes
- [Getting Started](GETTING_STARTED.md) — Installation and basic usage

## Guides

- [Architecture](ARCHITECTURE.md) — Overall system architecture and crate layout
- [Drivers](DRIVERS.md) — Supported formats: GeoTIFF/COG, GeoJSON, GeoParquet, Zarr, FlatGeobuf, Shapefile, NetCDF, HDF5, GRIB, JPEG2000, VRT (11 formats total)
- [Algorithms](ALGORITHMS.md) — Resampling, raster/vector operations, terrain analysis
- [Performance Guide](PERFORMANCE_GUIDE.md) — Performance tuning and optimization
- [WASM Guide](WASM_GUIDE.md) — Building for WebAssembly and in-browser usage
- [Best Practices](BEST_PRACTICES.md) — Error handling, memory management, COOLJAPAN policies

## Migration

- [Migration from GDAL](MIGRATION_FROM_GDAL.md) — Migrating from GDAL/OGR (C++, Rasterio, GeoPandas)
- [API Comparison](API_COMPARISON.md) — GDAL C++/Python vs OxiGDAL API mapping
- [Python to Rust](PYTHON_TO_RUST.md) — Rust primer for Python geospatial developers

## Cookbook

- [Raster Recipes](cookbook/raster_recipes.md) — Common raster processing patterns
- [Vector Recipes](cookbook/vector_recipes.md) — Common vector processing patterns
- [Cloud Recipes](cookbook/cloud_recipes.md) — S3, GCS, Azure cloud workflows
- [Format Conversion](cookbook/format_conversion.md) — Converting between geospatial formats

## Tutorials

1. [Getting Started](tutorials/01_getting_started.md)
2. [Reading Rasters](tutorials/02_reading_rasters.md)
3. [Raster Operations](tutorials/03_raster_operations.md)
4. [Vector Data](tutorials/04_vector_data.md)
5. [Projections](tutorials/05_projections.md)
6. [Cloud Storage](tutorials/06_cloud_storage.md)

## Troubleshooting

- [Troubleshooting](TROUBLESHOOTING.md) — Common issues and solutions

## Developer Tooling

- [Fail Test Detection](FAIL_TEST_DETECTION.md) — Automated test failure detection and fixing system
- [Fail Test Quick Start](FAIL_TEST_DETECTION_QUICKSTART.md) — Set up in 5 minutes
