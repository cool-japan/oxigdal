# OxiGDAL Advanced COG Viewer

An interactive, feature-rich web application for viewing Cloud Optimized GeoTIFFs (COGs) in the browser using OxiGDAL's WebAssembly bindings and Leaflet.

## Overview

This demo showcases the capabilities of OxiGDAL in the browser environment, demonstrating:

- **Pure Client-Side Processing**: All geospatial processing happens in the browser via WebAssembly
- **Interactive Map Interface**: Leaflet-powered map with pan, zoom, and tile-based rendering
- **Advanced Visualization**: Band selection, brightness/contrast adjustments, and opacity control
- **Real-Time Performance Metrics**: Track loading times, tile counts, and data transfer
- **Professional UI**: Modern, responsive design with comprehensive controls
- **Zero Backend Requirements**: No server-side processing needed

## Features

### Core Capabilities

- **COG Loading**: Load Cloud Optimized GeoTIFFs from any publicly accessible URL
- **HTTP Range Requests**: Efficient partial data fetching for fast initial rendering
- **Tile-Based Rendering**: Lazy loading of only visible tiles for optimal performance
- **Tile Caching**: In-memory caching of loaded tiles to minimize redundant requests
- **Multiple Band Support**: Handle grayscale, RGB, and multispectral imagery

### Visualization Controls

- **Band Mode Selection**:
  - RGB (Bands 1-3)
  - Grayscale (Band 1)
  - NIR False Color
  - Custom band combination

- **Image Adjustments**:
  - Opacity control (0-100%)
  - Brightness adjustment (-50 to +50)
  - Contrast adjustment (0-200%)

### Map Interaction

- **Pan**: Click and drag to navigate
- **Zoom**: Mouse wheel or zoom controls
- **Fit to Bounds**: Automatically fit COG extent to view
- **Coordinate Display**: Real-time center coordinates and zoom level

### Metadata Display

- Image dimensions (width × height)
- Tile size configuration
- Band count
- Overview/pyramid levels
- Coordinate Reference System (EPSG code)
- Source URL

### Performance Monitoring

- **Load Time**: Track COG opening performance
- **Render Time**: Monitor tile rendering speed
- **Data Transfer**: Total bytes downloaded
- **Tile Statistics**: Count of loaded and cached tiles

## Prerequisites

Before running the demo, ensure you have:

1. **Rust** (latest stable version)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **wasm-pack** for building WebAssembly packages
   ```bash
   cargo install wasm-pack
   ```

3. **A local web server** (choose one):
   - Python 3: `python3 -m http.server`
   - Node.js http-server: `npm install -g http-server`
   - Any static file server with CORS support

## Building & Running

### Step 1: Build the WASM Package

From the project root:

```bash
# Navigate to oxigdal-wasm crate
cd crates/oxigdal-wasm

# Build for web target (development)
wasm-pack build --target web --out-dir ../../demo/pkg

# Or build optimized for production
wasm-pack build --target web --release --out-dir ../../demo/pkg
```

This will:
- Compile Rust code to WebAssembly
- Generate JavaScript bindings
- Output files to `demo/pkg/`

### Step 2: Start Local Server

From the project root:

```bash
cd demo/cog-viewer

# Option 1: Python
python3 -m http.server 8080

# Option 2: Node.js http-server
http-server -p 8080 -c-1

# Option 3: Simple HTTP server script (create run.sh)
#!/bin/bash
python3 -m http.server 8080
```

### Step 3: Open in Browser

Navigate to:
```
http://localhost:8080
```

## Usage Guide

### Loading a COG

1. **Using Example Datasets**:
   - Click one of the example dataset cards in the left sidebar
   - The COG will load automatically

2. **Using Custom URL**:
   - Enter a COG URL in the input field
   - Click "Load" button or press Enter
   - Ensure the URL supports CORS and HTTP range requests

### Navigating the Map

- **Pan**: Click and drag on the map
- **Zoom In**: Use "+" button, mouse wheel up, or pinch gesture
- **Zoom Out**: Use "-" button, mouse wheel down, or pinch gesture
- **Fit to Bounds**: Click "Fit to Bounds" button to center COG

### Adjusting Visualization

1. **Change Band Mode**:
   - Select from dropdown: RGB, Grayscale, NIR, or Custom
   - For Custom, enter band numbers in R/G/B fields

