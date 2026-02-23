# Quick Start Guide - OxiGDAL COG Viewer

Get the COG viewer running in 3 simple steps.

## Prerequisites

- Rust (latest stable)
- wasm-pack
- Python 3 or Node.js (for local server)

## Quick Start

### 1. Build WASM (if not already built)

```bash
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --out-dir ../../demo/pkg
cd ../../demo/cog-viewer
```

### 2. Start Server

```bash
./run.sh
```

Or manually:
```bash
python3 -m http.server 8080
```

### 3. Open Browser

Navigate to: **http://localhost:8080**

## Verify Installation

Run the verification script:
```bash
./verify.sh
```

This checks that all required files are in place.

## First Steps

1. Click one of the **Example Datasets** in the left sidebar
2. Wait for the COG to load (shows loading spinner)
3. **Pan** by clicking and dragging the map
4. **Zoom** using the +/- buttons or mouse wheel
5. View **metadata** in the right sidebar

## Example Datasets

The demo includes 3 pre-configured datasets:

1. **Sentinel-2 RGB**: Satellite imagery from Sentinel-2
2. **Aerial Imagery**: High-resolution imagery from OpenAerialMap
3. **Hurricane Harvey**: Disaster response imagery

All examples are hosted on S3 with CORS enabled.

## Loading Custom COGs

1. Enter a COG URL in the input field
2. Click **Load** or press Enter
3. Ensure the COG server supports:
   - HTTP range requests
   - CORS headers

### Example URLs

```
https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/2020/S2A_36QWD_20200701_0_L2A/TCI.tif
```

## Visualization Controls

### Band Mode
- **RGB**: Display bands 1-3 as Red/Green/Blue
- **Grayscale**: Display band 1 only
- **Custom**: Select specific bands

### Image Adjustments
- **Opacity**: 0-100% transparency
- **Brightness**: -50 to +50 adjustment
- **Contrast**: 0-200% scaling

## Troubleshooting

### WASM not found
```bash
# Build the WASM package
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --out-dir ../../demo/pkg
```

### CORS errors
- Use provided example datasets (CORS enabled)
- Ensure your COG server allows CORS

### Blank map
1. Check browser console for errors
2. Click "Fit to Bounds" button
3. Verify COG URL is accessible

## Browser Support

- Chrome/Edge 90+
- Firefox 88+
- Safari 14+

Requires WebAssembly and ES6 modules support.

## Next Steps

- Read the full [README.md](README.md) for detailed documentation
- Try different visualization modes
- Load your own COGs
- Check the metadata panel for image details

## Development

### Rebuild WASM after code changes

```bash
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --dev --out-dir ../../demo/pkg
```

### Watch for changes

Use a tool like `watchexec`:
```bash
watchexec -w ../../crates/oxigdal-wasm/src -w main.js -w style.css -- echo "Files changed"
```

## Performance Tips

- Use Cloud Optimized GeoTIFFs (COGs)
- Ensure COGs have overviews/pyramids
- Use compression (LZW, DEFLATE)
- Host COGs on CDN for faster loading

## Links

- **Full Documentation**: [README.md](README.md)
- **OxiGDAL Repository**: https://github.com/cool-japan/oxigdal
- **Report Issues**: https://github.com/cool-japan/oxigdal/issues

---

**Quick Start Complete!** Enjoy exploring COGs in the browser with OxiGDAL.
