# OxiGeo Node.js Bindings

**Production-ready Node.js bindings for OxiGeo - Pure Rust geospatial data processing**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![npm version](https://img.shields.io/npm/v/@cooljapan/oxigeo-node.svg)](https://www.npmjs.com/package/@cooljapan/oxigeo-node)

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
npm install @cooljapan/oxigeo-node
```

Or with yarn:

```bash
yarn add @cooljapan/oxigeo-node
```

## Quick Start

### Raster Operations

```javascript
const oxigeo = require('@cooljapan/oxigeo-node');

// Open a raster file
const dataset = oxigeo.openRaster('input.tif');
console.log(`Size: ${dataset.width}x${dataset.height}`);
console.log(`Bands: ${dataset.bandCount}`);

// Read a band
const band = dataset.readBand(0);
const stats = band.statistics();
console.log(`Mean: ${stats.mean}, StdDev: ${stats.stddev}`);

// Create output
const output = oxigeo.createRaster(dataset.width, dataset.height, 1, 'float32');
output.writeBand(0, band);
output.save('output.tif');
```

### Terrain Analysis

```javascript
const oxigeo = require('@cooljapan/oxigeo-node');

async function analyzeTerrainAsync() {
  // Open DEM
  const dataset = await oxigeo.openRasterAsync('dem.tif');
  const dem = dataset.readBand(0);

  // pixelSize is the DEM's ground resolution (e.g. meters or degrees),
  // matching its coordinate reference system.
  const pixelSize = 30.0;

  // Compute hillshade
  const hillshade = await oxigeo.hillshadeAsync(dem, 315, 45, 1.0, pixelSize);

  // Compute slope in degrees
  const slope = await oxigeo.slopeAsync(dem, pixelSize, 1.0, false);

  // Compute aspect
  const aspect = await oxigeo.aspectAsync(dem, pixelSize);

  // Save results
  const hsDataset = oxigeo.createRaster(dataset.width, dataset.height, 1, 'uint8');
  hsDataset.writeBand(0, hillshade);
  await oxigeo.saveRasterAsync(hsDataset, 'hillshade.tif');
}

analyzeTerrainAsync().catch(console.error);
```

### Vector Operations

```javascript
const oxigeo = require('@cooljapan/oxigeo-node');

// Read GeoJSON
const collection = oxigeo.readGeojson('features.geojson');
console.log(`Features: ${collection.count}`);

// Create new feature
const point = oxigeo.GeometryWrapper.point(-122.4, 37.8);
const feature = new oxigeo.Feature(point);
feature.setProperty('name', 'San Francisco');
collection.addFeature(feature);

// Buffer operation
const buffered = oxigeo.buffer(point, 0.1, 32);

// Area calculation
const polygon = oxigeo.GeometryWrapper.polygon([
  [
    [-122.5, 37.5],
    [-122.3, 37.5],
    [-122.3, 37.7],
    [-122.5, 37.7],
    [-122.5, 37.5]
  ]
]);
const area = oxigeo.area(polygon, 'geodetic');
console.log(`Area: ${area.toFixed(2)} m²`);

// Save
oxigeo.writeGeojson('output.geojson', collection);
```

## API Documentation

### Raster API

#### Dataset

```javascript
// Create or open
const dataset = oxigeo.createRaster(width, height, bandCount, dataType);
const dataset = oxigeo.openRaster('file.tif');

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
const buffer = new oxigeo.BufferWrapper(width, height, 'float32');

// Pixel access
buffer.setPixel(x, y, value);
const value = buffer.getPixel(x, y);

// Operations
buffer.fill(value);
const stats = buffer.statistics(); // { min, max, mean, stddev, count }
const cloned = buffer.clone();

// Node.js Buffer conversion
const nodeBuffer = buffer.toBuffer();
const buffer = oxigeo.BufferWrapper.fromBuffer(nodeBuffer, width, height, 'float32');
```

### Vector API

#### Geometry

```javascript
// Create geometries
const point = oxigeo.GeometryWrapper.point(x, y, z);
const linestring = oxigeo.GeometryWrapper.linestring([[x1, y1], [x2, y2], ...]);
const polygon = oxigeo.GeometryWrapper.polygon([exteriorRing, hole1, hole2, ...]);

// Properties
geometry.geometryType
geometry.bounds() // [minX, minY, maxX, maxY]

// GeoJSON
const geojson = geometry.toGeojson();
const geometry = oxigeo.GeometryWrapper.fromGeojson(geojson);
```

#### Feature & FeatureCollection

```javascript
// Features
const feature = new oxigeo.Feature(geometry, properties);
feature.setProperty('name', 'value');
const value = feature.getProperty('name');
const geojson = feature.toGeojson();

// Collections
const collection = new oxigeo.FeatureCollection();
collection.addFeature(feature);
const feature = collection.getFeature(index);
const count = collection.count;

// I/O
const collection = oxigeo.readGeojson('file.geojson');
oxigeo.writeGeojson('output.geojson', collection);
```

### Algorithm API

#### Resampling

```javascript
const resampled = oxigeo.resample(
  buffer,
  newWidth,
  newHeight,
  oxigeo.ResamplingMethod.Bilinear
);

// Methods: NearestNeighbor, Bilinear, Bicubic, Lanczos
```

#### Terrain Analysis

```javascript
// pixelSize is the DEM's ground resolution (e.g. meters or degrees),
// matching its coordinate reference system.

// Hillshade
const hillshade = oxigeo.hillshade(dem, azimuth, altitude, zFactor, pixelSize);

// Slope (degrees or percent, depending on asPercent)
const slope = oxigeo.slope(dem, pixelSize, zFactor, asPercent);

// Aspect
const aspect = oxigeo.aspect(dem, pixelSize);

// Zonal statistics
const stats = oxigeo.zonalStats(raster, zones);
// Returns: [{ zoneId, count, min, max, mean, stddev, sum }, ...]
```

#### Raster Calculator

Evaluates a map-algebra expression across one or more input bands. Bands are
referenced positionally as `B1`, `B2`, ... (1-indexed). The language supports
arithmetic (`+ - * / ^`), math functions (`sqrt`, `log`, `log10`, `exp`,
`abs`, `floor`, `ceil`, `round`, `sin`, `cos`, `tan`, `min`, `max`),
comparisons (`> < >= <= == !=`), logical `and`/`or`, and `if/then/else`.

```javascript
// NDVI = (NIR - RED) / (NIR + RED)
const ndvi = oxigeo.calculate('(B1 - B2) / (B1 + B2)', [nirBand, redBand]);

// Math functions and conditionals
const magnitude = oxigeo.calculate('sqrt(B1 ^ 2 + B2 ^ 2)', [dx, dy]);
const mask = oxigeo.calculate('if B1 > 100 then 1 else 0', [band]);
```

#### Vector Algorithms

```javascript
// Buffer
const buffered = oxigeo.buffer(geometry, distance, segments);

// Area
const area = oxigeo.area(polygon, 'planar' | 'geodetic');

// Simplify
const simplified = oxigeo.simplify(geometry, tolerance, 'douglas-peucker' | 'visvalingam-whyatt');
```

### Async API

All major operations have async variants:

```javascript
// Raster I/O
const dataset = await oxigeo.openRasterAsync(path);
await oxigeo.saveRasterAsync(dataset, path);

// Vector I/O
const collection = await oxigeo.readGeojsonAsync(path);
await oxigeo.writeGeojsonAsync(path, collection);

// Processing
const resampled = await oxigeo.resampleAsync(buffer, width, height, method);
const hillshade = await oxigeo.hillshadeAsync(dem, azimuth, altitude, zFactor, pixelSize);
const slope = await oxigeo.slopeAsync(dem, pixelSize, zFactor, asPercent);
const aspect = await oxigeo.aspectAsync(dem, pixelSize);
const stats = await oxigeo.zonalStatsAsync(raster, zones);

// Batch processing.
// `operation` is a per-pixel transform applied to every band:
//   'identity' | 'abs' | 'negate' | 'square' | 'sqrt'
const paths = await oxigeo.batchProcessRasters(inputPaths, outputDir, 'sqrt');

// `processRasterParallel` splits each band into `chunkSize`-row chunks and
// applies the operation across `numThreads` worker threads.
const config = { numThreads: 0, chunkSize: 512, reportProgress: true };
const result = await oxigeo.processRasterParallel(dataset, 'square', config);

// Progress reporting: register a callback before starting a long-running
// operation to receive periodic progress fractions in [0.0, 1.0].
oxigeo.setProgressCallback((progress) => {
  console.log(`Progress: ${(progress * 100).toFixed(1)}%`);
});
await oxigeo.batchProcessRasters(inputPaths, outputDir, 'identity');
oxigeo.clearProgressCallback();
```

### Stream Processing

For large datasets:

```javascript
const stream = new oxigeo.RasterStream(dataset, chunkHeight);

let chunk;
while ((chunk = await stream.readNextChunk()) !== null) {
  console.log(`Progress: ${(stream.progress() * 100).toFixed(1)}%`);
  // Process chunk...
}
```

### Cancellation

A `CancellationToken` can be passed to the long-running batch/parallel
processors. When it is cancelled while work is in flight, the operation aborts
with a `CANCELLED` error instead of returning partially-processed data (chunks
not yet started are skipped; already-written batch outputs are left in place).

```javascript
const token = new oxigeo.CancellationToken();

// Pass the token into a long-running operation.
const promise = oxigeo.processRasterParallel(dataset, 'sqrt', config, token);

// Cancel it from elsewhere (e.g. a timeout or a user action).
setTimeout(() => token.cancel(), 1000);

try {
  const result = await promise;
} catch (err) {
  // err.message starts with "CANCELLED:" if the token was cancelled.
  console.log('Operation cancelled or failed:', err.message);
}

// The token can be reused after resetting it.
token.reset();
```

The token is likewise accepted as the trailing argument of
`batchProcessRasters(paths, outputDir, operation, token)`.

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

OxiGeo Node.js bindings are designed for production use with:

- **Zero-copy data transfer** where possible
- **SIMD vectorization** (x86_64 AVX2, ARM NEON)
- **Multi-threaded operations** via Rust's async runtime
- **Optimized memory usage** with custom allocators

## TypeScript

Full TypeScript support is included:

```typescript
import * as oxigeo from '@cooljapan/oxigeo-node';

const dataset: oxigeo.Dataset = oxigeo.openRaster('input.tif');
const band: oxigeo.BufferWrapper = dataset.readBand(0);
const stats: oxigeo.Statistics = band.statistics();

async function process(): Promise<void> {
  const hillshade = await oxigeo.hillshadeAsync(band, 315, 45, 1.0, 30.0);
  // ...
}
```

## Error Handling

All operations use standard JavaScript errors:

```javascript
try {
  const dataset = oxigeo.openRaster('nonexistent.tif');
} catch (error) {
  console.error(`Error: ${error.message}`);
  // Error codes available via oxigeo.getErrorCodes()
}
```

## Platform Support

- **Linux**: x86_64, aarch64 (glibc and musl)
- **macOS**: x86_64, Apple Silicon (M1/M2)
- **Windows**: x86_64, aarch64 (ARM64)

## Building from Source

Requirements:
- Rust 1.89+
- Node.js 16+

```bash
git clone https://github.com/cool-japan/oxigeo.git
cd oxigeo/crates/oxigeo-node
npm install
npm run build
```

## License

Apache-2.0

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

## Links

- [GitHub Repository](https://github.com/cool-japan/oxigeo)
- [Documentation](https://docs.rs/oxigeo)
- [Issue Tracker](https://github.com/cool-japan/oxigeo/issues)
- [COOLJAPAN](https://github.com/cool-japan)

## Authors

COOLJAPAN OU (Team Kitasan)

---

**OxiGeo** - Pure Rust geospatial processing for the modern age.
