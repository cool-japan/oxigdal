/**
 * OxiGDAL Advanced COG Viewer - Main Application
 *
 * Features:
 * - Leaflet-based interactive map
 * - OxiGDAL WASM integration for COG reading
 * - Advanced band selection and visualization
 * - Performance monitoring
 * - Comprehensive error handling
 * - Tile caching and optimization
 */

// Import WASM module from pkg directory (symlinked to crates/oxigdal-wasm/pkg)
import init, { WasmCogViewer, WasmTerrain, version } from './pkg/oxigdal_wasm.js';
// Vector format parsers (self-contained; keeps this file under 2000 lines).
import {
    isVectorFormat,
    detectVectorFormat,
    loadGeoJSON,
    loadFlatGeobuf,
    loadShapefile,
    loadGPX,
    loadKML,
    loadTopoJSON,
    loadGeoParquet,
} from './formats.js';
// Pure terrain raster helpers (self-contained; keeps this file under 2000 lines).
import {
    terrainLabel,
    grayToImageData,
    slopeToImageData,
    applyNodataAlpha,
} from './terrain-render.js';

// Application state
const app = {
    // WASM
    viewer: null,
    wasmInitialized: false,

    // Map
    map: null,
    cogLayer: null,

    // Vector layers
    vectorLayers: new Map(), // Map of layer name -> Leaflet layer
    currentVector: {
        url: null,
        format: null,
        features: [],
        bounds: null,
    },

    // Current COG info
    currentCog: {
        url: null,
        metadata: null,
        width: 0,
        height: 0,
        tileWidth: 256,
        tileHeight: 256,
        bandCount: 0,
        bounds: null,
    },

    // Visualization settings
    visualization: {
        bandMode: 'rgb',
        customBands: { r: 1, g: 2, b: 3 },
        opacity: 1.0,
        brightness: 0,
        contrast: 100,
    },

    // Performance tracking
    performance: {
        loadStartTime: 0,
        loadEndTime: 0,
        tilesLoaded: 0,
        tilesCached: 0,
        dataTransferred: 0,
        // Honest network accounting:
        //  - bytesUploaded is always 0: local files are decoded in-browser and
        //    never leave the machine (literal truth, not a placeholder).
        //  - bytesFetched is the LIVE sum of Content-Length over every network
        //    data fetch (COG range requests + vector files), measured by the
        //    fetch wrapper in installFetchAccounting(); app assets are excluded.
        bytesUploaded: 0,
        bytesFetched: 0,
    },

    // Terrain analysis state (browser-side WasmTerrain over the DEM's f32 grid)
    terrain: {
        mode: null, // null | 'hillshade' | 'multidirectional' | 'slope' | 'colorrelief'
        azimuth: 315,
        altitude: 45,
        zFactor: 1.3,
        palette: 'viridis',
    },

    // Tile cache
    tileCache: new Map(),

    // Measurement tools
    measurements: {
        active: false,
        type: null, // 'distance' or 'area'
        coordinates: [],
        layer: null,
        polyline: null,
        polygon: null,
        markers: [],
        label: null,
    },
};

/**
 * Initialize the application
 */
async function initializeApp() {
    try {
        updateStatus('loading', 'Initializing WebAssembly...');
        showLoading('Initializing OxiGDAL WASM module...');

        // Install the network byte accounting before anything fetches data.
        installFetchAccounting();
        updateNetworkBadges();

        // Initialize WASM
        await init();
        app.wasmInitialized = true;

        // Display version
        const versionBadge = document.getElementById('version-badge');
        versionBadge.textContent = `v${version()}`;

        console.log('OxiGDAL WASM initialized successfully');

        // Initialize Leaflet map
        initializeMap();

        // Setup event listeners
        setupEventListeners();

        updateStatus('ready', 'Ready');
        hideLoading();

        console.log('Application initialized successfully');
    } catch (error) {
        console.error('Failed to initialize application:', error);
        showError('Failed to initialize application: ' + error.message);
        updateStatus('error', 'Initialization failed');
    }
}

/**
 * Initialize Leaflet map
 */
function initializeMap() {
    // Create map centered at (0, 0)
    app.map = L.map('map-container', {
        center: [0, 0],
        zoom: 2,
        minZoom: 1,
        maxZoom: 20,
        zoomControl: true,
        attributionControl: true,
    });

    // Add OpenStreetMap base layer
    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
        attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
        maxZoom: 19,
    }).addTo(app.map);

    // Map event handlers
    app.map.on('moveend', updateMapInfo);
    app.map.on('zoomend', updateMapInfo);
    app.map.on('click', handleMapClick);

    console.log('Leaflet map initialized');
}

/**
 * Setup event listeners
 */
function setupEventListeners() {
    // Load button
    document.getElementById('load-btn').addEventListener('click', loadCogFromInput);

    // URL input - Enter key
    document.getElementById('cog-url-input').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') {
            loadCogFromInput();
        }
    });

    // Example cards
    document.querySelectorAll('.example-card').forEach(card => {
        card.addEventListener('click', (e) => {
            const button = e.currentTarget;
            const url = button.dataset.url;
            const name = button.dataset.name;
            document.getElementById('cog-url-input').value = url;

            // Detect if vector or raster format
            if (isVectorFormat(url)) {
                loadVector(url, name);
            } else {
                // Sample rasters are same-origin local files: fetch the whole
                // file and decode via openBytes (full codec + elevation support,
                // and robust on servers that do not honour HTTP Range requests).
                loadLocalRaster(url, name);
            }
        });
    });

    // Band mode selector
    document.getElementById('band-mode').addEventListener('change', (e) => {
        app.visualization.bandMode = e.target.value;
        const customBands = document.getElementById('custom-bands');
        customBands.style.display = e.target.value === 'custom' ? 'block' : 'none';
        refreshVisualization();
    });

    // Custom band inputs
    ['red-band', 'green-band', 'blue-band'].forEach((id, index) => {
        document.getElementById(id).addEventListener('change', (e) => {
            const band = parseInt(e.target.value) || 1;
            const channels = ['r', 'g', 'b'];
            app.visualization.customBands[channels[index]] = band;
            refreshVisualization();
        });
    });

    // Opacity slider
    const opacitySlider = document.getElementById('opacity-slider');
    opacitySlider.addEventListener('input', (e) => {
        const value = parseInt(e.target.value);
        app.visualization.opacity = value / 100;
        document.getElementById('opacity-value').textContent = `${value}%`;
        updateLayerOpacity();
    });

    // Brightness slider
    const brightnessSlider = document.getElementById('brightness-slider');
    brightnessSlider.addEventListener('input', (e) => {
        const value = parseInt(e.target.value);
        app.visualization.brightness = value;
        document.getElementById('brightness-value').textContent = value.toString();
        refreshVisualization();
    });

    // Contrast slider
    const contrastSlider = document.getElementById('contrast-slider');
    contrastSlider.addEventListener('input', (e) => {
        const value = parseInt(e.target.value);
        app.visualization.contrast = value;
        document.getElementById('contrast-value').textContent = `${value}%`;
        refreshVisualization();
    });

    // Reset visualization
    document.getElementById('reset-visualization').addEventListener('click', resetVisualization);

    // Fit bounds button
    document.getElementById('fit-bounds-btn').addEventListener('click', fitToBounds);

    // Toggle grid button
    document.getElementById('toggle-grid-btn').addEventListener('click', toggleGrid);

    // Dismiss error
    document.getElementById('dismiss-error').addEventListener('click', hideError);

    // Measurement tools
    const measureDistanceBtn = document.getElementById('measure-distance');
    if (measureDistanceBtn) {
        measureDistanceBtn.addEventListener('click', () => startMeasurement('distance'));
    }

    const measureAreaBtn = document.getElementById('measure-area');
    if (measureAreaBtn) {
        measureAreaBtn.addEventListener('click', () => startMeasurement('area'));
    }

    const clearMeasurementsBtn = document.getElementById('clear-measurements');
    if (clearMeasurementsBtn) {
        clearMeasurementsBtn.addEventListener('click', clearMeasurements);
    }

    // Layer controls
    const clearLayersBtn = document.getElementById('clear-layers-btn');
    if (clearLayersBtn) {
        clearLayersBtn.addEventListener('click', () => {
            clearVectorLayers();
            updateLayerControls();
        });
    }

    // Drag-and-drop + local file input (in-browser decode via openBytes)
    setupFileDrop();

    // Terrain analysis controls
    setupTerrainControls();
}