2. **Adjust Image Properties**:
   - Drag Opacity slider (0-100%)
   - Drag Brightness slider (-50 to +50)
   - Drag Contrast slider (0-200%)

3. **Reset to Defaults**:
   - Click "Reset Visualization" button

### Viewing Metadata

The right sidebar displays:
- Image dimensions and tile configuration
- Band count and overview levels
- Coordinate Reference System
- Performance metrics
- Tile loading statistics

## Example COG URLs

### Included Examples

1. **Sentinel-2 RGB**
   - URL: `https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/2020/S2A_36QWD_20200701_0_L2A/TCI.tif`
   - Description: True color satellite imagery from Sentinel-2
   - Bands: RGB (3 bands)

2. **OpenAerialMap Haiti**
   - URL: `https://oin-hotosm.s3.amazonaws.com/5afeda152b6a08001185f11a/0/5afeda152b6a08001185f11b.tif`
   - Description: High-resolution aerial imagery
   - Source: Humanitarian OpenStreetMap Team

3. **Hurricane Harvey**
   - URL: `https://storage.googleapis.com/pdd-stac/disasters/hurricane-harvey/0831/20170831_172754_101c_3B_AnalyticMS.tif`
   - Description: Disaster response imagery
   - Type: Multispectral

### Finding More COGs

Public COG repositories:
- **AWS Earth**: https://registry.opendata.aws/
- **Google Earth Engine**: https://earthengine.google.com/
- **Microsoft Planetary Computer**: https://planetarycomputer.microsoft.com/
- **OpenAerialMap**: https://openaerialmap.org/

## CORS Requirements

For the demo to work with remote COG files, the hosting server must:

1. **Support HTTP Range Requests**:
   ```
   Accept-Ranges: bytes
   ```

2. **Allow CORS from your domain**:
   ```
   Access-Control-Allow-Origin: *
   Access-Control-Allow-Methods: GET, HEAD
   Access-Control-Allow-Headers: Range
   Access-Control-Expose-Headers: Content-Length, Content-Range
   ```

### AWS S3 CORS Configuration

```xml
<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
    <CORSRule>
        <AllowedOrigin>*</AllowedOrigin>
        <AllowedMethod>GET</AllowedMethod>
        <AllowedMethod>HEAD</AllowedMethod>
        <AllowedHeader>*</AllowedHeader>
        <ExposeHeader>Content-Length</ExposeHeader>
        <ExposeHeader>Content-Range</ExposeHeader>
    </CORSRule>
</CORSConfiguration>
```

## Deployment

### GitHub Pages

1. Build the WASM package (see Step 1 above)

2. Push to repository:
   ```bash
   git add demo/
   git commit -m "Add COG viewer demo"
   git push
   ```

3. Enable GitHub Pages:
   - Go to repository Settings → Pages
   - Source: Deploy from branch
   - Branch: main, folder: `/demo/cog-viewer`
   - Save

4. Access at: `https://your-username.github.io/oxigdal/cog-viewer/`

### Netlify

1. Create `netlify.toml` in `demo/cog-viewer/`:
   ```toml
   [build]
     publish = "."
     command = "echo 'Static site - no build needed'"

   [[headers]]
     for = "/*"
     [headers.values]
       Access-Control-Allow-Origin = "*"
       Access-Control-Allow-Methods = "GET, HEAD"

   [[headers]]
     for = "/*.wasm"
     [headers.values]
       Content-Type = "application/wasm"
   ```

2. Deploy:
   ```bash
   cd demo/cog-viewer
   netlify deploy --prod
   ```

### Vercel

1. Create `vercel.json` in `demo/cog-viewer/`:
   ```json
   {
     "headers": [
       {
         "source": "/(.*)",
         "headers": [
           { "key": "Access-Control-Allow-Origin", "value": "*" },
           { "key": "Access-Control-Allow-Methods", "value": "GET, HEAD" }
         ]
       },
       {
         "source": "/*.wasm",
         "headers": [
           { "key": "Content-Type", "value": "application/wasm" }
         ]
       }
     ]
   }
   ```

2. Deploy:
   ```bash
   cd demo/cog-viewer
   vercel --prod
   ```

### Docker

Create `Dockerfile` in `demo/cog-viewer/`:

