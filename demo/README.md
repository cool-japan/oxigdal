# OxiGDAL COG Viewer - Browser Demo

A modern, interactive web application demonstrating OxiGDAL's WebAssembly capabilities for viewing Cloud Optimized GeoTIFFs (COGs) directly in the browser.

## Features

- **Pure Client-Side Processing**: No backend required - all processing happens in the browser via WebAssembly
- **COG Support**: Efficiently load and display Cloud Optimized GeoTIFFs using HTTP range requests
- **Interactive Viewer**: Pan and zoom capabilities with smooth rendering
- **Metadata Display**: Extract and display image metadata including dimensions, CRS, bands, and more
- **Tile-Based Rendering**: Efficient tile-based rendering on HTML Canvas
- **Modern UI**: Clean, responsive design that works on desktop and mobile

## Prerequisites

Before building and deploying the demo, ensure you have:

- **Rust** (latest stable version)
- **wasm-pack** for building WebAssembly packages
  ```bash
  cargo install wasm-pack
  ```
- A local web server for testing (options below)

## Building the WASM Package

1. Navigate to the oxigdal-wasm crate directory:
   ```bash
   cd crates/oxigdal-wasm
   ```

2. Build the WASM package:
   ```bash
   wasm-pack build --target web --out-dir ../../demo/pkg
   ```

   This will:
   - Compile the Rust code to WebAssembly
   - Generate JavaScript bindings
   - Output the package to `demo/pkg/`

3. Optional: Build with optimizations for production:
   ```bash
   wasm-pack build --target web --release --out-dir ../../demo/pkg
   ```

## Running Locally

### Option 1: Python HTTP Server

The simplest way to run the demo locally:

```bash
cd demo
python3 -m http.server 8080
```

Then open http://localhost:8080 in your browser.

### Option 2: Node.js http-server

If you have Node.js installed:

```bash
# Install http-server globally (one-time)
npm install -g http-server

# Run from demo directory
cd demo
http-server -p 8080 -c-1
```

### Option 3: Using a local web server

Any static file server will work. The key requirement is that it must:
- Serve the demo directory
- Support CORS headers (for fetching remote COG files)
- Serve with proper MIME types for `.wasm` files

## Deploying to Production

### GitHub Pages

1. Build the WASM package as described above

2. Commit the demo directory to your repository

3. Enable GitHub Pages:
   - Go to repository Settings > Pages
   - Select source: Deploy from a branch
   - Choose the branch containing the demo
   - Set folder to `/demo`

4. Your demo will be available at:
   ```
   https://your-username.github.io/oxigdal/
   ```

### Netlify

1. Build the WASM package

2. Create a `netlify.toml` in the demo directory:
   ```toml
   [build]
     publish = "."
     command = "echo 'No build needed'"

   [[headers]]
     for = "/*"
     [headers.values]
       Access-Control-Allow-Origin = "*"
   ```

3. Deploy:
   - Connect your repository to Netlify
   - Set base directory to `demo`
   - Deploy

### Vercel

1. Build the WASM package

2. Create a `vercel.json` in the demo directory:
   ```json
   {
     "headers": [
       {
         "source": "/(.*)",
         "headers": [
           { "key": "Access-Control-Allow-Origin", "value": "*" }
         ]
       }
     ]
   }
   ```

3. Deploy:
   ```bash
   cd demo
   vercel --prod
   ```

### Self-Hosted

1. Build the WASM package

2. Copy the entire demo directory to your web server:
   ```bash
   rsync -avz demo/ user@your-server:/var/www/oxigdal-demo/
   ```

3. Configure your web server (nginx example):
   ```nginx
   server {
       listen 80;
       server_name demo.oxigdal.com;
       root /var/www/oxigdal-demo;
       index index.html;

       location / {
           try_files $uri $uri/ =404;
           add_header Access-Control-Allow-Origin *;
       }

       location ~ \.wasm$ {
           types { application/wasm wasm; }
           add_header Access-Control-Allow-Origin *;
       }
   }
   ```

## Usage

1. **Load a COG**:
   - Enter a COG URL in the input field, or
   - Click one of the example datasets