/**
 * Wire the drag-and-drop zone and the "Choose file…" input so a local
 * GeoTIFF can be decoded entirely in the browser (openBytes), with nothing
 * ever uploaded to a server.
 */
function setupFileDrop() {
    const dropZone = document.getElementById('drop-zone');
    const fileInput = document.getElementById('file-input');
    const mapEl = document.getElementById('map-container');

    if (fileInput) {
        fileInput.addEventListener('change', (e) => {
            const file = e.target.files && e.target.files[0];
            if (file) {
                handleLocalFile(file);
            }
            // Reset so selecting the same file again re-triggers change.
            e.target.value = '';
        });
    }

    const dropTargets = [dropZone, mapEl].filter(Boolean);
    for (const target of dropTargets) {
        target.addEventListener('dragover', (e) => {
            e.preventDefault();
            if (e.dataTransfer) {
                e.dataTransfer.dropEffect = 'copy';
            }
            if (dropZone) {
                dropZone.classList.add('drop-zone-hover');
            }
        });
        target.addEventListener('dragleave', () => {
            if (dropZone) {
                dropZone.classList.remove('drop-zone-hover');
            }
        });
        target.addEventListener('drop', (e) => {
            e.preventDefault();
            if (dropZone) {
                dropZone.classList.remove('drop-zone-hover');
            }
            const file = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
            if (file) {
                handleLocalFile(file);
            }
        });
    }
}

/**
 * Wire the Terrain Analysis buttons, sliders and palette selector.
 */
function setupTerrainControls() {
    const modeButtons = {
        'terrain-hillshade': 'hillshade',
        'terrain-multidir': 'multidirectional',
        'terrain-slope': 'slope',
        'terrain-colorrelief': 'colorrelief',
    };
    for (const [id, mode] of Object.entries(modeButtons)) {
        const btn = document.getElementById(id);
        if (btn) {
            btn.addEventListener('click', () => setTerrainMode(mode));
        }
    }

    const azimuth = document.getElementById('terrain-azimuth');
    if (azimuth) {
        azimuth.addEventListener('input', (e) => {
            app.terrain.azimuth = parseInt(e.target.value, 10);
            const label = document.getElementById('terrain-azimuth-value');
            if (label) label.textContent = `${app.terrain.azimuth}°`;
            if (app.terrain.mode) scheduleTerrainRerender();
        });
    }

    const altitude = document.getElementById('terrain-altitude');
    if (altitude) {
        altitude.addEventListener('input', (e) => {
            app.terrain.altitude = parseInt(e.target.value, 10);
            const label = document.getElementById('terrain-altitude-value');
            if (label) label.textContent = `${app.terrain.altitude}°`;
            if (app.terrain.mode) scheduleTerrainRerender();
        });
    }

    const zfactor = document.getElementById('terrain-zfactor');
    if (zfactor) {
        zfactor.addEventListener('input', (e) => {
            app.terrain.zFactor = parseFloat(e.target.value);
            const label = document.getElementById('terrain-zfactor-value');
            if (label) label.textContent = app.terrain.zFactor.toFixed(1);
            if (app.terrain.mode) scheduleTerrainRerender();
        });
    }

    const palette = document.getElementById('terrain-palette');
    if (palette) {
        palette.addEventListener('change', (e) => {
            app.terrain.palette = e.target.value;
            if (app.terrain.mode === 'colorrelief') scheduleTerrainRerender();
        });
    }

    const clearBtn = document.getElementById('terrain-clear');
    if (clearBtn) {
        clearBtn.addEventListener('click', () => {
            app.terrain.mode = null;
            updateTerrainButtons();
            if (app.viewer) {
                createCogTileLayer();
            }
            updateStatus('ready', 'Terrain cleared');
        });
    }
}

/**
 * Load COG from input field
 */
async function loadCogFromInput() {
    const url = document.getElementById('cog-url-input').value.trim();
    if (!url) {
        showError('Please enter a COG URL');
        return;
    }
    await loadCog(url);
}

/**
 * Load a COG from a remote URL using HTTP range requests (the cloud-native
 * path). Requires the server to honour `Range` headers and CORS.
 */
async function loadCog(url, name = null) {
    try {
        app.performance.loadStartTime = performance.now();

        updateStatus('loading', `Loading ${name || 'COG'}...`);
        showLoading(`Loading COG: ${name || url}`);
        updateProgress(10);

        // Fresh viewer per load avoids stale internal reader state.
        app.viewer = new WasmCogViewer();
        updateProgress(20);

        // Open the COG over HTTP range requests.
        await app.viewer.open(url);

        // Shared post-open display pipeline.
        displayCogOnMap(name || url);

        console.log(`Successfully loaded COG: ${url}`);
    } catch (error) {
        console.error('Failed to load COG:', error);
        showError(`Failed to load COG: ${error.message || error}`);
        updateStatus('error', 'Failed to load COG');
        hideLoading();
    }
}

/**
 * Load a same-origin raster by fetching the whole file and decoding it with
 * `openBytes`. This is robust on servers without HTTP Range support and gives
 * the full codec + elevation-decode path needed for Terrain Analysis.
 */
async function loadLocalRaster(url, name = null) {
    try {
        app.performance.loadStartTime = performance.now();

        updateStatus('loading', `Loading ${name || 'raster'}...`);
        showLoading(`Loading raster: ${name || url}`);
        updateProgress(15);

        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        const buffer = await response.arrayBuffer();
        updateProgress(45);

        app.viewer = new WasmCogViewer();
        app.viewer.openBytes(new Uint8Array(buffer), name || url);

        displayCogOnMap(name || url);

        console.log(`Successfully loaded raster: ${url}`);
    } catch (error) {
        console.error('Failed to load raster:', error);
        showError(`Failed to load raster: ${error.message || error}`);
        updateStatus('error', 'Failed to load raster');
        hideLoading();
    }
}

/**
 * Handle a locally chosen / dropped GeoTIFF: read its bytes in-browser and
 * decode via `openBytes`. Nothing is uploaded — bytesUploaded stays 0.
 */
