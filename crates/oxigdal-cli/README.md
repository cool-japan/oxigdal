# OxiGDAL CLI

Command-line interface for OxiGDAL geospatial operations. A Pure Rust alternative to GDAL utilities.

## Installation

```bash
cargo install oxigdal-cli
```

Or build from source:

```bash
git clone https://github.com/cool-japan/oxigdal
cd oxigdal
cargo build --release -p oxigdal-cli
```

The binary will be available at `target/release/oxigdal`.

## Commands

### `oxigdal info` - Display File Information

Display metadata, geometry info, CRS, and statistics for raster and vector files.

```bash
# Basic info
oxigdal info input.tif

# With detailed statistics
oxigdal info input.tif --stats

# Show CRS details
oxigdal info input.tif --crs

# JSON output
oxigdal info input.tif --format json
```

**Options:**
- `--stats` - Show detailed statistics
- `--compute-minmax` - Compute min/max values
- `--metadata` - Show all metadata
- `--crs` - Show CRS details
- `--bands` - Show band/layer information

### `oxigdal convert` - Format Conversion

Convert between geospatial formats with compression and tiling options.

```bash
# Basic conversion
oxigdal convert input.tif output.tif

# Create Cloud-Optimized GeoTIFF
oxigdal convert input.tif output.cog --cog -t 256 -c lzw

# Convert with specific compression
oxigdal convert input.tif output.tif -c deflate

# Create COG with overviews
oxigdal convert input.tif output.cog --cog --overviews 4
```

**Options:**
- `-f, --format <FORMAT>` - Output format (auto-detected from extension)
- `-t, --tile-size <SIZE>` - Tile size for COG output (default: 512)
- `-c, --compression <METHOD>` - Compression (none, lzw, deflate, zstd, jpeg)
- `--compression-level <LEVEL>` - Compression level (1-9)
- `--cog` - Create Cloud-Optimized GeoTIFF
- `--overviews <NUM>` - Number of overview levels
- `--overwrite` - Overwrite existing output file
- `--progress` - Show progress bar (default: true)

### `oxigdal translate` - Subset and Resample

Subset rasters by extent or pixel coordinates and resample to different resolutions.

```bash
# Subset by bounding box
oxigdal translate input.tif output.tif --projwin -180 -90 180 90

# Subset by pixel coordinates
oxigdal translate input.tif output.tif --srcwin 0 0 1000 1000

# Resize to specific dimensions
oxigdal translate input.tif output.tif --outsize-x 500 --outsize-y 500

# Resample with bilinear interpolation
oxigdal translate input.tif output.tif -r bilinear --outsize-x 1000

# Select specific bands
oxigdal translate input.tif output.tif -b 1,2,3
```

**Options:**
- `--outsize-x <WIDTH>` - Output width in pixels
- `--outsize-y <HEIGHT>` - Output height in pixels
- `--projwin <MINX MINY MAXX MAXY>` - Subset by bounding box
- `--srcwin <XOFF YOFF XSIZE YSIZE>` - Subset by pixel coordinates
- `-b, --bands <BANDS>` - Select specific bands (comma-separated, 1-indexed)
- `-r, --resampling <METHOD>` - Resampling method (nearest, bilinear, bicubic, lanczos)
- `--overwrite` - Overwrite existing output file

### `oxigdal warp` - Reprojection

Reproject rasters to different coordinate reference systems.

```bash
# Reproject to Web Mercator
oxigdal warp input.tif output.tif -t EPSG:3857

# Reproject with specific source CRS
oxigdal warp input.tif output.tif -s EPSG:4326 -t EPSG:3857

# Reproject and resize
oxigdal warp input.tif output.tif -t EPSG:3857 --ts-x 1000 --ts-y 1000

# Set target resolution
oxigdal warp input.tif output.tif -t EPSG:3857 --tr 0.01

# Reproject with Lanczos resampling
oxigdal warp input.tif output.tif -t EPSG:3857 -r lanczos
```

**Options:**
- `-s, --s-srs <SRS>` - Source CRS (EPSG code or WKT)
- `-t, --t-srs <SRS>` - Target CRS (EPSG code or WKT) **[Required]**
- `--ts-x <WIDTH>` - Output width in pixels
- `--ts-y <HEIGHT>` - Output height in pixels
- `--tr <RESOLUTION>` - Output resolution in target units
- `-r, --resampling <METHOD>` - Resampling method (nearest, bilinear, bicubic, lanczos)
- `--te <MINX MINY MAXX MAXY>` - Output bounds in target SRS
- `--overwrite` - Overwrite existing output file

