# OxiGDAL Advanced COG Viewer - Overview

**Phase 1 Browser Breakthrough Showcase Demo**

A production-ready, feature-rich web application demonstrating OxiGDAL's browser capabilities for viewing Cloud Optimized GeoTIFFs (COGs) using WebAssembly.

## Quick Facts

- **Technology**: Pure Rust (compiled to WebAssembly) + Vanilla JavaScript
- **Map Framework**: Leaflet 1.9.4
- **Lines of Code**: 3500+ (HTML, JavaScript, CSS, Documentation)
- **WASM Size**: ~103 KB (optimized)
- **Browser Support**: Chrome 90+, Firefox 88+, Safari 14+
- **Dependencies**: Zero runtime dependencies (except Leaflet CDN)

## Project Structure

```
demo/cog-viewer/
├── index.html           13 KB  - Main HTML structure with modern UI
├── main.js              17 KB  - Application logic and WASM integration
├── style.css            14 KB  - Professional CSS with custom properties
├── package.json          1.5 KB - NPM configuration
├── run.sh                1.6 KB - Quick start script (executable)
├── verify.sh             3.2 KB - Verification script (executable)
├── .gitignore            339 B  - Git ignore rules
├── README.md            16 KB  - Comprehensive documentation
├── QUICKSTART.md         3.2 KB - Fast path to getting started
├── DEPLOYMENT.md        13 KB  - Platform-specific deployment guides
├── TESTING.md           11 KB  - Comprehensive testing checklist
└── OVERVIEW.md          (this file)

Shared WASM package (in parent demo/pkg/):
├── oxigdal_wasm.js      24 KB  - JavaScript bindings
├── oxigdal_wasm_bg.wasm 103 KB - WebAssembly binary
├── oxigdal_wasm.d.ts    2.2 KB - TypeScript definitions
└── package.json         688 B  - WASM package metadata
```

## Key Features

### Core Capabilities

1. **Cloud Optimized GeoTIFF Support**
   - HTTP range requests for efficient partial data fetching
   - Tile-based rendering for large imagery
   - Support for grayscale, RGB, RGBA, and multispectral

2. **Interactive Map Interface**
   - Leaflet-powered mapping with smooth pan/zoom
   - Custom tile layer using OxiGDAL WASM
   - Fit to bounds, coordinate display
   - Zoom level tracking

3. **Advanced Visualization**
   - Band mode selection (RGB, Grayscale, NIR, Custom)
   - Custom band combination (select R/G/B bands)
   - Opacity control (0-100%)
   - Brightness adjustment (-50 to +50)
   - Contrast adjustment (0-200%)
   - Real-time visualization updates

4. **Metadata Display**
   - Image dimensions (width × height)
   - Tile configuration
   - Band count and overview levels
   - Coordinate Reference System (EPSG code)
   - Source URL

5. **Performance Monitoring**
   - Load time tracking
   - Tile loading statistics
   - Cache hit/miss tracking
   - Data transfer monitoring
   - Real-time performance metrics

6. **Error Handling**
   - Graceful CORS error handling
   - Network error recovery
   - Invalid COG detection
   - User-friendly error messages
   - Error overlay with dismiss option

### User Interface

- **Modern Design**: Clean, professional UI with CSS custom properties
- **Responsive Layout**: Adapts to different screen sizes
- **Three-Panel Layout**:
  - Left: Controls and examples
  - Center: Interactive map
  - Right: Metadata and performance
- **Accessible**: Keyboard navigation, ARIA labels, proper contrast
- **Loading States**: Spinner, progress bar, status indicators

## Technology Stack

### Frontend

- **HTML5**: Semantic markup, modern standards
- **CSS3**: Grid, Flexbox, Custom Properties, Animations
- **JavaScript ES6**: Modules, async/await, modern syntax
- **Leaflet 1.9.4**: Interactive mapping library

### Backend (Client-Side)

- **Rust**: Pure Rust implementation (no C/Fortran dependencies)
- **wasm-bindgen**: Rust-JavaScript bridge
- **WebAssembly**: Near-native performance in browser
- **OxiGDAL Core**: Geospatial data processing
- **OxiGDAL GeoTIFF**: COG reading and parsing

### Build Tools

- **wasm-pack**: WebAssembly package builder
- **Cargo**: Rust package manager
- **No bundler**: Direct ES6 modules for simplicity

## Architecture

### Data Flow

```
User Input → JavaScript Event Handler
    ↓
WASM Module (WasmCogViewer)
    ↓
FetchBackend (HTTP Range Requests)
    ↓
COG Server (S3, GCS, etc.)
    ↓
Tile Data (bytes)
    ↓
WASM Processing (decode, convert to RGBA)
    ↓
ImageData Object
    ↓
Canvas Rendering (with visualization filters)
    ↓
Leaflet Display (composite tiles to map)
```

### Component Interaction