async function handleLocalFile(file) {
    const name = file.name || 'local.tif';
    if (!/\.(tif|tiff)$/i.test(name)) {
        showError('Please choose a GeoTIFF (.tif / .tiff) file.');
        return;
    }
    try {
        app.performance.loadStartTime = performance.now();

        updateStatus('loading', `Reading ${name}...`);
        showLoading(`Reading local file: ${name}`);
        updateProgress(15);

        // Read the file entirely in the browser; it never leaves the machine.
        const buffer = await file.arrayBuffer();
        updateProgress(40);

        app.viewer = new WasmCogViewer();
        app.viewer.openBytes(new Uint8Array(buffer), name);

        displayCogOnMap(name);

        // Reflect the (unchanged, zero) upload counter after a local load.
        updateNetworkBadges();

        console.log(`Successfully decoded local file: ${name} (${buffer.byteLength} bytes, 0 uploaded)`);
    } catch (error) {
        console.error('Failed to read local file:', error);
        showError(`Failed to open local GeoTIFF: ${error.message || error}`);
        updateStatus('error', 'Failed to open file');
        hideLoading();
    }
}

/**
 * Shared post-open display pipeline used by every raster entry point
 * (URL range-open, local sample fetch+openBytes, and drag-drop openBytes).
 * Extracts metadata, computes bounds, and renders the raster on the map.
 */
function displayCogOnMap(name) {
    try {
        updateProgress(55);

        app.performance.tilesLoaded = 0;
        app.performance.tilesCached = 0;
        app.performance.dataTransferred = 0;
        app.tileCache.clear();

        // Extract metadata.
        app.currentCog.url = app.viewer.url() || name || '';
        app.currentCog.width = app.viewer.width();
        app.currentCog.height = app.viewer.height();
        app.currentCog.tileWidth = app.viewer.tile_width();
        app.currentCog.tileHeight = app.viewer.tile_height();
        app.currentCog.bandCount = app.viewer.band_count();

        const metadataJson = app.viewer.metadata_json();
        app.currentCog.metadata = JSON.parse(metadataJson);
        console.log('COG Metadata:', app.currentCog.metadata);

        updateProgress(70);

        // Calculate bounds from geotransform.
        app.currentCog.bounds = calculateBounds();

        // A new dataset always starts in raw-raster view.
        app.terrain.mode = null;
        updateTerrainButtons();

        updateProgress(80);

        // Create tile layer (auto-detects single vs multi-tile).
        createCogTileLayer();

        updateProgress(90);

        displayMetadata();
        fitToBounds();

        updateProgress(100);

        app.performance.loadEndTime = performance.now();
        updatePerformanceDisplay();

        updateStatus('ready', `${name || 'Raster'} loaded successfully`);
        hideLoading();
    } catch (error) {
        console.error('Failed to display raster:', error);
        showError(`Failed to display raster: ${error.message || error}`);
        updateStatus('error', 'Failed to display raster');
        hideLoading();
    }
}

/**
 * Calculate geographic bounds from GeoTIFF geotransform
 * Uses ModelPixelScale and ModelTiepoint tags to determine actual bounds
 * Handles both EPSG:4326 (lat/lon) and EPSG:3857 (Web Mercator) coordinate systems
 */
function calculateBounds() {
    // Get geotransform from WASM viewer
    const pixelScaleX = app.viewer.pixel_scale_x();
    const pixelScaleY = app.viewer.pixel_scale_y();
    const tiepointGeoX = app.viewer.tiepoint_geo_x();
    const tiepointGeoY = app.viewer.tiepoint_geo_y();
    const width = app.viewer.width();
    const height = app.viewer.height();

    // Check if we have geotransform data
    if (pixelScaleX === undefined || pixelScaleY === undefined ||
        tiepointGeoX === undefined || tiepointGeoY === undefined) {
        console.warn('No geotransform data available, using default bounds');
        // Fallback to default bounds
        return [
            [40, -120],
            [50, -110]
        ];
    }

    // Convert BigInt to Number for calculations
    const widthNum = Number(width);
    const heightNum = Number(height);

    // Check if coordinates are in Web Mercator (EPSG:3857) or lat/lon (EPSG:4326)
    // Web Mercator coordinates are typically very large (millions)
    const isWebMercator = Math.abs(tiepointGeoX) > 360 || Math.abs(tiepointGeoY) > 180;

    let minLon, maxLon, minLat, maxLat;

    if (isWebMercator) {
        console.log('Detected Web Mercator (EPSG:3857) coordinates');

        // Calculate bounds in Web Mercator
        const minX = tiepointGeoX;
        const maxY = tiepointGeoY;
        const maxX = minX + (widthNum * pixelScaleX);
        const minY = maxY - (heightNum * pixelScaleY); // Y increases upward in Web Mercator

        // Convert Web Mercator to WGS84 lat/lon
        const swCorner = webMercatorToLatLon(minX, minY);
        const neCorner = webMercatorToLatLon(maxX, maxY);

        minLon = swCorner.lon;
        minLat = swCorner.lat;
        maxLon = neCorner.lon;
        maxLat = neCorner.lat;

        console.log('Web Mercator bounds:', { minX, minY, maxX, maxY });
        console.log('Converted to lat/lon:', { minLat, minLon, maxLat, maxLon });
    } else {
        console.log('Detected geographic (EPSG:4326) coordinates');

        // Lat/lon coordinates (EPSG:4326)
        minLon = tiepointGeoX;
        maxLat = tiepointGeoY;
        maxLon = minLon + (widthNum * pixelScaleX);
        minLat = maxLat + (heightNum * pixelScaleY); // pixelScaleY is negative for lat/lon

        console.log('Geographic bounds:', { minLon, maxLat, maxLon, minLat });
    }

    return [
        [minLat, minLon],  // Southwest corner
        [maxLat, maxLon]   // Northeast corner
    ];
}

/**
 * Convert Web Mercator (EPSG:3857) coordinates to WGS84 lat/lon (EPSG:4326)
 */
function webMercatorToLatLon(x, y) {
    const lon = (x / 20037508.34) * 180;
    let lat = (y / 20037508.34) * 180;
    lat = 180 / Math.PI * (2 * Math.atan(Math.exp(lat * Math.PI / 180)) - Math.PI / 2);
    return { lat, lon };
}

/**
 * Create COG tile layer (supports both single-tile and multi-tile GeoTIFFs)
 */
function createCogTileLayer() {
    // Remove existing layer
    if (app.cogLayer) {
        app.map.removeLayer(app.cogLayer);
    }

    // Check if this is a single-tile GeoTIFF
    const isSingleTile = (Number(app.currentCog.width) <= Number(app.currentCog.tileWidth) &&
                          Number(app.currentCog.height) <= Number(app.currentCog.tileHeight));

    if (isSingleTile) {
        // Use ImageOverlay for single-tile GeoTIFFs
        createImageOverlay();
    } else {
        // Use GridLayer for multi-tile GeoTIFFs
        createGridLayer();
    }
}

/**
 * Create image overlay for single-tile GeoTIFFs
 */
async function createImageOverlay() {
    try {
        // Load the single tile (0, 0)
        const imageData = await app.viewer.read_tile_as_image_data(0, 0, 0);

        // Create canvas and render the tile
        const canvas = document.createElement('canvas');
        canvas.width = Number(app.currentCog.tileWidth);
        canvas.height = Number(app.currentCog.tileHeight);
        const ctx = canvas.getContext('2d');
        ctx.putImageData(imageData, 0, 0);

        // Apply visualization settings
        applyVisualization(ctx, canvas.width, canvas.height);

        // Convert canvas to data URL
        const dataUrl = canvas.toDataURL('image/png');

        // Create image overlay with geographic bounds
        app.cogLayer = L.imageOverlay(dataUrl, app.currentCog.bounds, {
            opacity: app.visualization.opacity,
            interactive: false,
        });

        app.cogLayer.addTo(app.map);

        // Update stats
        app.performance.tilesLoaded = 1;
        app.performance.dataTransferred = imageData.data.length;
        updateTileInfo();
        updatePerformanceDisplay();

        console.log('COG image overlay created successfully');
    } catch (error) {
        console.error('Failed to create COG overlay:', error);
        showError(`Failed to display COG: ${error.message}`);
    }
}