```dockerfile
FROM nginx:alpine

# Copy demo files
COPY . /usr/share/nginx/html/

# Copy nginx config for CORS and WASM MIME type
RUN echo 'server { \
    listen 80; \
    location / { \
        root /usr/share/nginx/html; \
        index index.html; \
        add_header Access-Control-Allow-Origin *; \
        add_header Access-Control-Allow-Methods "GET, HEAD"; \
        types { \
            application/wasm wasm; \
        } \
    } \
}' > /etc/nginx/conf.d/default.conf

EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
```

Build and run:
```bash
docker build -t oxigdal-cog-viewer .
docker run -p 8080:80 oxigdal-cog-viewer
```

## Architecture

### Technology Stack

- **Frontend Framework**: Vanilla JavaScript (ES6 modules)
- **Map Library**: Leaflet 1.9.4
- **WASM Runtime**: wasm-bindgen
- **Styling**: Modern CSS with CSS Custom Properties
- **No Build Tools**: Pure web standards for maximum compatibility

### Component Structure

```
demo/cog-viewer/
├── index.html          # Main HTML structure
├── main.js             # Application logic and WASM integration
├── style.css           # Comprehensive styling
├── package.json        # NPM metadata
└── README.md           # This file

demo/pkg/               # Generated WASM package
├── oxigdal_wasm.js     # JavaScript bindings
├── oxigdal_wasm_bg.wasm # WebAssembly binary
└── ...                 # TypeScript definitions, etc.
```

### Data Flow

1. **User Input**: User selects COG or enters URL
2. **WASM Initialization**: `WasmCogViewer` instance created
3. **COG Opening**: Fetch metadata via HTTP HEAD/range requests
4. **Metadata Display**: Extract and display image information
5. **Tile Loading**: Leaflet requests visible tiles
6. **Tile Rendering**: WASM reads and decodes tile data
7. **Visualization**: Apply band selection and image adjustments
8. **Canvas Rendering**: Draw processed tiles to canvas
9. **Caching**: Store tiles in memory for reuse

## Performance Optimization

### Implemented Optimizations

1. **Lazy Tile Loading**: Only fetch visible tiles
2. **In-Memory Caching**: Reuse previously loaded tiles
3. **Range Requests**: Download only needed data chunks
4. **Canvas Rendering**: Hardware-accelerated drawing
5. **WASM Processing**: Near-native performance for data processing

### Performance Tips

- **Use COGs**: Ensure GeoTIFFs are properly cloud-optimized
- **Enable Compression**: LZW or DEFLATE compression reduces transfer
- **Add Overviews**: Pyramids enable faster zoomed-out viewing
- **Host on CDN**: Use CloudFront, CloudFlare, etc. for faster delivery
- **Enable HTTP/2**: Multiplexing improves concurrent tile loading

## Browser Compatibility

### Supported Browsers

- Chrome/Edge 90+ (recommended)
- Firefox 88+
- Safari 14+
- Opera 76+

### Requirements

- WebAssembly support
- ES6 modules support
- HTML5 Canvas API
- Fetch API with Range header support
- CSS Grid and Flexbox

### Known Limitations

- Mobile browsers may have memory constraints with large COGs
- Safari has stricter CORS requirements
- Older browsers lack WebAssembly support

## Troubleshooting

### WASM Module Not Found

**Error**: `Failed to fetch WASM module`

**Solution**:
1. Ensure you built the WASM package:
   ```bash
   cd crates/oxigdal-wasm
   wasm-pack build --target web --out-dir ../../demo/pkg
   ```
2. Verify `demo/pkg/` directory exists with `.wasm` file
3. Check that web server serves `.wasm` files correctly

### CORS Errors

**Error**: `CORS policy: No 'Access-Control-Allow-Origin' header`

**Solutions**:
1. Use provided example datasets (they have CORS enabled)
2. Host your own COG with proper CORS headers
3. Use a CORS proxy (development only):
   ```
   https://cors-anywhere.herokuapp.com/YOUR_COG_URL
   ```

### Blank Map

**Issue**: Map displays but no COG imagery

**Solutions**:
1. Check browser console for errors
2. Verify COG URL is accessible in browser
3. Click "Fit to Bounds" to ensure correct viewport
4. Try resetting visualization settings
5. Check that COG has valid geographic extent

### Slow Loading

**Issue**: COG takes a long time to load

**Solutions**:
1. Verify COG is cloud-optimized (use `gdalinfo`)
2. Check network speed and COG file size
3. Try a COG with overviews/pyramids
4. Use a CDN-hosted COG for faster delivery
5. Consider using a lower-resolution overview