```
┌─────────────────────────────────────────────┐
│           User Interface (HTML)             │
│  ┌────────┐  ┌────────┐  ┌──────────────┐  │
│  │ Controls│  │  Map   │  │   Metadata   │  │
│  └────────┘  └────────┘  └──────────────┘  │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│     Application Logic (main.js)             │
│  ┌───────────┐  ┌──────────┐  ┌─────────┐  │
│  │ State Mgmt│  │ Event    │  │ Rendering│  │
│  │           │  │ Handlers │  │ Pipeline │  │
│  └───────────┘  └──────────┘  └─────────┘  │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│      WASM Module (oxigdal_wasm)             │
│  ┌──────────────┐  ┌────────────────────┐  │
│  │ WasmCogViewer│  │  FetchBackend      │  │
│  │  - open()    │  │  - HTTP Ranges     │  │
│  │  - read_tile()│  │  - CORS Support   │  │
│  │  - metadata()│  │  - Async Fetch     │  │
│  └──────────────┘  └────────────────────┘  │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│          COG Server (Remote)                │
│  AWS S3 / GCS / Azure / HTTP Server         │
└─────────────────────────────────────────────┘
```

## API Reference

### WASM API (Exposed to JavaScript)

```javascript
// Initialize WASM module
await init();

// Create viewer instance
const viewer = new WasmCogViewer();

// Open COG from URL
await viewer.open(url);

// Get metadata
const width = viewer.width();
const height = viewer.height();
const tileWidth = viewer.tile_width();
const tileHeight = viewer.tile_height();
const bandCount = viewer.band_count();
const overviewCount = viewer.overview_count();
const epsgCode = viewer.epsg_code();
const metadataJson = viewer.metadata_json();

// Read tile as raw bytes
const tileBytes = await viewer.read_tile(level, tileX, tileY);

// Read tile as ImageData for canvas
const imageData = await viewer.read_tile_as_image_data(level, tileX, tileY);

// Get version
const ver = version();
```

### JavaScript Application API

```javascript
// Core functions
initializeApp()              // Initialize WASM and UI
loadCog(url, name)           // Load COG from URL
refreshVisualization()       // Reapply visualization settings
resetVisualization()         // Reset to defaults
fitToBounds()                // Fit map to COG extent

// Visualization
applyVisualization(ctx, w, h) // Apply filters to canvas
updateLayerOpacity()          // Update layer opacity

// UI Updates
updateStatus(status, text)    // Update status indicator
showLoading(message)          // Show loading overlay
hideLoading()                 // Hide loading overlay
showError(message)            // Show error overlay
displayMetadata()             // Update metadata panel
updateTileInfo()              // Update tile statistics
updatePerformanceDisplay()    // Update performance metrics
```

## Example Datasets

### Included Examples

1. **Sentinel-2 RGB** (Satellite Imagery)
   - URL: `https://sentinel-cogs.s3.us-west-2.amazonaws.com/...TCI.tif`
   - Type: True color RGB
   - Size: Large (cloud-optimized)
   - Bands: 3 (RGB)

2. **OpenAerialMap Haiti** (Aerial Photography)
   - URL: `https://oin-hotosm.s3.amazonaws.com/.../5afeda152b6a08001185f11b.tif`
   - Type: High-resolution aerial
   - Coverage: Haiti
   - Bands: 3 (RGB)

3. **Hurricane Harvey** (Disaster Response)
   - URL: `https://storage.googleapis.com/pdd-stac/.../20170831_172754_101c_3B_AnalyticMS.tif`
   - Type: Multispectral
   - Event: Hurricane Harvey 2017
   - Bands: Multiple (MS)

## Performance Characteristics

### Load Times (estimated, 100 Mbps connection)

- Application initialization: < 1s
- WASM module load: < 500ms
- COG metadata fetch: < 500ms
- First tile display: < 2s
- Full viewport render: < 5s

### Resource Usage

- **Memory**: 50-200 MB (depends on cache size)
- **WASM Binary**: 103 KB
- **JavaScript**: 24 KB
- **Total Transfer** (first load): ~150 KB + tiles

### Optimization Techniques

1. **Lazy Loading**: Only fetch visible tiles
2. **Caching**: In-memory tile cache
3. **Range Requests**: Download only needed chunks
4. **WASM**: Near-native processing speed
5. **Canvas Rendering**: Hardware-accelerated drawing

## Browser Compatibility Matrix

| Browser        | Version | Support | Notes                    |
|----------------|---------|---------|--------------------------|
| Chrome         | 90+     | ✅ Full  | Recommended, best perf   |
| Edge           | 90+     | ✅ Full  | Chromium-based, fast     |
| Firefox        | 88+     | ✅ Full  | Good performance         |
| Safari         | 14+     | ✅ Full  | May need CORS setup      |
| Chrome Mobile  | 90+     | ✅ Full  | Touch gestures work      |
| Safari iOS     | 14+     | ⚠️ Partial | Memory limitations    |
| Firefox Mobile | 88+     | ✅ Full  | Works well               |

**Requirements:**
- WebAssembly support
- ES6 modules
- HTML5 Canvas API
- Fetch API with Range headers

