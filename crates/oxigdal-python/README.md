# OxiGDAL Python Bindings

[![PyPI version](https://badge.fury.io/py/oxigdal.svg)](https://badge.fury.io/py/oxigdal)
[![Python versions](https://img.shields.io/pypi/pyversions/oxigdal.svg)](https://pypi.org/project/oxigdal/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Python bindings for [OxiGDAL](https://github.com/cool-japan/oxigdal), a pure Rust geospatial data abstraction library. OxiGDAL provides high-performance geospatial operations with seamless NumPy integration.

## Features

- **Pure Rust Performance**: Leverages Rust's speed and safety
- **NumPy Integration**: Zero-copy data transfer where possible
- **Raster Operations**: Read/write GeoTIFF, Zarr, NetCDF, and more
- **Vector Operations**: GeoJSON support with geometry operations
- **Raster Calculator**: Algebraic expressions on raster data
- **Coordinate Transformations**: Reprojection and warping
- **Type Hints**: Full type stub support for IDEs
- **No External Dependencies**: No GDAL installation required

## Installation

### From PyPI (recommended)

```bash
pip install oxigdal
```

### From Source

```bash
# Install maturin
pip install maturin

# Build and install
cd crates/oxigdal-python
maturin develop --release
```

## Quick Start

### Reading Rasters

```python
import oxigdal
import numpy as np

# Open a raster file
ds = oxigdal.open("input.tif")

# Get metadata
print(f"Size: {ds.width}x{ds.height}")
print(f"Bands: {ds.band_count}")

# Read a band as NumPy array
band1 = ds.read_band(1)
print(band1.shape, band1.dtype)

# Get full metadata
metadata = ds.get_metadata()
print(metadata)

# Close dataset
ds.close()
```

### Writing Rasters

```python
import oxigdal
import numpy as np

# Create test data
data = np.random.rand(512, 512).astype(np.float32)

# Create a new raster
ds = oxigdal.create_raster(
    "output.tif",
    width=512,
    height=512,
    bands=1,
    dtype="float32",
    crs="EPSG:4326",
    nodata=-9999.0
)

# Write data
ds.write_band(1, data)

# Set metadata
ds.set_metadata({
    "crs": "EPSG:4326",
    "nodata": -9999.0
})

# Close
ds.close()
```

### Using Context Manager

```python
import oxigdal
import numpy as np

# Automatically closes the dataset
with oxigdal.open("input.tif") as ds:
    data = ds.read_band(1)
    print(f"Mean: {data.mean():.2f}")
```

### Raster Calculator

```python
import oxigdal
import numpy as np

# Open multi-band image
ds = oxigdal.open("sentinel2.tif")
red = ds.read_band(3)   # Red band
nir = ds.read_band(4)   # NIR band

# Calculate NDVI using algebraic expression
ndvi = oxigdal.calc(
    "(NIR - RED) / (NIR + RED)",
    NIR=nir,
    RED=red
)

# More complex expressions
evi = oxigdal.calc(
    "2.5 * (NIR - RED) / (NIR + 6 * RED - 7.5 * BLUE + 1)",
    NIR=nir,
    RED=red,
    BLUE=ds.read_band(2)
)

# Simple arithmetic
scaled = oxigdal.calc("A * 0.0001 - 0.1", A=red)
```

### Reprojection

```python
import oxigdal

# Reproject to Web Mercator
oxigdal.warp(
    "input.tif",
    "output_3857.tif",
    dst_crs="EPSG:3857",
    resampling="bilinear"
)

# Resize raster
oxigdal.warp(
    "input.tif",
    "output_resized.tif",
    width=1024,
    height=1024,
    resampling="cubic"
)

# Reproject and resize
oxigdal.warp(
    "input.tif",
    "output.tif",
    dst_crs="EPSG:4326",
    width=2048,
    height=2048,
    resampling="lanczos"
)
```

### Vector Operations

```python
import oxigdal

# Read GeoJSON
features = oxigdal.read_geojson("input.geojson")
print(f"Features: {len(features['features'])}")

# Buffer a geometry
point = {
    "type": "Point",
    "coordinates": [0.0, 0.0]
}
buffered = oxigdal.buffer_geometry(point, distance=100.0)

# Write GeoJSON
output = {
    "type": "FeatureCollection",
    "features": [
        {
            "type": "Feature",
            "geometry": buffered,
            "properties": {"name": "Buffered Point"}
        }
    ]
}
oxigdal.write_geojson("output.geojson", output, pretty=True)
```

## API Reference

### Core Functions

#### `open(path: str, mode: str = "r") -> Dataset`

Opens a geospatial dataset.

**Parameters:**
- `path`: Path to file (local or remote URL)
- `mode`: Open mode - "r" for read (default), "w" for write

**Returns:** Opened `Dataset` object

**Raises:**
- `IOError`: If file cannot be opened
- `ValueError`: If format is not supported

#### `version() -> str`

Returns the OxiGDAL version string.

### Raster Functions

#### `create_raster(...) -> Dataset`

Creates a new raster file.

**Parameters:**
- `path`: Output file path
- `width`: Width in pixels
- `height`: Height in pixels
- `bands`: Number of bands (default: 1)
- `dtype`: Data type (default: "float32")
- `crs`: CRS as WKT or EPSG code (optional)
- `nodata`: NoData value (optional)

**Returns:** Created `Dataset` opened for writing

#### `calc(expression: str, **arrays) -> np.ndarray`

Raster calculator - evaluates expressions on raster data.

**Parameters:**
- `expression`: Mathematical expression (e.g., "(A - B) / (A + B)")
- `**arrays`: Named NumPy arrays (A=array1, B=array2, etc.)

**Returns:** Result array

**Supported Operators:**
- Arithmetic: `+`, `-`, `*`, `/`, `**` (power)
- Comparison: `<`, `>`, `<=`, `>=`, `==`, `!=`
- Functions: `sqrt`, `abs`, `log`, `exp`, `sin`, `cos`, `tan`

#### `warp(...) -> None`

Reprojects (warps) a raster to different CRS or resolution.

**Parameters:**
- `src_path`: Source raster path
- `dst_path`: Destination raster path
- `dst_crs`: Target CRS (EPSG code or WKT, optional)
- `width`: Target width in pixels (optional)
- `height`: Target height in pixels (optional)
- `resampling`: Resampling method (default: "bilinear")
  - Options: "nearest", "bilinear", "cubic", "lanczos"

### Vector Functions

#### `read_geojson(path: str) -> dict`

Reads a GeoJSON file.

**Parameters:**
- `path`: Path to GeoJSON file

**Returns:** Parsed GeoJSON as dictionary

#### `write_geojson(path: str, data: dict, pretty: bool = True) -> None`

Writes a GeoJSON file.

**Parameters:**
- `path`: Output path
- `data`: GeoJSON data as dictionary
- `pretty`: Pretty-print JSON (default: True)

#### `buffer_geometry(geometry: dict, distance: float, segments: int = 8) -> dict`

Buffers a geometry by specified distance.

**Parameters:**
- `geometry`: GeoJSON geometry
- `distance`: Buffer distance in geometry units
- `segments`: Number of segments per quadrant (default: 8)

**Returns:** Buffered geometry as GeoJSON

### Dataset Class

#### Properties

- `path`: Dataset file path (read-only)
- `width`: Width in pixels (read-only)
- `height`: Height in pixels (read-only)
- `band_count`: Number of bands (read-only)

#### Methods

##### `read_band(band: int) -> np.ndarray`

Reads a raster band as NumPy array.

**Parameters:**
- `band`: Band number (1-indexed)

**Returns:** 2D NumPy array with shape (height, width)

##### `write_band(band: int, array: np.ndarray) -> None`

Writes a NumPy array to a raster band.

**Parameters:**
- `band`: Band number (1-indexed)
- `array`: 2D NumPy array to write

##### `get_metadata() -> dict`

Returns dataset metadata as a dictionary.

##### `set_metadata(metadata: dict) -> None`

Sets dataset metadata.

**Parameters:**
- `metadata`: Metadata dictionary

##### `close() -> None`

Closes the dataset and flushes pending writes.

### RasterMetadata Class

Metadata container for raster datasets.

**Parameters:**
- `width`: Width in pixels
- `height`: Height in pixels
- `band_count`: Number of bands (default: 1)
- `data_type`: Data type as string (default: "float32")
- `crs`: CRS as WKT or EPSG code (optional)
- `nodata`: NoData value (optional)

**Methods:**
- `to_dict()`: Converts to dictionary

### Exceptions

#### `OxiGdalError`

Base exception for all OxiGDAL errors. Inherits from `Exception`.

Specific error types are mapped to appropriate Python exceptions:
- `IOError`: File I/O errors
- `ValueError`: Invalid parameters or data
- `NotImplementedError`: Unsupported operations
- `RuntimeError`: Internal errors

## Data Types

Supported raster data types:
- `uint8`, `int8`
- `uint16`, `int16`
- `uint32`, `int32`
- `uint64`, `int64`
- `float32`, `float64`
- `complex64`, `complex128`

## Supported Formats

### Raster Formats
- GeoTIFF (`.tif`, `.tiff`)
- Cloud Optimized GeoTIFF (COG)
- Zarr (`.zarr`)
- NetCDF (`.nc`)

### Vector Formats
- GeoJSON (`.geojson`, `.json`)
- FlatGeobuf (`.fgb`)
- Shapefile (`.shp`)
- GeoParquet (`.parquet`)

## Performance Tips

1. **Use context managers** for automatic resource cleanup
2. **Batch operations** when processing multiple files
3. **Use appropriate data types** - smaller types use less memory
4. **Leverage NumPy** for array operations
5. **Use resampling wisely** - "nearest" is fastest, "lanczos" highest quality

## Examples

See the `examples/` directory for complete examples:
- `raster_processing.py` - Raster I/O and processing
- `ndvi_calculation.py` - Vegetation index calculation
- `reprojection.py` - Coordinate transformation
- `vector_operations.py` - GeoJSON and geometry operations

## Development

### Requirements

- **Rust**: 1.70 or later
- **Python**: 3.9 or later
- **PyO3**: 0.24.x (automatically handled by Cargo)
- **Maturin**: For building Python wheels

### Building from Source

```bash
# Install development dependencies
pip install maturin pytest pytest-cov mypy ruff

# Build in debug mode
maturin develop

# Build in release mode
maturin develop --release

# Run tests
pytest tests/

# Type checking
mypy python/oxigdal/

# Linting
ruff check python/
```

### Build Configuration

The crate uses PyO3 with specific feature flags:

- **Default features** (`extension-module`): For building Python extensions (.so/.dylib files)
- **Without extension-module**: For Rust unit tests that need to initialize Python

The `extension-module` feature prevents linking to libpython (correct for Python extensions), but Rust unit tests need libpython to initialize the interpreter. See "Running Tests" section below for details.

### Running Tests

#### Python Tests (Recommended)

```bash
# Run all tests
pytest tests/ -v

# Run with coverage
pytest tests/ --cov=oxigdal --cov-report=html

# Run specific test file
pytest tests/test_raster.py -v
```

#### Rust Unit Tests

The library uses PyO3 with the `extension-module` feature for building Python extensions. To run Rust unit tests, you need to build without this feature:

```bash
# Compile library (default - with extension-module)
cargo check -p oxigdal-python --lib

# Compile and run Rust unit tests (without extension-module)
cargo test -p oxigdal-python --no-default-features --features geotiff,geojson,algorithms

# Or using maturin
maturin develop && pytest tests/
```

**Note:** The `extension-module` feature is required for building the Python extension but prevents Rust unit tests from linking to libpython. This is a PyO3 design choice - Python-facing tests should use pytest, while Rust unit tests validate internal logic.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE) for details.

## Citation

If you use OxiGDAL in your research, please cite:

```bibtex
@software{oxigdal2026,
  title = {OxiGDAL: Pure Rust Geospatial Data Abstraction Library},
  author = {{COOLJAPAN OU (Team Kitasan)}},
  year = {2026},
  url = {https://github.com/cool-japan/oxigdal}
}
```

## Acknowledgments

- Built with [PyO3](https://pyo3.rs/) for Rust-Python bindings
- Uses [maturin](https://www.maturin.rs/) for packaging
- Inspired by [GDAL](https://gdal.org/) but pure Rust

## Support

- Documentation: https://docs.rs/oxigdal
- Issue Tracker: https://github.com/cool-japan/oxigdal/issues
- Discussions: https://github.com/cool-japan/oxigdal/discussions

## Related Projects

- [OxiGDAL Core](../oxigdal-core) - Core Rust library
- [OxiGDAL WASM](../oxigdal-wasm) - WebAssembly bindings
- [OxiGDAL CLI](../oxigdal-cli) - Command-line interface