### Out of Memory

**Issue**: Browser crashes or freezes

**Solutions**:
1. Use smaller COGs or lower resolution
2. Clear tile cache by reloading page
3. Close other browser tabs
4. Try a different browser with more memory
5. Reduce concurrent tile loading

## Development

### Local Development Setup

1. Clone repository:
   ```bash
   git clone https://github.com/cool-japan/oxigdal.git
   cd oxigdal
   ```

2. Build WASM:
   ```bash
   cd crates/oxigdal-wasm
   wasm-pack build --target web --dev --out-dir ../../demo/pkg
   ```

3. Start server:
   ```bash
   cd ../../demo/cog-viewer
   python3 -m http.server 8080
   ```

4. Make changes to HTML/CSS/JS files

5. Rebuild WASM only if Rust code changes

6. Refresh browser to see changes

### Adding New Features

To add new visualization features:

1. **Update HTML**: Add UI controls in `index.html`
2. **Add Event Listeners**: Handle user input in `main.js`
3. **Modify Visualization Logic**: Update `applyVisualization()` function
4. **Update State**: Add new properties to `app.visualization` object
5. **Test**: Verify with various COGs and band counts

### Debugging

Enable verbose logging:
```javascript
// In main.js, add at top:
const DEBUG = true;

// Use throughout:
if (DEBUG) console.log('Debug info:', data);
```

Use browser DevTools:
1. **Console**: View logs and errors
2. **Network**: Monitor tile requests and timing
3. **Performance**: Profile rendering performance
4. **Memory**: Check for memory leaks

## Testing

### Manual Testing Checklist

- [ ] Application loads without errors
- [ ] Version badge displays correctly
- [ ] All example datasets load successfully
- [ ] Custom URL input works
- [ ] Map pan/zoom functions correctly
- [ ] Metadata displays accurate information
- [ ] Band mode selection changes visualization
- [ ] Brightness/contrast sliders work
- [ ] Opacity control functions
- [ ] Fit to bounds centers COG correctly
- [ ] Tile caching reduces redundant requests
- [ ] Performance metrics update
- [ ] Error overlay displays for invalid URLs
- [ ] Works in Chrome, Firefox, Safari

### Automated Testing

(Future enhancement - add unit tests for JavaScript functions)

## Contributing

Contributions are welcome! To contribute:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Test thoroughly across browsers
5. Commit: `git commit -m 'Add amazing feature'`
6. Push: `git push origin feature/amazing-feature`
7. Open a Pull Request

### Code Style

- Use 4-space indentation
- Follow existing naming conventions
- Add JSDoc comments for functions
- Keep functions focused and single-purpose
- Use meaningful variable names

## License

This demo is part of the OxiGDAL project and is licensed under the Apache-2.0 License.

See the main repository for full license text.

## Links & Resources

### OxiGDAL

- **Repository**: https://github.com/cool-japan/oxigdal
- **Documentation**: (Link to docs when available)
- **Issues**: https://github.com/cool-japan/oxigdal/issues

### COOLJAPAN Ecosystem

- **GitHub Organization**: https://github.com/cool-japan
- **Pure Rust Initiative**: All libraries implemented in 100% Rust

### Related Projects

- **GDAL**: https://gdal.org/
- **COG Specification**: https://www.cogeo.org/
- **Leaflet**: https://leafletjs.com/
- **wasm-bindgen**: https://rustwasm.github.io/wasm-bindgen/

### Learning Resources

- **WebAssembly**: https://webassembly.org/
- **COG Tutorial**: https://www.cogeo.org/developers-guide.html
- **GeoTIFF Spec**: https://www.awaresystems.be/imaging/tiff/specification/TIFF6.pdf
- **Rust WASM Book**: https://rustwasm.github.io/docs/book/

## Support

For questions, issues, or feature requests:

1. **Check Documentation**: Review this README and main project docs
2. **Search Issues**: Check if already reported
3. **Open Issue**: Create detailed issue on GitHub
4. **Discussions**: Use GitHub Discussions for questions

## Acknowledgments

- **GDAL Team**: For the original GDAL library inspiration
- **Rust Community**: For excellent WASM tooling
- **Leaflet Team**: For the mapping library
- **COOLJAPAN Team**: For the Pure Rust ecosystem vision

---

**Built with OxiGDAL** | Part of the **COOLJAPAN Pure Rust Ecosystem**

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
