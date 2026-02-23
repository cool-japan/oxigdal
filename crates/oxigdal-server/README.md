# OxiGDAL Tile Server

WMS/WMTS tile server for serving geospatial raster data over HTTP, powered by OxiGDAL.

## Features

- **WMS 1.3.0**: Full Web Map Service support
  - GetCapabilities
  - GetMap
  - GetFeatureInfo

- **WMTS 1.0.0**: Web Map Tile Service
  - GetCapabilities
  - GetTile (KVP and RESTful)
  - Multiple tile matrix sets (Web Mercator, WGS84)

- **XYZ Tiles**: Simple tile serving
  - Compatible with Leaflet, MapLibre, OpenLayers
  - TileJSON support

- **High Performance**
  - Async/await with Tokio
  - Multi-level caching (memory + disk)
  - LRU cache eviction
  - Configurable tile sizes

- **Pure Rust**
  - No C/C++ dependencies
  - Memory safe
  - Fast compilation

## Installation

### From Source

```bash
cd crates/oxigdal-server
cargo build --release
```

### Using Cargo

```bash
cargo install --path crates/oxigdal-server
```

## Quick Start

### 1. Generate a Configuration File

```bash
oxigdal-server --generate-config config.toml
```

### 2. Edit the Configuration

Edit `config.toml` to add your datasets:

```toml
[[layers]]
name = "landsat"
path = "/path/to/landsat.tif"
formats = ["png", "jpeg"]
tile_size = 256
```

### 3. Start the Server

```bash
oxigdal-server --config config.toml
```

The server will start on `http://0.0.0.0:8080` by default.

## Configuration

### Server Settings

```toml
[server]
host = "0.0.0.0"      # Bind address
port = 8080            # Bind port
workers = 4            # Number of worker threads (0 = auto)
enable_cors = true     # Enable CORS
```

### Cache Settings

```toml
[cache]
memory_size_mb = 256           # In-memory cache size
disk_cache = "/tmp/cache"      # Optional disk cache
ttl_seconds = 3600             # Cache TTL
```

### Layer Configuration

```toml
[[layers]]
name = "my-layer"              # Layer identifier
title = "My Layer"             # Display name
path = "/data/layer.tif"       # Path to GeoTIFF
formats = ["png", "jpeg"]      # Supported formats
tile_size = 256                # Tile size in pixels
min_zoom = 0                   # Minimum zoom level
max_zoom = 18                  # Maximum zoom level
```

## API Endpoints

### WMS

```
GET /wms?SERVICE=WMS&REQUEST=GetCapabilities
GET /wms?SERVICE=WMS&REQUEST=GetMap&LAYERS=landsat&BBOX=-180,-90,180,90&WIDTH=512&HEIGHT=512&FORMAT=image/png
GET /wms?SERVICE=WMS&REQUEST=GetFeatureInfo&...
```

### WMTS

```
GET /wmts?SERVICE=WMTS&REQUEST=GetCapabilities
GET /wmts/1.0.0/{layer}/{tileMatrixSet}/{z}/{x}/{y}.png
```

### XYZ Tiles

```
GET /tiles/{layer}/{z}/{x}/{y}.png
GET /tiles/{layer}/tilejson
```

### Utility Endpoints

```
GET /              # Landing page
GET /health        # Health check
GET /stats         # Cache statistics
```

## Usage Examples

### Leaflet

```javascript
const map = L.map('map').setView([0, 0], 2);

L.tileLayer('http://localhost:8080/tiles/landsat/{z}/{x}/{y}.png', {
    attribution: 'OxiGDAL Tile Server',
    maxZoom: 18
}).addTo(map);
```

### MapLibre GL JS

```javascript
const map = new maplibregl.Map({
    container: 'map',
    style: {
        version: 8,
        sources: {
            'landsat': {
                type: 'raster',
                tiles: ['http://localhost:8080/tiles/landsat/{z}/{x}/{y}.png'],
                tileSize: 256
            }
        },
        layers: [{
            id: 'landsat',
            type: 'raster',
            source: 'landsat'
        }]
    }
});
```

### QGIS

1. Add WMS/WMTS connection
2. URL: `http://localhost:8080/wms` or `http://localhost:8080/wmts`
3. Select layers to display

## Command-Line Options

```bash
oxigdal-server [OPTIONS]

Options:
  -c, --config <FILE>              Configuration file path
      --host <HOST>                Host address (env: OXIGDAL_HOST)
  -p, --port <PORT>                Port number (env: OXIGDAL_PORT)
  -w, --workers <WORKERS>          Number of workers (env: OXIGDAL_WORKERS)
      --log-level <LEVEL>          Log level [default: info]
      --generate-config <FILE>     Generate default config file
  -h, --help                       Print help
  -V, --version                    Print version
```

## Environment Variables

- `OXIGDAL_HOST`: Server host address
- `OXIGDAL_PORT`: Server port
- `OXIGDAL_WORKERS`: Number of worker threads
- `OXIGDAL_LOG_LEVEL`: Log level (trace, debug, info, warn, error)

## Docker

### Build

```bash
docker build -t oxigdal-server .
```

### Run

```bash
docker run -d \
  -p 8080:8080 \
  -v /path/to/data:/data:ro \
  -v /path/to/config.toml:/etc/oxigdal/config.toml:ro \
  oxigdal-server
```

## Performance Tuning

### Cache Size

Increase memory cache for better performance:

```toml
[cache]
memory_size_mb = 1024  # 1 GB
```

### Workers

Set workers based on your CPU:

```toml
[server]
workers = 8  # Or use 0 for auto-detection
```

### Disk Cache

Enable disk cache for persistence:

```toml
[cache]
disk_cache = "/var/cache/oxigdal"
```

## Monitoring

### Cache Statistics

```bash
curl http://localhost:8080/stats
```

Returns cache hit/miss rates and memory usage.

### Health Check

```bash
curl http://localhost:8080/health
```

Returns server status.

## Troubleshooting

### Server won't start

1. Check configuration file syntax
2. Verify dataset paths exist
3. Ensure port is not in use
4. Check log output with `--log-level debug`

### Tiles not rendering

1. Verify layer is enabled in config
2. Check dataset format is supported
3. Verify tile coordinates are valid
4. Check server logs for errors

### Performance issues

1. Increase cache size
2. Enable disk caching
3. Adjust worker count
4. Use appropriate tile sizes

## Development

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Running locally

```bash
cargo run -- --config examples/config.toml
```

## License

Apache-2.0

## Contributing

Contributions are welcome! Please see the main OxiGDAL repository.

## Authors

COOLJAPAN OU (Team Kitasan)