2. **Navigate the Image**:
   - **Pan**: Click and drag the image
   - **Zoom In/Out**: Use the +/- buttons or mouse wheel
   - **Reset View**: Click "Reset View" to fit the image

3. **View Metadata**:
   - The right panel displays image metadata
   - Includes dimensions, tile size, bands, CRS, and more

## Example COG URLs

The demo includes several example datasets:

1. **Sentinel-2 RGB**: True color satellite imagery from Sentinel-2
2. **OpenAerialMap Haiti**: High-resolution aerial imagery
3. **Hurricane Harvey**: Disaster response imagery

You can also use any publicly accessible COG URL that supports CORS and HTTP range requests.

## CORS Considerations

For the demo to work with remote COG files, the server hosting the COG must:

1. Support HTTP range requests (Accept-Ranges: bytes header)
2. Allow CORS requests from your demo domain
3. Include proper CORS headers:
   ```
   Access-Control-Allow-Origin: *
   Access-Control-Allow-Methods: GET, HEAD
   Access-Control-Allow-Headers: Range
   ```

Common COG hosting services (AWS S3, Google Cloud Storage, Azure Blob Storage) can be configured to support these requirements.

## Troubleshooting

### WASM module not found

**Error**: `Failed to fetch wasm module`

**Solution**: Ensure you've built the WASM package:
```bash
cd crates/oxigdal-wasm
wasm-pack build --target web --out-dir ../../demo/pkg
```

### CORS errors

**Error**: `CORS policy: No 'Access-Control-Allow-Origin' header`

**Solution**: The COG server must allow CORS. Try:
- Using one of the example datasets
- Hosting your own COG with proper CORS headers
- Using a CORS proxy (for development only)

### Blank canvas

**Issue**: Canvas appears but no image is rendered

**Solution**:
- Check browser console for errors
- Ensure the COG URL is accessible
- Try resetting the view with the "Reset View" button

### Slow loading

**Issue**: COG takes a long time to load

**Solution**:
- Large COGs may take time to download tiles
- Try selecting a lower resolution overview (future feature)
- Ensure good internet connection

## Architecture

The demo application consists of:

- **index.html**: Main HTML structure and layout
- **styles.css**: Modern, responsive CSS styling
- **app.js**: JavaScript application logic and WASM integration
- **pkg/**: Generated WASM package and JS bindings (from wasm-pack build)

### Technology Stack

- **Frontend**: Vanilla JavaScript (ES6 modules), HTML5 Canvas
- **WASM**: OxiGDAL compiled to WebAssembly via wasm-pack
- **Styling**: Modern CSS with CSS Grid and Flexbox
- **No frameworks**: Pure web standards for minimal dependencies

## Performance

The demo is optimized for performance:

- **Tile Caching**: Downloaded tiles are cached in memory
- **Lazy Loading**: Only visible tiles are fetched and rendered
- **Efficient Rendering**: Canvas-based rendering with minimal redraws
- **WebAssembly**: Near-native performance for data processing

## Browser Compatibility

Tested and supported on:

- Chrome/Edge 90+
- Firefox 88+
- Safari 14+

Requires:
- WebAssembly support
- ES6 module support
- HTML5 Canvas API

## Development

To modify the demo:

1. Edit HTML/CSS/JS files in the `demo/` directory
2. Rebuild WASM if you modify Rust code:
   ```bash
   cd crates/oxigdal-wasm
   wasm-pack build --target web --out-dir ../../demo/pkg
   ```
3. Refresh your browser to see changes

## Contributing

Contributions are welcome! To contribute:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test the demo thoroughly
5. Submit a pull request

## License

This demo is part of the OxiGDAL project and is licensed under Apache-2.0.

## Links

- [OxiGDAL Repository](https://github.com/cool-japan/oxigdal)
- [COOLJAPAN Ecosystem](https://github.com/cool-japan)
- [Cloud Optimized GeoTIFF Specification](https://www.cogeo.org/)

## Support

For issues or questions:

- Open an issue on GitHub
- Check the main OxiGDAL documentation
- Review the browser console for error messages

---

Built with OxiGDAL | Part of the COOLJAPAN Pure Rust Ecosystem