/**
 * Create grid layer for multi-tile GeoTIFFs
 */
function createGridLayer() {
    // Create custom tile layer with coordinate transformation
    const CogTileLayer = L.GridLayer.extend({
        createTile: function(coords, done) {
            const tile = document.createElement('canvas');
            tile.width = Number(app.currentCog.tileWidth);
            tile.height = Number(app.currentCog.tileHeight);

            const ctx = tile.getContext('2d');

            // Convert Leaflet tile coords to GeoTIFF tile coords
            const geotiffCoords = leafletToGeotiffTileCoords(coords);

            if (geotiffCoords) {
                // Load tile data asynchronously
                loadTileData(geotiffCoords.level, geotiffCoords.x, geotiffCoords.y)
                    .then(imageData => {
                        if (imageData) {
                            ctx.putImageData(imageData, 0, 0);
                            applyVisualization(ctx, tile.width, tile.height);
                        }
                        done(null, tile);
                    })
                    .catch(error => {
                        console.error(`Failed to load tile ${coords.z}/${coords.x}/${coords.y}:`, error);
                        // Draw empty tile
                        ctx.fillStyle = 'rgba(0, 0, 0, 0)';
                        ctx.fillRect(0, 0, tile.width, tile.height);
                        done(null, tile);
                    });
            } else {
                // Tile is outside COG bounds - return empty tile
                ctx.fillStyle = 'rgba(0, 0, 0, 0)';
                ctx.fillRect(0, 0, tile.width, tile.height);
                done(null, tile);
            }

            return tile;
        }
    });

    app.cogLayer = new CogTileLayer({
        tileSize: 256,
        opacity: app.visualization.opacity,
        bounds: app.currentCog.bounds,
        minZoom: 0,
        maxZoom: 20,
    });

    app.cogLayer.addTo(app.map);
    console.log('COG grid layer created successfully');
}

// ─── Terrain analysis ────────────────────────────────────────────────────────

let terrainRerenderTimer = null;

/**
 * Debounced terrain re-render, so dragging a slider doesn't spawn a compute
 * storm — only the last value within the window is rendered.
 */
function scheduleTerrainRerender() {
    if (terrainRerenderTimer) {
        clearTimeout(terrainRerenderTimer);
    }
    terrainRerenderTimer = setTimeout(() => {
        terrainRerenderTimer = null;
        renderTerrainOverlay();
    }, 140);
}

/**
 * Select (or toggle off) a terrain product and render it.
 */
function setTerrainMode(mode) {
    if (!app.viewer) {
        showError('Load a single-band elevation raster first (e.g. the San Francisco DEM).');
        return;
    }
    // Clicking the active product again returns to the raw raster.
    app.terrain.mode = app.terrain.mode === mode ? null : mode;
    updateTerrainButtons();

    if (app.terrain.mode === null) {
        createCogTileLayer();
        updateStatus('ready', 'Terrain cleared');
        return;
    }
    renderTerrainOverlay();
}

/**
 * Toggle the active-state class on the four terrain product buttons.
 */
function updateTerrainButtons() {
    const idByMode = {
        hillshade: 'terrain-hillshade',
        multidirectional: 'terrain-multidir',
        slope: 'terrain-slope',
        colorrelief: 'terrain-colorrelief',
    };
    for (const [mode, id] of Object.entries(idByMode)) {
        const btn = document.getElementById(id);
        if (btn) {
            btn.classList.toggle('terrain-btn-active', app.terrain.mode === mode);
        }
    }
}

/**
 * Assemble the DEM's full elevation grid from its native tiles, run the chosen
 * WasmTerrain product over the whole grid (so there are no per-tile seams),
 * and place the result as a single georeferenced image overlay.
 *
 * Reading `readTileElevation` for every native tile and feeding the assembled
 * grid to `WasmTerrain.{hillshade,hillshadeMultidirectional,slope,colorReliefShaded}`
 * is the correct, artefact-free equivalent of a per-tile pipeline for a
 * bounded DEM, and reuses the existing image-overlay render path.
 */
async function renderTerrainOverlay() {
    if (!app.viewer || !app.terrain.mode) {
        return;
    }
    try {
        updateStatus('loading', `Computing ${terrainLabel(app.terrain.mode)}...`);
        showLoading(`Terrain: ${terrainLabel(app.terrain.mode)}`);
        updateProgress(15);

        const width = Number(app.viewer.width());
        const height = Number(app.viewer.height());
        const tileWidth = Number(app.viewer.tile_width());
        const tileHeight = Number(app.viewer.tile_height());
        if (!width || !height || !tileWidth || !tileHeight) {
            throw new Error('Invalid raster dimensions for terrain analysis');
        }

        const bandCount = app.viewer.band_count();
        if (bandCount && bandCount > 1) {
            console.warn(`Terrain: raster has ${bandCount} bands; using band 1 as elevation.`);
        }

        const grid = await assembleElevationGrid(width, height, tileWidth, tileHeight);
        updateProgress(80);

        const cellSize = app.viewer.pixel_scale_x() || 30;
        const az = app.terrain.azimuth;
        const alt = app.terrain.altitude;
        const z = app.terrain.zFactor;

        let imageData;
        switch (app.terrain.mode) {
            case 'colorrelief':
                imageData = WasmTerrain.colorReliefShaded(
                    grid.elev, width, height, cellSize,
                    app.terrain.palette, grid.min, grid.max, alt, z
                );
                break;
            case 'hillshade': {
                const gray = WasmTerrain.hillshade(grid.elev, width, height, cellSize, az, alt, z);
                imageData = grayToImageData(gray, width, height);
                break;
            }
            case 'multidirectional': {
                const gray = WasmTerrain.hillshadeMultidirectional(grid.elev, width, height, cellSize, alt, z);
                imageData = grayToImageData(gray, width, height);
                break;
            }
            case 'slope': {
                const slopeDeg = WasmTerrain.slope(grid.elev, width, height, cellSize, z);
                imageData = slopeToImageData(slopeDeg, width, height);
                break;
            }
            default:
                throw new Error(`Unknown terrain mode: ${app.terrain.mode}`);
        }

        // Make nodata cells transparent so the basemap shows through.
        applyNodataAlpha(imageData, grid.mask);

        const canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext('2d');
        ctx.putImageData(imageData, 0, 0);
        const dataUrl = canvas.toDataURL('image/png');

        if (app.cogLayer) {
            app.map.removeLayer(app.cogLayer);
        }
        app.cogLayer = L.imageOverlay(dataUrl, app.currentCog.bounds, {
            opacity: app.visualization.opacity,
            interactive: false,
        });
        app.cogLayer.addTo(app.map);

        updateProgress(100);
        updateStatus('ready', `Terrain: ${terrainLabel(app.terrain.mode)}`);
        hideLoading();
        console.log(`Rendered terrain "${app.terrain.mode}" over ${width}x${height} grid`);
    } catch (error) {
        console.error('Terrain render failed:', error);
        showError(`Terrain analysis failed: ${error.message || error}`);
        updateStatus('error', 'Terrain failed');
        hideLoading();
    }
}