Full CRS reprojection is performed by `oxigdal-proj` (20+ projections, 1,000+ EPSG codes). Enable with the `proj` feature.

### `oxigdal calc` - Raster Calculator

Perform mathematical operations on rasters (map algebra).

```bash
# Calculate NDVI
oxigdal calc output.tif -A nir.tif -B red.tif --calc="(A-B)/(A+B)"

# Multi-band calculation
oxigdal calc output.tif -A band1.tif -B band2.tif -C band3.tif --calc="A+B+C"

# With custom output type
oxigdal calc output.tif -A input.tif -B mask.tif --calc="A*B" --output-type float32

# Set no-data value
oxigdal calc output.tif -A input.tif --calc="A*2" --no-data -9999
```

**Options:**
- `-A, --input-a <FILE>` - Input file A
- `-B, --input-b <FILE>` - Input file B
- `-C, --input-c <FILE>` - Input file C
- `-D, --input-d <FILE>` - Input file D
- `--calc <EXPR>` - Calculation expression **[Required]**
- `--no-data <VALUE>` - No data value for output
- `--output-type <TYPE>` - Output data type (uint8, uint16, uint32, int16, int32, float32, float64)
- `--overwrite` - Overwrite existing output file

**Supported Expressions:**
- NDVI: `(A-B)/(A+B)`
- Full raster algebra DSL via Pest grammar (arithmetic, logical, conditional, per-band ops)

### `oxigdal validate` - File Validation

Validate file format compliance and check for issues.

```bash
# Validate GeoTIFF
oxigdal validate input.tif

# Validate as COG
oxigdal validate input.tif --cog

# Validate GeoJSON
oxigdal validate input.geojson --geojson

# Strict validation
oxigdal validate input.tif --strict --verbose

# JSON output for CI/CD
oxigdal validate input.tif --cog --format json
```

**Options:**
- `--cog` - Validate as Cloud-Optimized GeoTIFF
- `--geojson` - Validate GeoJSON against specification
- `--strict` - Check for common issues and best practices
- `-v, --verbose` - Detailed validation report

### Shell Completions

Generate shell completions for bash, zsh, fish, PowerShell, or elvish.

```bash
# Bash
oxigdal completions bash > ~/.local/share/bash-completion/completions/oxigdal

# Zsh
oxigdal completions zsh > ~/.zfunc/_oxigdal

# Fish
oxigdal completions fish > ~/.config/fish/completions/oxigdal.fish

# PowerShell
oxigdal completions powershell > oxigdal.ps1
```

## Global Options

These options work with all commands:

- `-v, --verbose` - Enable verbose output
- `-q, --quiet` - Suppress all output except errors
- `--format <FORMAT>` - Output format (text, json)
- `-h, --help` - Print help information
- `-V, --version` - Print version information

## Examples

### Create a Cloud-Optimized GeoTIFF

```bash
oxigdal convert input.tif output.cog \
    --cog \
    --tile-size 256 \
    --compression lzw \
    --overviews 5
```

### Extract a subset and resize

```bash
oxigdal translate input.tif subset.tif \
    --projwin -122.5 37.5 -122.0 38.0 \
    --outsize-x 1000 \
    --outsize-y 1000 \
    --resampling bilinear
```

### Calculate NDVI from Landsat bands

```bash
oxigdal calc ndvi.tif \
    -A landsat_band5.tif \
    -B landsat_band4.tif \
    --calc="(A-B)/(A+B)" \
    --output-type float32 \
    --no-data -9999
```

### Validate COG compliance

```bash
oxigdal validate output.cog --cog --strict --verbose
```

## Supported Formats

### Raster Formats

- **GeoTIFF** (.tif, .tiff) - Including Cloud-Optimized GeoTIFF (COG)
- **Zarr** (.zarr) - Cloud-native chunked arrays v2/v3 (enable with `--features zarr`)

### Vector Formats

- **GeoJSON** (.json, .geojson)
- **Shapefile** (.shp)
- **FlatGeobuf** (.fgb)
- **GeoParquet** (.parquet, .geoparquet)

## Performance

OxiGDAL CLI is optimized for performance:

- Pure Rust implementation (no C/Fortran dependencies)
- SIMD vectorization for resampling operations
- Parallel processing where applicable
- Memory-efficient streaming for large files

## Cross-Platform Support

OxiGDAL CLI runs on:

- Linux (x86_64, aarch64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64)

## Contributing

Contributions are welcome! See the main OxiGDAL repository for guidelines.

## License

Licensed under Apache-2.0. See LICENSE for details.

## Authors

Copyright © COOLJAPAN OU (Team Kitasan)
