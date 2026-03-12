# OxiGDAL WebAssembly Guide

This guide covers using OxiGDAL in the browser via WebAssembly (WASM).

## Table of Contents

1. [Introduction](#introduction)
2. [Setup and Installation](#setup-and-installation)
3. [COG Viewer](#cog-viewer)
4. [Advanced Features](#advanced-features)
5. [Performance Optimization](#performance-optimization)
6. [Examples](#examples)

---

## Introduction

OxiGDAL's WebAssembly bindings (`oxigdal-wasm`) enable geospatial processing directly in the browser:

- **No server required** - All processing happens client-side
- **Fast performance** - Near-native speed via WASM
- **Cloud data** - Read COGs directly from cloud storage via HTTP
- **Tile caching** - LRU cache for optimal performance
- **Web Workers** - Parallel tile loading
- **Image processing** - Contrast enhancement, color manipulation

### Use Cases

- **Web map viewers** - Display large COG files without backend
- **Interactive analysis** - Client-side raster/vector processing
- **Offline GIS** - Full GIS functionality in progressive web apps
- **Data preview** - Quick inspection of remote files
- **Mobile apps** - Use in React Native, Ionic, etc.

---

## Setup and Installation

### Building from Source

```bash
# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build the WASM package
cd oxigdal/crates/oxigdal-wasm
wasm-pack build --target web --out-dir ../../demo/cog-viewer/pkg
```

### Using in Your Project

**NPM/Yarn Installation** (when published):

```bash
npm install @cooljapan/oxigdal
# or
yarn add @cooljapan/oxigdal
```

**Manual Installation**:

Copy the generated `pkg/` directory to your project and import:

```javascript
import init, { WasmCogViewer } from './pkg/oxigdal_wasm.js';
```

### HTML Setup

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>OxiGDAL WASM Demo</title>
</head>
<body>
    <canvas id="map" width="800" height="600"></canvas>
    <script type="module">
        import init from './pkg/oxigdal_wasm.js';
        await init();
        // Your code here
    </script>
</body>
</html>
```

---

## COG Viewer

### Basic COG Viewing

```javascript
import init, { WasmCogViewer } from './pkg/oxigdal_wasm.js';

async function viewCog(url) {
    // Initialize WASM module
    await init();

    // Create viewer
    const viewer = new WasmCogViewer();
    await viewer.open(url);

    // Get metadata
    console.log(`Size: ${viewer.width()}x${viewer.height()}`);
    console.log(`Tiles: ${viewer.tile_width()}x${viewer.tile_height()}`);
    console.log(`Bands: ${viewer.band_count()}`);
    console.log(`Overviews: ${viewer.overview_count()}`);
    console.log(`EPSG: ${viewer.epsg_code()}`);
    console.log(`URL: ${viewer.url()}`);

    // Read and display a tile
    const level = 0;  // Full resolution
    const tileX = 0;  // First tile in X
    const tileY = 0;  // First tile in Y

    const imageData = await viewer.read_tile_as_image_data(level, tileX, tileY);

    // Draw on canvas
    const canvas = document.getElementById('map');
    const ctx = canvas.getContext('2d');
    ctx.putImageData(imageData, 0, 0);
}

// Example usage
viewCog('https://example.com/satellite.tif');
```

### Metadata Access

```javascript
// Get comprehensive metadata as JSON
const viewer = new WasmCogViewer();
await viewer.open(url);

const metadata = JSON.parse(viewer.metadata_json());
console.log('Metadata:', metadata);
/*
{
  "url": "https://...",
  "width": 8192,
  "height": 8192,
  "tileWidth": 256,
  "tileHeight": 256,
  "bandCount": 3,
  "overviewCount": 4,
  "epsgCode": 3857
}
*/
```

### Error Handling

```javascript
try {
    const viewer = new WasmCogViewer();
    await viewer.open(url);

    const imageData = await viewer.read_tile_as_image_data(0, 0, 0);
    renderTile(imageData);

} catch (error) {
    console.error('Failed to load COG:', error);
    displayError(error.message);
}
```

---

## Advanced Features

### Advanced Viewer with Caching

```javascript
import { AdvancedCogViewer } from './pkg/oxigdal_wasm.js';

async function advancedView(url) {
    const viewer = new AdvancedCogViewer();

    // Open with 50MB cache
    await viewer.open(url, 50);

    // Set viewport size
    viewer.setViewportSize(800, 600);

    // Fit to image
    viewer.fitToImage();

    // Pan and zoom
    viewer.pan(100, 100);  // Pan 100 pixels right and down
    viewer.zoomIn();       // Zoom in one level
    viewer.setZoom(2);     // Set specific zoom level

    // Get viewport info
    const viewport = JSON.parse(viewer.getViewport());
    console.log('Viewport:', viewport);

    // Read tile with caching
    const imageData = await viewer.readTileAsImageData(0, 0, 0);

    // Check cache stats
    const cacheStats = JSON.parse(viewer.getCacheStats());
    console.log('Cache hit rate:', cacheStats.hitRate);

    // Clear cache if needed
    viewer.clearCache();
}
```

### Viewport Management

```javascript
class MapViewer {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        this.ctx = this.canvas.getContext('2d');
        this.viewer = new AdvancedCogViewer();
    }

    async open(url) {
        await this.viewer.open(url, 100); // 100MB cache

        this.viewer.setViewportSize(
            this.canvas.width,
            this.canvas.height
        );

        this.viewer.fitToImage();
        await this.render();

        this.setupInteraction();
    }

    setupInteraction() {
        let isDragging = false;
        let lastX, lastY;

        this.canvas.addEventListener('mousedown', (e) => {
            isDragging = true;
            lastX = e.clientX;
            lastY = e.clientY;
        });

        this.canvas.addEventListener('mousemove', async (e) => {
            if (isDragging) {
                const dx = e.clientX - lastX;
                const dy = e.clientY - lastY;

                this.viewer.pan(-dx, -dy);
                await this.render();

                lastX = e.clientX;
                lastY = e.clientY;
            }
        });

        this.canvas.addEventListener('mouseup', () => {
            isDragging = false;
        });

        this.canvas.addEventListener('wheel', async (e) => {
            e.preventDefault();

            if (e.deltaY < 0) {
                this.viewer.zoomIn();
            } else {
                this.viewer.zoomOut();
            }

            await this.render();
        });
    }

    async render() {
        // Determine visible tiles and render them
        const viewport = JSON.parse(this.viewer.getViewport());
        // Calculate which tiles are visible
        // Render tiles...
    }
}

// Usage
const mapViewer = new MapViewer('map');
await mapViewer.open('https://example.com/large.tif');
```

### Image Processing

```javascript
// Read tile with contrast enhancement
const imageData = await viewer.readTileWithContrast(
    0, 0, 0,
    'linear'  // 'linear', 'histogram', or 'adaptive'
);

// Compute statistics
const stats = JSON.parse(
    await viewer.computeStats(0, 0, 0)
);
console.log('Min:', stats.min);
console.log('Max:', stats.max);
console.log('Mean:', stats.mean);

// Compute histogram
const histogram = JSON.parse(
    await viewer.computeHistogram(0, 0, 0)
);
```

### Batch Tile Loading

```javascript
import { BatchTileLoader } from './pkg/oxigdal_wasm.js';

async function batchLoad(url) {
    const loader = new BatchTileLoader(4); // 4 parallel requests
    await loader.open(url, 50);

    // Load multiple tiles in parallel
    const coords = [
        0, 0,  // Tile (0, 0)
        1, 0,  // Tile (1, 0)
        0, 1,  // Tile (0, 1)
        1, 1,  // Tile (1, 1)
    ];

    const results = await loader.loadTilesBatch(0, coords);

    results.forEach((imageData, i) => {
        const tx = coords[i * 2];
        const ty = coords[i * 2 + 1];
        renderTile(imageData, tx, ty);
    });
}
```

### Web Workers

```javascript
import { WasmWorkerPool } from './pkg/oxigdal_wasm.js';

async function useWorkers(url) {
    // Create worker pool
    const pool = new WasmWorkerPool(4); // 4 workers
    await pool.init();

    // Load COG in workers
    await pool.loadCog(url);

    // Process tiles in parallel
    const tasks = [
        { level: 0, x: 0, y: 0 },
        { level: 0, x: 1, y: 0 },
        { level: 0, x: 0, y: 1 },
        { level: 0, x: 1, y: 1 },
    ];

    const results = await Promise.all(
        tasks.map(task => pool.processTile(task))
    );

    // Get pool stats
    const stats = pool.getStats();
    console.log('Tasks completed:', stats.completedTasks);
    console.log('Active workers:', stats.activeWorkers);

    // Cleanup
    pool.terminate();
}
```

---

## Performance Optimization

### Tile Caching Strategy

```javascript
// Configure cache size based on zoom levels
function calculateCacheSize(width, height, tileSize, maxZoom) {
    const tilesPerLevel = Math.ceil(width / tileSize) *
                          Math.ceil(height / tileSize);
    const bytesPerTile = tileSize * tileSize * 4; // RGBA

    // Cache 2 zoom levels worth of tiles
    const cacheSizeBytes = tilesPerLevel * bytesPerTile * 2;
    const cacheSizeMB = Math.ceil(cacheSizeBytes / (1024 * 1024));

    return cacheSizeMB;
}

const viewer = new AdvancedCogViewer();
const cacheSize = calculateCacheSize(8192, 8192, 256, 18);
await viewer.open(url, cacheSize);
```

### Prefetch Strategies

```javascript
// Set prefetch strategy
viewer.setPrefetchStrategy('neighbors');  // Prefetch neighboring tiles
viewer.setPrefetchStrategy('pyramid');    // Prefetch pyramid levels
viewer.setPrefetchStrategy('none');       // No prefetching
```

### HTTP Range Request Optimization

```javascript
// COGs are optimized for HTTP range requests
// The viewer automatically uses byte-range requests
// to fetch only the needed tiles

// Monitor fetch statistics
import { FetchStats } from './pkg/oxigdal_wasm.js';

const stats = FetchStats.get();
console.log('Total requests:', stats.totalRequests);
console.log('Total bytes:', stats.totalBytes);
console.log('Cache hits:', stats.cacheHits);
```

### Retry Configuration

```javascript
import { FetchBackend, RetryConfig } from './pkg/oxigdal_wasm.js';

// Configure retry behavior
const config = RetryConfig.new()
    .maxRetries(3)
    .initialDelayMs(100)
    .maxDelayMs(5000)
    .backoffMultiplier(2.0);

const backend = await FetchBackend.newWithConfig(url, config);
```

---

## Examples

### Complete Interactive Viewer

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>OxiGDAL COG Viewer</title>
    <style>
        #container {
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        #map {
            border: 1px solid #ccc;
            cursor: move;
        }
        #controls {
            margin: 10px;
        }
        #info {
            font-family: monospace;
            white-space: pre;
        }
    </style>
</head>
<body>
    <div id="container">
        <div id="controls">
            <input type="text" id="url" placeholder="COG URL"
                   style="width: 400px;">
            <button id="load">Load</button>
            <button id="zoomIn">Zoom In</button>
            <button id="zoomOut">Zoom Out</button>
            <button id="fit">Fit</button>
        </div>
        <canvas id="map" width="800" height="600"></canvas>
        <div id="info"></div>
    </div>

    <script type="module">
        import init, { AdvancedCogViewer } from './pkg/oxigdal_wasm.js';

        await init();

        const canvas = document.getElementById('map');
        const ctx = canvas.getContext('2d');
        const info = document.getElementById('info');
        let viewer = null;

        document.getElementById('load').onclick = async () => {
            const url = document.getElementById('url').value;

            try {
                viewer = new AdvancedCogViewer();
                await viewer.open(url, 50);

                viewer.setViewportSize(canvas.width, canvas.height);
                viewer.fitToImage();

                updateInfo();
                await render();

            } catch (error) {
                alert('Failed to load COG: ' + error);
            }
        };

        document.getElementById('zoomIn').onclick = async () => {
            if (viewer) {
                viewer.zoomIn();
                await render();
                updateInfo();
            }
        };

        document.getElementById('zoomOut').onclick = async () => {
            if (viewer) {
                viewer.zoomOut();
                await render();
                updateInfo();
            }
        };

        document.getElementById('fit').onclick = async () => {
            if (viewer) {
                viewer.fitToImage();
                await render();
                updateInfo();
            }
        };

        async function render() {
            if (!viewer) return;

            // Clear canvas
            ctx.clearRect(0, 0, canvas.width, canvas.height);

            // Get visible tiles and render
            // (simplified - full implementation would calculate visible tiles)
            try {
                const imageData = await viewer.readTileAsImageData(0, 0, 0);
                ctx.putImageData(imageData, 0, 0);
            } catch (error) {
                console.error('Render error:', error);
            }
        }

        function updateInfo() {
            if (!viewer) return;

            const metadata = JSON.parse(viewer.getMetadata());
            const viewport = JSON.parse(viewer.getViewport());
            const cacheStats = JSON.parse(viewer.getCacheStats());

            info.textContent = `
Image: ${metadata.width}x${metadata.height}
Tiles: ${metadata.tileWidth}x${metadata.tileHeight}
Bands: ${metadata.bandCount}
Overviews: ${metadata.overviewCount}
EPSG: ${metadata.epsgCode || 'Unknown'}

Viewport Zoom: ${viewport.zoom}
Viewport Center: (${viewport.center_x.toFixed(1)}, ${viewport.center_y.toFixed(1)})

Cache Size: ${(cacheStats.sizeBytes / 1024 / 1024).toFixed(2)} MB
Cache Entries: ${cacheStats.entryCount}
Hit Rate: ${(cacheStats.hitRate * 100).toFixed(1)}%
            `.trim();
        }

        // Pan/zoom interaction
        let isDragging = false;
        let lastX, lastY;

        canvas.addEventListener('mousedown', (e) => {
            isDragging = true;
            lastX = e.clientX;
            lastY = e.clientY;
        });

        canvas.addEventListener('mousemove', async (e) => {
            if (isDragging && viewer) {
                const dx = e.clientX - lastX;
                const dy = e.clientY - lastY;

                viewer.pan(-dx, -dy);
                await render();
                updateInfo();

                lastX = e.clientX;
                lastY = e.clientY;
            }
        });

        canvas.addEventListener('mouseup', () => {
            isDragging = false;
        });

        canvas.addEventListener('wheel', async (e) => {
            e.preventDefault();

            if (viewer) {
                if (e.deltaY < 0) {
                    viewer.zoomIn();
                } else {
                    viewer.zoomOut();
                }

                await render();
                updateInfo();
            }
        });
    </script>
</body>
</html>
```

### GeoJSON Export

```javascript
import { GeoJsonExporter } from './pkg/oxigdal_wasm.js';

// Export image bounds as GeoJSON
const bounds = GeoJsonExporter.exportBounds(
    -122.5, 37.7,  // west, south
    -122.3, 37.8,  // east, north
    4326           // EPSG code
);

console.log('Bounds GeoJSON:', bounds);

// Export a point
const point = GeoJsonExporter.exportPoint(
    -122.4194, 37.7749,
    JSON.stringify({ name: "San Francisco" })
);

console.log('Point GeoJSON:', point);
```

---

## Browser Compatibility

OxiGDAL WASM works in all modern browsers:

| Browser | Minimum Version | Notes |
|---------|----------------|-------|
| Chrome | 87+ | Full support |
| Firefox | 89+ | Full support |
| Safari | 15+ | Full support |
| Edge | 87+ | Full support |
| Opera | 73+ | Full support |

**Requirements:**
- WebAssembly support
- ES6 modules
- Async/await
- Fetch API with range requests

---

## Debugging

### Enable Console Logging

```javascript
// WASM module automatically logs to console
// Check browser console for detailed messages

// Example output:
// "Opening COG: https://example.com/image.tif"
// "TIFF header: LittleEndian, BigTIFF: false"
// "Opened COG: 8192x8192, 3 bands, 4 overviews"
```

### Performance Profiling

```javascript
console.time('open');
await viewer.open(url, 50);
console.timeEnd('open');

console.time('tile');
const imageData = await viewer.readTileAsImageData(0, 0, 0);
console.timeEnd('tile');
```

### Memory Usage

```javascript
// Monitor memory usage
if (performance.memory) {
    console.log('Used JS heap:',
        (performance.memory.usedJSHeapSize / 1024 / 1024).toFixed(2), 'MB');
    console.log('Total JS heap:',
        (performance.memory.totalJSHeapSize / 1024 / 1024).toFixed(2), 'MB');
}
```

---

## See Also

- [Quickstart Guide](oxigdal_quickstart_guide.md)
- [Driver Guide](oxigdal_driver_guide.md)
- [Algorithm Guide](oxigdal_algorithm_guide.md)
- **Demo Application**: `oxigdal/demo/cog-viewer/`
- **API Documentation**: https://docs.rs/oxigdal-wasm
- **WASM Pack Documentation**: https://rustwasm.github.io/wasm-pack/