## Deployment Options

### Tested Platforms

- ✅ GitHub Pages (free, easy setup)
- ✅ Netlify (CDN, HTTPS, custom domain)
- ✅ Vercel (fast deployment, edge network)
- ✅ AWS S3 + CloudFront (enterprise-grade)
- ✅ Docker (nginx-alpine, portable)
- ✅ Self-hosted Nginx (full control)

### Deployment Time

- GitHub Pages: ~5 minutes
- Netlify: ~2 minutes
- Vercel: ~2 minutes
- Docker: ~5 minutes
- Self-hosted: ~15 minutes

## Development Workflow

### Quick Start Development

```bash
# 1. Clone repository
git clone https://github.com/cool-japan/oxigdal.git
cd oxigdal/demo/cog-viewer

# 2. Verify setup
./verify.sh

# 3. Start development server
./run.sh
```

### Build and Deploy

```bash
# 1. Build WASM (production)
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --release --out-dir ../../demo/pkg

# 2. Test locally
cd ../../demo/cog-viewer
./run.sh

# 3. Deploy (choose one)
netlify deploy --prod
# or
vercel --prod
# or
git push  # (if using GitHub Pages)
```

### Development Tips

1. **Hot Reload**: Use `live-server` or similar for auto-refresh
2. **WASM Rebuild**: Only rebuild when Rust code changes
3. **Browser DevTools**: Essential for debugging
4. **Network Tab**: Monitor tile requests and timing
5. **Performance Tab**: Profile rendering and memory

## Testing Strategy

### Manual Testing

- Use `TESTING.md` checklist
- Test on multiple browsers
- Try various COG types
- Verify error handling

### Automated Testing (Future)

- Unit tests for JavaScript functions
- E2E tests with Playwright/Cypress
- Visual regression tests
- Performance benchmarks

## Known Limitations

1. **Browser Memory**: Large COGs may exceed mobile browser limits
2. **CORS**: Requires server-side CORS configuration
3. **CRS Support**: Limited to common projections (future enhancement)
4. **Geotransform**: Simplified bounds calculation (future enhancement)
5. **Band Math**: No on-the-fly band calculations yet
6. **Export**: No download/export functionality yet

## Future Enhancements

### Phase 2 (Planned)

- [ ] Web Worker integration for parallel tile loading
- [ ] Advanced band math (NDVI, NDWI, etc.)
- [ ] Export to PNG/JPEG
- [ ] Measurement tools (distance, area)
- [ ] Time series visualization
- [ ] 3D terrain rendering
- [ ] Offline mode with IndexedDB caching
- [ ] Multi-file comparison view

### Phase 3 (Possible)

- [ ] Vector overlay support
- [ ] Analysis tools integration
- [ ] Real-time collaboration
- [ ] Cloud processing integration
- [ ] Mobile app (React Native + WASM)

## Documentation

### Available Guides

- **README.md**: Full documentation (16 KB, comprehensive)
- **QUICKSTART.md**: Fast path to running (3.2 KB)
- **DEPLOYMENT.md**: Platform-specific guides (13 KB)
- **TESTING.md**: Testing checklist (11 KB)
- **OVERVIEW.md**: This file

### External Resources

- [OxiGDAL Repository](https://github.com/cool-japan/oxigdal)
- [COG Specification](https://www.cogeo.org/)
- [Leaflet Documentation](https://leafletjs.com/reference.html)
- [WebAssembly Concepts](https://webassembly.org/docs/use-cases/)

## Credits

### Technology

- **Rust**: Programming language
- **wasm-bindgen**: Rust-WASM bridge
- **Leaflet**: Interactive maps
- **OxiGDAL**: Pure Rust GDAL implementation

### Team

- **Author**: COOLJAPAN OU (Team Kitasan)
- **License**: Apache-2.0
- **Repository**: https://github.com/cool-japan/oxigdal

## Support

### Getting Help

1. **Documentation**: Read README and guides first
2. **Issues**: Check existing GitHub issues
3. **New Issue**: Open detailed issue with:
   - Browser and version
   - COG URL (if public)
   - Console errors
   - Expected vs actual behavior

### Contributing

Contributions welcome! See main repository for guidelines.

## Summary

The OxiGDAL Advanced COG Viewer is a **production-ready**, **feature-rich** web application that demonstrates the power of WebAssembly for geospatial data processing in the browser. With **zero backend dependencies**, it provides a smooth, interactive experience for viewing Cloud Optimized GeoTIFFs from any publicly accessible source.

**Key Achievements:**
- ✅ Pure client-side processing
- ✅ Professional UI/UX
- ✅ Advanced visualization controls
- ✅ Comprehensive error handling
- ✅ Performance monitoring
- ✅ Multi-browser support
- ✅ Extensive documentation
- ✅ Multiple deployment options

**Phase 1 Browser Breakthrough**: Complete and ready to showcase.

---

**Built with OxiGDAL** | Part of the **COOLJAPAN Pure Rust Ecosystem**

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