/**
 * Read `readTileElevation` for every native tile and stitch a single row-major
 * `Float32Array` of size width*height. Nodata (−32768 / non-finite) samples are
 * tracked in a mask and replaced with the valid minimum so WasmTerrain math
 * stays finite. Returns `{ elev, mask, min, max }`.
 */
async function assembleElevationGrid(width, height, tileWidth, tileHeight) {
    const nTilesX = Math.ceil(width / tileWidth);
    const nTilesY = Math.ceil(height / tileHeight);

    const elev = new Float32Array(width * height);
    const mask = new Uint8Array(width * height); // 1 = valid sample
    const NODATA = -32768;
    let vMin = Infinity;
    let vMax = -Infinity;

    for (let ty = 0; ty < nTilesY; ty++) {
        for (let tx = 0; tx < nTilesX; tx++) {
            let tileElev;
            try {
                tileElev = await app.viewer.readTileElevation(0, tx, ty);
            } catch (e) {
                console.warn(`Terrain: elevation read failed for tile (${tx}, ${ty}):`, e);
                continue;
            }
            const cols = Math.min(tileWidth, width - tx * tileWidth);
            const rows = Math.min(tileHeight, height - ty * tileHeight);
            for (let r = 0; r < rows; r++) {
                const gy = ty * tileHeight + r;
                for (let c = 0; c < cols; c++) {
                    const gx = tx * tileWidth + c;
                    const v = tileElev[r * tileWidth + c];
                    const gi = gy * width + gx;
                    if (v === undefined || Number.isNaN(v) ||
                        v <= NODATA + 1 || v < -1e30 || v > 1e30) {
                        elev[gi] = NaN; // provisional nodata; filled below
                    } else {
                        elev[gi] = v;
                        mask[gi] = 1;
                        if (v < vMin) vMin = v;
                        if (v > vMax) vMax = v;
                    }
                }
            }
        }
        updateProgress(15 + Math.round((60 * (ty + 1)) / nTilesY));
    }

    if (!isFinite(vMin) || !isFinite(vMax)) {
        throw new Error('No valid elevation samples found (is this a single-band DEM?)');
    }
    if (vMax <= vMin) {
        vMax = vMin + 1;
    }
    // Replace nodata cells with the valid minimum to keep the math finite.
    for (let i = 0; i < elev.length; i++) {
        if (Number.isNaN(elev[i])) {
            elev[i] = vMin;
        }
    }
    return { elev, mask, min: vMin, max: vMax };
}

// ─── Network byte accounting ─────────────────────────────────────────────────

/**
 * Wrap `window.fetch` to keep a LIVE tally of bytes fetched over the network
 * for geospatial data — COG tile range requests and vector files alike.
 *
 * Only the response `Content-Length` is read (never the body), so this never
 * interferes with the WASM reader's own consumption of the response. App assets
 * (the WASM module, vendored libraries, CSS/JS) are excluded so the badge
 * reflects data transfer, not page weight. Local drag-dropped files never go
 * through fetch, so they are correctly never counted.
 */
function installFetchAccounting() {
    if (typeof window === 'undefined' || window.__oxigdalFetchWrapped) {
        return;
    }
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (input, init) => {
        const response = await originalFetch(input, init);
        try {
            // `input` may be a string, a Request (has `.url`), or a URL object
            // (has `.href`; wasm-bindgen fetches the module via `new URL(...)`).
            let url = '';
            if (typeof input === 'string') {
                url = input;
            } else if (input) {
                url = input.url || input.href || String(input);
            }
            if (!isAppAssetUrl(url)) {
                const len = response.headers.get('content-length');
                if (len) {
                    app.performance.bytesFetched += parseInt(len, 10) || 0;
                    updateNetworkBadges();
                }
            }
        } catch (_e) {
            // Header inspection must never break the actual fetch.
        }
        return response;
    };
    window.__oxigdalFetchWrapped = true;
}

/** True for the demo's own code/assets (excluded from the data-fetched tally). */
function isAppAssetUrl(url) {
    if (!url) {
        return false;
    }
    return /\/(pkg|vendor)\//.test(url) ||
        url.endsWith('.wasm') ||
        url.endsWith('.js') ||
        url.endsWith('.css');
}

/** Refresh the two honest network counters in the badge row. */
function updateNetworkBadges() {
    const fetchBadge = document.getElementById('fetch-badge');
    if (fetchBadge) {
        const kb = app.performance.bytesFetched / 1024;
        fetchBadge.textContent = `⬇ ${kb.toFixed(1)} KB fetched`;
    }
    const uploadBadge = document.getElementById('upload-badge');
    if (uploadBadge) {
        uploadBadge.textContent = `⬆ ${app.performance.bytesUploaded} bytes uploaded`;
    }
}

/**
 * Convert Leaflet tile coordinates to GeoTIFF tile coordinates
 */
function leafletToGeotiffTileCoords(leafletCoords) {
    const z = leafletCoords.z;
    const x = leafletCoords.x;
    const y = leafletCoords.y;

    // Calculate tile bounds in Web Mercator
    const tileSize = 256;
    const n = Math.pow(2, z);
    const lon_min = (x / n) * 360 - 180;
    const lon_max = ((x + 1) / n) * 360 - 180;
    const lat_max = Math.atan(Math.sinh(Math.PI * (1 - 2 * y / n))) * 180 / Math.PI;
    const lat_min = Math.atan(Math.sinh(Math.PI * (1 - 2 * (y + 1) / n))) * 180 / Math.PI;

    // Get COG bounds
    const cogBounds = app.currentCog.bounds;
    const cogMinLat = cogBounds[0][0];
    const cogMinLon = cogBounds[0][1];
    const cogMaxLat = cogBounds[1][0];
    const cogMaxLon = cogBounds[1][1];

    // Check if tile overlaps with COG bounds
    if (lon_max < cogMinLon || lon_min > cogMaxLon ||
        lat_max < cogMinLat || lat_min > cogMaxLat) {
        return null; // Tile is outside COG bounds
    }

    // Use center of Leaflet tile for coordinate transformation
    const centerLon = (lon_min + lon_max) / 2;
    const centerLat = (lat_min + lat_max) / 2;

    // Get geotransform parameters
    const pixelScaleX = app.viewer.pixel_scale_x();
    const pixelScaleY = app.viewer.pixel_scale_y();
    const tiepointGeoX = app.viewer.tiepoint_geo_x();
    const tiepointGeoY = app.viewer.tiepoint_geo_y();

    if (pixelScaleX === undefined || pixelScaleY === undefined ||
        tiepointGeoX === undefined || tiepointGeoY === undefined) {
        console.warn('Missing geotransform data');
        return null;
    }

    console.log(`Transforming Leaflet tile ${z}/${x}/${y} to GeoTIFF coords`);

    // Convert geographic coordinates to GeoTIFF pixel coordinates
    const pixelX = Math.floor((centerLon - tiepointGeoX) / pixelScaleX);
    const pixelY = Math.floor((centerLat - tiepointGeoY) / pixelScaleY);

    // Convert pixel coordinates to GeoTIFF tile coordinates
    const tileX = Math.floor(pixelX / Number(app.currentCog.tileWidth));
    const tileY = Math.floor(pixelY / Number(app.currentCog.tileHeight));

    // Check if tile indices are valid
    const maxTileX = Math.ceil(Number(app.currentCog.width) / Number(app.currentCog.tileWidth)) - 1;
    const maxTileY = Math.ceil(Number(app.currentCog.height) / Number(app.currentCog.tileHeight)) - 1;

    if (tileX < 0 || tileY < 0 || tileX > maxTileX || tileY > maxTileY) {
        console.log(`Tile (${tileX}, ${tileY}) out of range (max: ${maxTileX}, ${maxTileY})`);
        return null; // Tile is outside valid range
    }

    console.log(`Mapped to GeoTIFF tile (${tileX}, ${tileY})`);

    return {
        level: 0, // Use base resolution for now
        x: tileX,
        y: tileY
    };
}

