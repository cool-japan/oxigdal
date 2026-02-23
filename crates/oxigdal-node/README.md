# OxiGDAL Node.js Bindings

**Production-ready Node.js bindings for OxiGDAL - Pure Rust geospatial data processing**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![npm version](https://img.shields.io/npm/v/@oxigdal/node.svg)](https://www.npmjs.com/package/@oxigdal/node)

## Features

- **Pure Rust Performance**: No C/C++ dependencies, full native performance
- **Zero-Copy Buffers**: Efficient data transfer between Node.js and Rust
- **Async/Await Support**: Promise-based async operations for I/O and processing
- **TypeScript Definitions**: Full TypeScript support with comprehensive type definitions
- **Comprehensive APIs**: Raster I/O, vector operations, terrain analysis, and more
- **Cloud-Native**: COG (Cloud Optimized GeoTIFF) support built-in
- **Cross-Platform**: Works on Linux, macOS, and Windows (x64, ARM64)

## Installation

```bash
npm install @oxigdal/node
```

Or with yarn:

```bash
yarn add @oxigdal/node
```

## Quick Start

### Raster Operations

```javascript
const oxigdal = require('@oxigdal/node');

// Open a raster file
const dataset = oxigdal.openRaster('input.tif');
console.log(`Size: ${dataset.width}x${dataset.height}`);
console.log(`Bands: ${dataset.bandCount}`);

// Read a band
const band = dataset.readBand(0);
const stats = band.statistics();
console.log(`Mean: ${stats.mean}, StdDev: ${stats.stddev}`);

// Create output
const output = oxigdal.createRaster(dataset.width, dataset.height, 1, 'float32');
output.writeBand(0, band);
output.save('output.tif');
```

### Terrain Analysis

```javascript
const oxigdal = require('@oxigdal/node');

async function analyzeTerrainAsync() {
  // Open DEM
  const dataset = await oxigdal.openRasterAsync('dem.tif');
  const dem = dataset.readBand(0);

  // Compute hillshade
  const hillshade = await oxigdal.hillshadeAsync(dem, 315, 45, 1.0);

  // Compute slope in degrees
  const slope = await oxigdal.slopeAsync(dem, 1.0, false);

  // Compute aspect
  const aspect = await oxigdal.aspectAsync(dem);

  // Save results
  const hsDataset = oxigdal.createRaster(dataset.width, dataset.height, 1, 'uint8');
  hsDataset.writeBand(0, hillshade);
  await oxigdal.saveRasterAsync(hsDataset, 'hillshade.tif');
}

analyzeTerrainAsync().catch(console.error);
```

### Vector Operations

```javascript
const oxigdal = require('@oxigdal/node');

// Read GeoJSON
const collection = oxigdal.readGeojson('features.geojson');
console.log(`Features: ${collection.count}`);

// Create new feature
const point = oxigdal.GeometryWrapper.point(-122.4, 37.8);
const feature = new oxigdal.Feature(point);
feature.setProperty('name', 'San Francisco');
collection.addFeature(feature);

// Buffer operation
const buffered = oxigdal.buffer(point, 0.1, 32);

// Area calculation
const polygon = oxigdal.GeometryWrapper.polygon([
  [
    [-122.5, 37.5],
    [-122.3, 37.5],
    [-122.3, 37.7],
    [-122.5, 37.7],
    [-122.5, 37.5]
  ]
]);
const area = oxigdal.area(polygon, 'geodetic');
console.log(`Area: ${area.toFixed(2)} m²`);

// Save
oxigdal.writeGeojson('output.geojson', collection);
```

## API Documentation

### Raster API

#### Dataset

```javascript
// Create or open
const dataset = oxigdal.createRaster(width, height, bandCount, dataType);
const dataset = oxigdal.openRaster('file.tif');

// Properties
dataset.width
dataset.height
dataset.bandCount
dataset.dataType
dataset.crs
dataset.nodata

// Geo transform
dataset.setGeoTransform([originX, pixelWidth, rotationX, originY, rotationY, pixelHeight]);
const gt = dataset.getGeoTransform();

// Coordinate conversion
const geo = dataset.pixelToGeo(x, y);
const pixel = dataset.geoToPixel(lon, lat);

// Band I/O
const band = dataset.readBand(bandIndex);
dataset.writeBand(bandIndex, buffer);
const window = dataset.readWindow(bandIndex, xOff, yOff, width, height);

// Save
dataset.save('output.tif');
```

#### BufferWrapper

```javascript
// Create
const buffer = new oxigdal.BufferWrapper(width, height, 'float32');

// Pixel access
buffer.setPixel(x, y, value);
const value = buffer.getPixel(x, y);

// Operations
buffer.fill(value);
const stats = buffer.statistics(); // { min, max, mean, stddev, count }
const cloned = buffer.clone();

// Node.js Buffer conversion
const nodeBuffer = buffer.toBuffer();
const buffer = oxigdal.BufferWrapper.fromBuffer(nodeBuffer, width, height, 'float32');
```

### Vector API

#### Geometry

```javascript
// Create geometries
const point = oxigdal.GeometryWrapper.point(x, y, z);
const linestring = oxigdal.GeometryWrapper.linestring([[x1, y1], [x2, y2], ...]);
const polygon = oxigdal.GeometryWrapper.polygon([exteriorRing, hole1, hole2, ...]);

// Properties
geometry.geometryType
geometry.bounds() // [minX, minY, maxX, maxY]

// GeoJSON
const geojson = geometry.toGeojson();
const geometry = oxigdal.GeometryWrapper.fromGeojson(geojson);
```

#### Feature & FeatureCollection

```javascript
// Features
const feature = new oxigdal.Feature(geometry, properties);
feature.setProperty('name', 'value');
const value = feature.getProperty('name');
const geojson = feature.toGeojson();

// Collections
const collection = new oxigdal.FeatureCollection();
collection.addFeature(feature);
const feature = collection.getFeature(index);
const count = collection.count;

// I/O
const collection = oxigdal.readGeojson('file.geojson');
oxigdal.writeGeojson('output.geojson', collection);
```

### Algorithm API

#### Resampling

```javascript
const resampled = oxigdal.resample(
  buffer,
  newWidth,
  newHeight,
  oxigdal.ResamplingMethod.Bilinear
);

// Methods: NearestNeighbor, Bilinear, Bicubic, Lanczos
```

#### Terrain Analysis

```javascript
// Hillshade
const hillshade = oxigdal.hillshade(dem, azimuth, altitude, zFactor);

// Slope (degrees or percent)
const slope = oxigdal.slope(dem, zFactor, asPercent);

// Aspect
const aspect = oxigdal.aspect(dem);

// Zonal statistics
const stats = oxigdal.zonalStats(raster, zones);
// Returns: [{ zoneId, count, min, max, mean, stddev, sum }, ...]
```

#### Vector Algorithms

```javascript
// Buffer
const buffered = oxigdal.buffer(geometry, distance, segments);

// Area
const area = oxigdal.area(polygon, 'planar' | 'geodetic');

// Simplify
const simplified = oxigdal.simplify(geometry, tolerance, 'douglas-peucker' | 'visvalingam-whyatt');
```

### Async API

All major operations have async variants:

```javascript
// Raster I/O
const dataset = await oxigdal.openRasterAsync(path);
await oxigdal.saveRasterAsync(dataset, path);

// Vector I/O
const collection = await oxigdal.readGeojsonAsync(path);
await oxigdal.writeGeojsonAsync(path, collection);

// Processing
const resampled = await oxigdal.resampleAsync(buffer, width, height, method);
const hillshade = await oxigdal.hillshadeAsync(dem, azimuth, altitude, zFactor);
const slope = await oxigdal.slopeAsync(dem, zFactor, asPercent);
const aspect = await oxigdal.aspectAsync(dem);
const stats = await oxigdal.zonalStatsAsync(raster, zones);

// Batch processing
const paths = await oxigdal.batchProcessRasters(inputPaths, outputDir, operation);
const result = await oxigdal.processRasterParallel(dataset, operation, config);
```

### Stream Processing

For large datasets:

```javascript
const stream = new oxigdal.RasterStream(dataset, chunkHeight);

let chunk;
while ((chunk = await stream.readNextChunk()) !== null) {
  console.log(`Progress: ${(stream.progress() * 100).toFixed(1)}%`);
  // Process chunk...
}
```

### Cancellation

```javascript
const token = new oxigdal.CancellationToken();

// Start operation
const promise = oxigdal.openRasterAsync(path);

// Cancel if needed
setTimeout(() => token.cancel(), 1000);

// Check status
if (token.isCancelled()) {
  console.log('Operation cancelled');
}
```

## Data Types

Supported raster data types:

- `'uint8'` - Unsigned 8-bit integer
- `'int16'` - Signed 16-bit integer
- `'uint16'` - Unsigned 16-bit integer
- `'int32'` - Signed 32-bit integer
- `'uint32'` - Unsigned 32-bit integer
- `'float32'` - 32-bit floating point
- `'float64'` - 64-bit floating point

## Supported Formats

### Raster
- **GeoTIFF** (.tif, .tiff) - Full support including COG
- Additional format bindings (GeoParquet, Zarr, FlatGeobuf) accessible via `oxigdal.open()` with appropriate features enabled

### Vector
- **GeoJSON** (.json, .geojson) - Full support

## Examples

See the `examples/` directory for complete examples:

- `01_basic_raster.js` - Basic raster I/O and operations
- `02_terrain_analysis.js` - DEM processing and terrain analysis
- `03_vector_operations.js` - Vector I/O and geometry operations
- `04_async_batch.js` - Async operations and batch processing

Run examples:

```bash
node examples/01_basic_raster.js
```

## Testing

```bash
npm test
```

Run with coverage:

```bash
npm test -- --coverage
```

## Performance

OxiGDAL Node.js bindings are designed for production use with:

- **Zero-copy data transfer** where possible
- **SIMD vectorization** (x86_64 AVX2, ARM NEON)
- **Multi-threaded operations** via Rust's async runtime
- **Optimized memory usage** with custom allocators

## TypeScript

Full TypeScript support is included:

```typescript
import * as oxigdal from '@oxigdal/node';

const dataset: oxigdal.Dataset = oxigdal.openRaster('input.tif');
const band: oxigdal.BufferWrapper = dataset.readBand(0);
const stats: oxigdal.Statistics = band.statistics();

async function process(): Promise<void> {
  const hillshade = await oxigdal.hillshadeAsync(band, 315, 45, 1.0);
  // ...
}
```

## Error Handling

All operations use standard JavaScript errors:

```javascript
try {
  const dataset = oxigdal.openRaster('nonexistent.tif');
} catch (error) {
  console.error(`Error: ${error.message}`);
  // Error codes available via oxigdal.getErrorCodes()
}
```

## Platform Support

- **Linux**: x86_64, aarch64 (glibc and musl)
- **macOS**: x86_64, Apple Silicon (M1/M2)
- **Windows**: x86_64, aarch64 (ARM64)

## Building from Source

Requirements:
- Rust 1.85+
- Node.js 16+

```bash
git clone https://github.com/cool-japan/oxigdal.git
cd oxigdal/crates/oxigdal-node
npm install
npm run build
```

## License

Apache-2.0

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## Links

- [GitHub Repository](https://github.com/cool-japan/oxigdal)
- [Documentation](https://docs.rs/oxigdal)
- [Issue Tracker](https://github.com/cool-japan/oxigdal/issues)
- [COOLJAPAN](https://github.com/cool-japan)

## Authors

COOLJAPAN OU (Team Kitasan)

---

**OxiGDAL** - Pure Rust geospatial processing for the modern age.
