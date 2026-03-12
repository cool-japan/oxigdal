# oxigdal-wasm

WebAssembly bindings for OxiGDAL - geospatial processing in the browser.

[![Documentation](https://docs.rs/oxigdal-wasm/badge.svg)](https://docs.rs/oxigdal-wasm)
[![License](https://img.shields.io/crates/l/oxigdal-wasm)](LICENSE)

## Overview

`oxigdal-wasm` enables client-side geospatial processing in the browser:

- **COG Viewer** - Display Cloud Optimized GeoTIFFs without a backend
- **Tile Caching** - LRU cache for optimal performance
- **Web Workers** - Parallel tile loading
- **Image Processing** - Contrast enhancement, color manipulation
- **Zero Server** - All processing happens client-side

## Features

- ✅ Browser-based COG viewing
- ✅ HTTP range request support
- ✅ Tile pyramid navigation
- ✅ Viewport management (pan/zoom)
- ✅ Image enhancement
- ✅ Statistics computation
- ✅ GeoJSON export

## Building

```bash
# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build
wasm-pack build --target web
```

## Installation (when published)

```bash
npm install @cooljapan/oxigdal
```

## Quick Start

### Basic COG Viewing

```html
<!DOCTYPE html>
<html>
<head>
    <title>COG Viewer</title>
</head>
<body>
    <canvas id="map" width="800" height="600"></canvas>

    <script type="module">
        import init, { WasmCogViewer } from './pkg/oxigdal_wasm.js';

        await init();

        const viewer = new WasmCogViewer();
        await viewer.open('https://example.com/satellite.tif');

        console.log(`Size: ${viewer.width()}x${viewer.height()}`);
        console.log(`Bands: ${viewer.band_count()}`);
        console.log(`EPSG: ${viewer.epsg_code()}`);

        // Read and display a tile
        const imageData = await viewer.read_tile_as_image_data(0, 0, 0);

        const canvas = document.getElementById('map');
        const ctx = canvas.getContext('2d');
        ctx.putImageData(imageData, 0, 0);
    </script>
</body>
</html>
```

### Advanced Viewer with Caching

```javascript
import { AdvancedCogViewer } from './pkg/oxigdal_wasm.js';

const viewer = new AdvancedCogViewer();

// Open with 50MB cache
await viewer.open(url, 50);

// Set viewport
viewer.setViewportSize(800, 600);
viewer.fitToImage();

// Pan and zoom
viewer.pan(100, 100);
viewer.zoomIn();

// Read with caching
const imageData = await viewer.readTileAsImageData(0, 0, 0);

// Check cache stats
const stats = JSON.parse(viewer.getCacheStats());
console.log('Cache hit rate:', stats.hitRate);
```

### Interactive Map

```javascript
class InteractiveMap {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        this.ctx = this.canvas.getContext('2d');
        this.viewer = new AdvancedCogViewer();
        this.setupInteraction();
    }

    async open(url) {
        await this.viewer.open(url, 100);
        this.viewer.setViewportSize(
            this.canvas.width,
            this.canvas.height
        );
        this.viewer.fitToImage();
        await this.render();
    }

    setupInteraction() {
        let dragging = false;
        let lastX, lastY;

        this.canvas.addEventListener('mousedown', (e) => {
            dragging = true;
            lastX = e.clientX;
            lastY = e.clientY;
        });

        this.canvas.addEventListener('mousemove', async (e) => {
            if (dragging) {
                const dx = e.clientX - lastX;
                const dy = e.clientY - lastY;
                this.viewer.pan(-dx, -dy);
                await this.render();
                lastX = e.clientX;
                lastY = e.clientY;
            }
        });

        this.canvas.addEventListener('mouseup', () => {
            dragging = false;
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
        // Render visible tiles
        const imageData = await this.viewer.readTileAsImageData(0, 0, 0);
        this.ctx.putImageData(imageData, 0, 0);
    }
}

// Usage
const map = new InteractiveMap('map');
await map.open('https://example.com/satellite.tif');
```

### Image Enhancement

```javascript
// Read tile with contrast enhancement
const imageData = await viewer.readTileWithContrast(
    0, 0, 0,
    'linear'  // 'linear', 'histogram', or 'adaptive'
);

// Compute statistics
const stats = JSON.parse(await viewer.computeStats(0, 0, 0));
console.log('Min:', stats.min, 'Max:', stats.max);

// Compute histogram
const hist = JSON.parse(await viewer.computeHistogram(0, 0, 0));
```

### Batch Tile Loading

```javascript
import { BatchTileLoader } from './pkg/oxigdal_wasm.js';

const loader = new BatchTileLoader(4); // 4 parallel requests
await loader.open(url, 50);

const coords = [
    0, 0,  // Tile (0, 0)
    1, 0,  // Tile (1, 0)
    0, 1,  // Tile (0, 1)
    1, 1,  // Tile (1, 1)
];

const results = await loader.loadTilesBatch(0, coords);
results.forEach((imageData, i) => {
    // Render each tile
});
```

### Web Workers

```javascript
import { WasmWorkerPool } from './pkg/oxigdal_wasm.js';

const pool = new WasmWorkerPool(4);
await pool.init();

await pool.loadCog(url);

const tasks = [
    { level: 0, x: 0, y: 0 },
    { level: 0, x: 1, y: 0 },
];

const results = await Promise.all(
    tasks.map(task => pool.processTile(task))
);

pool.terminate();
```

## API Reference

### WasmCogViewer

Basic COG viewer:

- `open(url)` - Open a COG file
- `width()` - Get image width
- `height()` - Get image height
- `tile_width()` - Get tile width
- `tile_height()` - Get tile height
- `band_count()` - Get number of bands
- `overview_count()` - Get number of overviews
- `epsg_code()` - Get EPSG code
- `read_tile(level, x, y)` - Read tile bytes
- `read_tile_as_image_data(level, x, y)` - Read as ImageData

### AdvancedCogViewer

Advanced viewer with caching and viewport:

- `open(url, cache_size_mb)` - Open with cache
- `setViewportSize(width, height)` - Set viewport
- `fitToImage()` - Fit viewport to image
- `pan(dx, dy)` - Pan viewport
- `zoomIn()` / `zoomOut()` - Zoom controls
- `setZoom(level)` - Set zoom level
- `readTileAsImageData(level, x, y)` - Read with cache
- `readTileWithContrast(level, x, y, method)` - Read with enhancement
- `computeStats(level, x, y)` - Compute statistics
- `getCacheStats()` - Get cache statistics
- `clearCache()` - Clear cache

## Demo Application

A complete COG viewer demo is available in `demo/cog-viewer/`:

```bash
cd demo/cog-viewer
npm install
npm run dev
```

## Browser Support

| Browser | Minimum Version |
|---------|----------------|
| Chrome | 87+ |
| Firefox | 89+ |
| Safari | 15+ |
| Edge | 87+ |

Requires WebAssembly, ES6 modules, and Fetch API.

## Performance Tips

1. **Enable caching** - Use AdvancedCogViewer with appropriate cache size
2. **Use overviews** - Read from appropriate zoom level
3. **Batch requests** - Use BatchTileLoader for parallel fetching
4. **Prefetch** - Set prefetch strategy for smoother navigation
5. **Web Workers** - Offload processing to background threads

## COOLJAPAN Policies

- ✅ Pure Rust (compiles to WASM)
- ✅ No unwrap() - Safe error handling
- ✅ Zero external dependencies
- ✅ Production ready

## License

Licensed under Apache-2.0.

Copyright © 2025 COOLJAPAN OU (Team Kitasan)

## See Also

- [WASM Guide](/tmp/oxigdal_wasm_guide.md)
- [API Documentation](https://docs.rs/oxigdal-wasm)
- [Demo Application](../../demo/cog-viewer/)
- [wasm-pack Documentation](https://rustwasm.github.io/wasm-pack/)