/**
 * Load tile data from WASM viewer
 */
async function loadTileData(z, x, y) {
    const cacheKey = `${z}-${x}-${y}`;

    // Check cache
    if (app.tileCache.has(cacheKey)) {
        app.performance.tilesCached++;
        updateTileInfo();
        return app.tileCache.get(cacheKey);
    }

    try {
        // Use the provided tile coordinates
        console.log(`Loading tile at level ${z}, tile (${x}, ${y})`);

        const imageData = await app.viewer.read_tile_as_image_data(z, x, y);

        // Cache the tile
        app.tileCache.set(cacheKey, imageData);

        // Update stats
        app.performance.tilesLoaded++;
        const tileSize = imageData.data.length;
        app.performance.dataTransferred += tileSize;

        updateTileInfo();
        updatePerformanceDisplay();

        return imageData;
    } catch (error) {
        console.error(`Error loading tile ${cacheKey}:`, error);
        return null;
    }
}

/**
 * Apply visualization settings to tile canvas
 */
function applyVisualization(ctx, width, height) {
    const imageData = ctx.getImageData(0, 0, width, height);
    const data = imageData.data;

    const brightness = app.visualization.brightness;
    const contrast = app.visualization.contrast / 100;

    for (let i = 0; i < data.length; i += 4) {
        // Apply contrast
        data[i] = ((data[i] - 128) * contrast + 128);
        data[i + 1] = ((data[i + 1] - 128) * contrast + 128);
        data[i + 2] = ((data[i + 2] - 128) * contrast + 128);

        // Apply brightness
        data[i] = Math.max(0, Math.min(255, data[i] + brightness));
        data[i + 1] = Math.max(0, Math.min(255, data[i + 1] + brightness));
        data[i + 2] = Math.max(0, Math.min(255, data[i + 2] + brightness));
    }

    ctx.putImageData(imageData, 0, 0);
}

/**
 * Refresh visualization with current settings
 */
function refreshVisualization() {
    if (app.cogLayer && app.viewer) {
        createCogTileLayer();
    }
}

/**
 * Update layer opacity
 */
function updateLayerOpacity() {
    if (app.cogLayer) {
        app.cogLayer.setOpacity(app.visualization.opacity);
    }
}

/**
 * Reset visualization to defaults
 */
function resetVisualization() {
    app.visualization = {
        bandMode: 'rgb',
        customBands: { r: 1, g: 2, b: 3 },
        opacity: 1.0,
        brightness: 0,
        contrast: 100,
    };

    document.getElementById('band-mode').value = 'rgb';
    document.getElementById('opacity-slider').value = 100;
    document.getElementById('opacity-value').textContent = '100%';
    document.getElementById('brightness-slider').value = 0;
    document.getElementById('brightness-value').textContent = '0';
    document.getElementById('contrast-slider').value = 100;
    document.getElementById('contrast-value').textContent = '100%';

    refreshVisualization();
}

/**
 * Fit map to COG bounds
 */
function fitToBounds() {
    if (app.currentCog.bounds && app.map) {
        const url = app.currentCog.url || '';

        // For Golden Triangle and Iron Belt, use zoom level 10
        if (url.includes('golden-triangle') || url.includes('iron-belt')) {
            const bounds = app.currentCog.bounds;
            const centerLat = (bounds[0][0] + bounds[1][0]) / 2;
            const centerLon = (bounds[0][1] + bounds[1][1]) / 2;
            app.map.setView([centerLat, centerLon], 10);
        } else {
            app.map.fitBounds(app.currentCog.bounds);
        }
    }
}

/**
 * Toggle grid overlay
 */
function toggleGrid() {
    // Placeholder for grid overlay functionality
    console.log('Toggle grid overlay');
}

/**
 * Display metadata panel
 */
function displayMetadata() {
    const panel = document.getElementById('metadata-panel');
    const metadata = app.currentCog.metadata;

    const items = [
        { label: 'Dimensions', value: `${metadata.width} × ${metadata.height} px` },
        { label: 'Tile Size', value: `${metadata.tileWidth} × ${metadata.tileHeight} px` },
        { label: 'Bands', value: metadata.bandCount },
        { label: 'Overviews', value: metadata.overviewCount },
    ];

    if (metadata.epsgCode) {
        items.push({ label: 'CRS (EPSG)', value: metadata.epsgCode });
    }

    items.push({ label: 'URL', value: metadata.url, class: 'url-value' });

    panel.innerHTML = items.map(item => `
        <div class="metadata-item">
            <div class="metadata-label">${item.label}</div>
            <div class="metadata-value ${item.class || ''}">${item.value}</div>
        </div>
    `).join('');
}

/**
 * Update tile information display
 */
function updateTileInfo() {
    document.getElementById('tiles-loaded').textContent = app.performance.tilesLoaded;
    document.getElementById('tiles-cached').textContent = app.performance.tilesCached;
}

/**
 * Update map information display
 */
function updateMapInfo() {
    if (!app.map) return;

    const zoom = app.map.getZoom();
    const center = app.map.getCenter();

    document.getElementById('current-zoom').textContent = zoom.toFixed(1);
    document.getElementById('view-center').textContent =
        `${center.lat.toFixed(4)}, ${center.lng.toFixed(4)}`;
}

/**
 * Update performance display
 */
function updatePerformanceDisplay() {
    const loadTime = app.performance.loadEndTime - app.performance.loadStartTime;
    document.getElementById('load-time').textContent = `${loadTime.toFixed(0)} ms`;

    const dataTransferMB = (app.performance.dataTransferred / (1024 * 1024)).toFixed(2);
    document.getElementById('data-transfer').textContent = `${dataTransferMB} MB`;
}

/**
 * Update application status
 */
function updateStatus(status, text) {
    const dot = document.getElementById('status-dot');
    const textElement = document.getElementById('status-text');

    dot.className = 'status-dot';
    dot.classList.add(`status-${status}`);
    textElement.textContent = text;
}

/**
 * Show loading overlay
 */
function showLoading(message) {
    const overlay = document.getElementById('loading-overlay');
    const messageElement = document.getElementById('loading-message');
    overlay.style.display = 'flex';
    messageElement.textContent = message;
}

/**
 * Hide loading overlay
 */
function hideLoading() {
    const overlay = document.getElementById('loading-overlay');
    overlay.style.display = 'none';
}

/**
 * Update loading progress
 */
function updateProgress(percent) {
    const bar = document.getElementById('progress-bar');
    bar.style.width = `${percent}%`;
}

/**
 * Show error overlay
 */
function showError(message) {
    const overlay = document.getElementById('error-overlay');
    const messageElement = document.getElementById('error-message');
    overlay.style.display = 'flex';
    messageElement.textContent = message;
}

/**
 * Hide error overlay
 */
function hideError() {
    const overlay = document.getElementById('error-overlay');
    overlay.style.display = 'none';
}

/**
 * Start measurement tool
 */
function startMeasurement(type) {
    // Clear any previous measurement
    clearMeasurements();

    app.measurements.active = true;
    app.measurements.type = type;
    app.measurements.coordinates = [];

    // Change cursor style
    document.getElementById('map-container').style.cursor = 'crosshair';

    updateStatus('ready', `Click on map to measure ${type}`);
    console.log(`Started ${type} measurement`);
}

