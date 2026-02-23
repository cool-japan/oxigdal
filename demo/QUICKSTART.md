# Quick Start Guide

Get the OxiGDAL COG Viewer demo running in 3 simple steps!

## Prerequisites

- Rust (latest stable)
- wasm-pack: `cargo install wasm-pack`

## Steps

### 1. Build the WASM package

```bash
./build.sh
```

Or for a release build with optimizations:

```bash
./build.sh --release
```

### 2. Start a local web server

```bash
python3 -m http.server 8080
```

### 3. Open in browser

Navigate to: http://localhost:8080

## What's Next?

1. Click one of the example datasets or enter your own COG URL
2. Click "Load COG" to load the image
3. Use your mouse to pan (drag) and zoom (scroll wheel)
4. View metadata in the right panel

## Troubleshooting

**Problem**: "wasm-pack not found"
**Solution**: Install it with `cargo install wasm-pack`

**Problem**: "Failed to fetch wasm module"
**Solution**: Make sure you ran `./build.sh` first

**Problem**: CORS errors with custom COG URLs
**Solution**: The COG server must support CORS. Use example datasets to test.

## Next Steps

- Read the full [README.md](README.md) for deployment options
- Explore the source code in `app.js`
- Try loading your own COG files

Enjoy exploring geospatial data in the browser!