/**
 * Handle map click for measurements
 */
function handleMapClick(e) {
    if (!app.measurements.active) return;

    app.measurements.coordinates.push(e.latlng);

    // Add marker
    const marker = L.circleMarker(e.latlng, {
        radius: 5,
        color: '#2563eb',
        fillColor: '#3b82f6',
        fillOpacity: 0.8,
    }).addTo(app.map);
    app.measurements.markers.push(marker);

    if (app.measurements.type === 'distance') {
        updateDistanceMeasurement();
    } else if (app.measurements.type === 'area') {
        updateAreaMeasurement();
    }
}

/**
 * Update distance measurement display
 */
function updateDistanceMeasurement() {
    const coords = app.measurements.coordinates;

    if (coords.length < 2) return;

    // Remove existing polyline if any
    if (app.measurements.polyline) {
        app.map.removeLayer(app.measurements.polyline);
    }

    // Create polyline
    app.measurements.polyline = L.polyline(coords, {
        color: '#2563eb',
        weight: 3,
        opacity: 0.8,
    }).addTo(app.map);

    // Calculate total distance
    let totalDistance = 0;
    for (let i = 0; i < coords.length - 1; i++) {
        totalDistance += coords[i].distanceTo(coords[i + 1]);
    }

    // Format distance
    const distanceKm = (totalDistance / 1000).toFixed(2);
    const distanceMi = (totalDistance / 1609.34).toFixed(2);

    // Display result
    const lastCoord = coords[coords.length - 1];
    if (app.measurements.label) {
        app.map.removeLayer(app.measurements.label);
    }

    app.measurements.label = L.popup({
        closeButton: false,
        className: 'measurement-popup',
    })
        .setLatLng(lastCoord)
        .setContent(`<strong>Distance:</strong><br>${distanceKm} km<br>${distanceMi} mi`)
        .openOn(app.map);

    updateStatus('ready', `Distance: ${distanceKm} km (${distanceMi} mi)`);
}

/**
 * Update area measurement display
 */
function updateAreaMeasurement() {
    const coords = app.measurements.coordinates;

    if (coords.length < 3) {
        // Show temporary polyline
        if (coords.length === 2) {
            if (app.measurements.polyline) {
                app.map.removeLayer(app.measurements.polyline);
            }
            app.measurements.polyline = L.polyline(coords, {
                color: '#2563eb',
                weight: 3,
                opacity: 0.8,
                dashArray: '5, 5',
            }).addTo(app.map);
        }
        return;
    }

    // Remove polyline
    if (app.measurements.polyline) {
        app.map.removeLayer(app.measurements.polyline);
        app.measurements.polyline = null;
    }

    // Remove existing polygon if any
    if (app.measurements.polygon) {
        app.map.removeLayer(app.measurements.polygon);
    }

    // Create polygon
    app.measurements.polygon = L.polygon(coords, {
        color: '#2563eb',
        fillColor: '#3b82f6',
        fillOpacity: 0.3,
        weight: 3,
    }).addTo(app.map);

    // Calculate area using Leaflet's built-in method
    const bounds = app.measurements.polygon.getBounds();
    const area = L.GeometryUtil ?
        L.GeometryUtil.geodesicArea(coords.map(c => [c.lat, c.lng])) :
        calculatePolygonArea(coords);

    // Format area
    const areaSqKm = (area / 1000000).toFixed(2);
    const areaSqMi = (area / 2589988.11).toFixed(2);
    const areaHectares = (area / 10000).toFixed(2);

    // Display result
    const center = app.measurements.polygon.getBounds().getCenter();
    if (app.measurements.label) {
        app.map.removeLayer(app.measurements.label);
    }

    app.measurements.label = L.popup({
        closeButton: false,
        className: 'measurement-popup',
    })
        .setLatLng(center)
        .setContent(`<strong>Area:</strong><br>${areaSqKm} km²<br>${areaSqMi} mi²<br>${areaHectares} ha`)
        .openOn(app.map);

    updateStatus('ready', `Area: ${areaSqKm} km² (${areaHectares} ha)`);
}

/**
 * Calculate polygon area using spherical geometry
 */
function calculatePolygonArea(latlngs) {
    if (latlngs.length < 3) return 0;

    const earthRadius = 6371000; // meters
    let area = 0;

    for (let i = 0; i < latlngs.length; i++) {
        const j = (i + 1) % latlngs.length;
        const lat1 = latlngs[i].lat * Math.PI / 180;
        const lat2 = latlngs[j].lat * Math.PI / 180;
        const lng1 = latlngs[i].lng * Math.PI / 180;
        const lng2 = latlngs[j].lng * Math.PI / 180;

        area += (lng2 - lng1) * (2 + Math.sin(lat1) + Math.sin(lat2));
    }

    area = Math.abs(area * earthRadius * earthRadius / 2);
    return area;
}

/**
 * Clear measurements
 */
function clearMeasurements() {
    app.measurements.active = false;
    app.measurements.type = null;
    app.measurements.coordinates = [];

    // Remove all markers
    app.measurements.markers.forEach(marker => app.map.removeLayer(marker));
    app.measurements.markers = [];

    // Remove polyline
    if (app.measurements.polyline) {
        app.map.removeLayer(app.measurements.polyline);
        app.measurements.polyline = null;
    }

    // Remove polygon
    if (app.measurements.polygon) {
        app.map.removeLayer(app.measurements.polygon);
        app.measurements.polygon = null;
    }

    // Remove label
    if (app.measurements.label) {
        app.map.removeLayer(app.measurements.label);
        app.measurements.label = null;
    }

    // Reset cursor
    document.getElementById('map-container').style.cursor = '';

    updateStatus('ready', 'Ready');
    console.log('Measurements cleared');
}

/**
 * Load vector data from URL
 */
async function loadVector(url, name = null) {
    try {
        app.performance.loadStartTime = performance.now();

        updateStatus('loading', `Loading ${name || 'vector data'}...`);
        showLoading(`Loading vector: ${name || url}`);
        updateProgress(10);

        const format = detectVectorFormat(url);
        if (!format) {
            throw new Error('Unsupported vector format');
        }

        updateProgress(30);

        let geojson = null;

        switch (format) {
            case 'geojson':
                geojson = await loadGeoJSON(url);
                break;
            case 'flatgeobuf':
                geojson = await loadFlatGeobuf(url);
                break;
            case 'shapefile':
                geojson = await loadShapefile(url);
                break;
            case 'geoparquet':
                geojson = await loadGeoParquet(url);
                break;
            case 'gpx':
                geojson = await loadGPX(url);
                break;
            case 'kml':
                geojson = await loadKML(url);
                break;
            case 'topojson':
                geojson = await loadTopoJSON(url);
                break;
            default:
                throw new Error(`Format ${format} not implemented yet`);
        }

        updateProgress(70);

        // Store vector info
        app.currentVector.url = url;
        app.currentVector.format = format;
        app.currentVector.features = geojson.features || [];

        // Create Leaflet layer
        createVectorLayer(geojson, name || url, format);

        updateProgress(90);

        // Display vector metadata
        displayVectorMetadata(geojson, format);

        // Fit to vector bounds
        fitToVectorBounds();

        updateProgress(100);

        app.performance.loadEndTime = performance.now();
        updatePerformanceDisplay();

        updateStatus('ready', 'Vector data loaded successfully');
        hideLoading();

        console.log(`Successfully loaded vector: ${url}`);
    } catch (error) {
        console.error('Failed to load vector:', error);
        showError(`Failed to load vector data: ${error.message || error}`);
        updateStatus('error', 'Failed to load vector');
        hideLoading();
    }
}

/**
 * Create Leaflet vector layer from GeoJSON
 */
function createVectorLayer(geojson, name, format) {
    // Remove existing vector layer with same name
    if (app.vectorLayers.has(name)) {
        const oldLayer = app.vectorLayers.get(name);
        app.map.removeLayer(oldLayer);
    }

    // Create styled GeoJSON layer
    const layer = L.geoJSON(geojson, {
        style: (feature) => getVectorStyle(feature, format),
        pointToLayer: (feature, latlng) => {
            return L.circleMarker(latlng, {
                radius: 6,
                fillColor: getFeatureColor(feature, format),
                color: '#fff',
                weight: 2,
                opacity: 1,
                fillOpacity: 0.8
            });
        },
        onEachFeature: (feature, layer) => {
            // Add popup with feature properties
            if (feature.properties) {
                const popupContent = createFeaturePopup(feature);
                layer.bindPopup(popupContent);
            }
        }
    });

    layer.addTo(app.map);
    app.vectorLayers.set(name, layer);

    // Calculate bounds
    const bounds = layer.getBounds();
    app.currentVector.bounds = bounds;

    // Update layer controls UI
    updateLayerControls();
}

/**
 * Get style for vector feature based on format
 */
function getVectorStyle(feature, format) {
    // Use feature properties for styling if available
    if (feature.properties) {
        const props = feature.properties;

        return {
            color: props['stroke'] || props['marker-color'] || '#3388ff',
            weight: props['stroke-width'] || 2,
            opacity: props['stroke-opacity'] || 1,
            fillColor: props['fill'] || props['marker-color'] || '#3388ff',
            fillOpacity: props['fill-opacity'] || 0.2
        };
    }

    // Default styles by format
    const defaultStyles = {
        geojson: {
            color: '#3388ff',
            weight: 2,
            opacity: 1,
            fillColor: '#3388ff',
            fillOpacity: 0.2
        },
        flatgeobuf: {
            color: '#ff7800',
            weight: 2,
            opacity: 1,
            fillColor: '#ff7800',
            fillOpacity: 0.2
        },
        shapefile: {
            color: '#00ff00',
            weight: 2,
            opacity: 1,
            fillColor: '#00ff00',
            fillOpacity: 0.2
        },
        geoparquet: {
            color: '#ff00ff',
            weight: 2,
            opacity: 1,
            fillColor: '#ff00ff',
            fillOpacity: 0.2
        }
    };

    return defaultStyles[format] || defaultStyles.geojson;
}

/**
 * Get color for feature based on properties
 */
function getFeatureColor(feature, format) {
    if (feature.properties) {
        return feature.properties['marker-color'] ||
               feature.properties['fill'] ||
               '#3388ff';
    }
    return '#3388ff';
}

/**
 * Create popup content for feature
 */
function createFeaturePopup(feature) {
    const props = feature.properties || {};

    let html = '<div class="feature-popup">';

    // Add name/title if available
    const name = props.name || props.title || props.NAME || props.TITLE;
    if (name) {
        html += `<h4 class="feature-name">${escapeHtml(name)}</h4>`;
    }

    // Add description if available
    const desc = props.description || props.desc || props.DESCRIPTION;
    if (desc) {
        html += `<p class="feature-description">${escapeHtml(desc)}</p>`;
    }

    // Add properties table
    html += '<table class="feature-properties">';
    for (const [key, value] of Object.entries(props)) {
        // Skip already displayed properties
        if (['name', 'title', 'NAME', 'TITLE', 'description', 'desc', 'DESCRIPTION'].includes(key)) {
            continue;
        }

        // Skip styling properties
        if (key.startsWith('marker-') || key.startsWith('stroke') || key.startsWith('fill')) {
            continue;
        }

        html += `<tr><td class="prop-key">${escapeHtml(key)}:</td><td class="prop-value">${escapeHtml(String(value))}</td></tr>`;
    }
    html += '</table>';
    html += '</div>';

    return html;
}

/**
 * Escape HTML to prevent XSS
 */
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

/**
 * Display vector metadata in panel
 */
function displayVectorMetadata(geojson, format) {
    const panel = document.getElementById('metadata-panel');
    const features = geojson.features || [];

    // Count feature types
    const typeCounts = {};
    features.forEach(feature => {
        const type = feature.geometry?.type || 'Unknown';
        typeCounts[type] = (typeCounts[type] || 0) + 1;
    });

    const items = [
        { label: 'Format', value: format.toUpperCase() },
        { label: 'Features', value: features.length },
        { label: 'Feature Types', value: Object.keys(typeCounts).join(', ') },
    ];

    // Add type counts
    for (const [type, count] of Object.entries(typeCounts)) {
        items.push({ label: `  ${type}`, value: count });
    }

    items.push({ label: 'URL', value: app.currentVector.url, class: 'url-value' });

    panel.innerHTML = items.map(item => `
        <div class="metadata-item">
            <div class="metadata-label">${item.label}</div>
            <div class="metadata-value ${item.class || ''}">${item.value}</div>
        </div>
    `).join('');
}

/**
 * Fit map to vector bounds
 */
function fitToVectorBounds() {
    if (app.currentVector.bounds && app.map) {
        const url = app.currentVector.url || '';

        // For Golden Triangle and Iron Belt, use zoom level 10
        if (url.includes('golden-triangle') || url.includes('iron-belt')) {
            // app.currentVector.bounds is a Leaflet LatLngBounds object
            const center = app.currentVector.bounds.getCenter();
            app.map.setView(center, 10);
        } else {
            app.map.fitBounds(app.currentVector.bounds);
        }
    }
}

/**
 * Toggle vector layer visibility
 */
function toggleVectorLayer(name, visible) {
    const layer = app.vectorLayers.get(name);
    if (layer) {
        if (visible) {
            app.map.addLayer(layer);
        } else {
            app.map.removeLayer(layer);
        }
    }
}

/**
 * Remove all vector layers
 */
function clearVectorLayers() {
    app.vectorLayers.forEach((layer, name) => {
        app.map.removeLayer(layer);
    });
    app.vectorLayers.clear();
    app.currentVector = {
        url: null,
        format: null,
        features: [],
        bounds: null,
    };
}

/**
 * Update layer controls UI
 */
function updateLayerControls() {
    const section = document.getElementById('layer-controls-section');
    const container = document.getElementById('layer-controls');

    if (!section || !container) {
        return;
    }

    // Show/hide section based on layer count
    if (app.vectorLayers.size === 0) {
        section.style.display = 'none';
        return;
    }

    section.style.display = 'block';

    // Generate layer toggles
    let html = '';
    app.vectorLayers.forEach((layer, name) => {
        const isVisible = app.map.hasLayer(layer);
        html += `
            <div class="layer-toggle">
                <span class="layer-name">${escapeHtml(name)}</span>
                <label class="layer-switch">
                    <input type="checkbox"
                           ${isVisible ? 'checked' : ''}
                           onchange="toggleVectorLayer('${escapeHtml(name)}', this.checked)">
                    <span class="layer-switch-slider"></span>
                </label>
            </div>
        `;
    });

    container.innerHTML = html;
}

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeApp);
} else {
    initializeApp();
}

// Expose functions to global scope for HTML event handlers
window.toggleVectorLayer = toggleVectorLayer;
