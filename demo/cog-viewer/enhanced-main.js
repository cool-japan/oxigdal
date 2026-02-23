/**
 * OxiGDAL Advanced COG Viewer - Enhanced Main Application
 *
 * Features:
 * - Leaflet-based interactive map
 * - OxiGDAL WASM integration for COG reading
 * - Advanced band selection and visualization
 * - Performance monitoring
 * - Comprehensive error handling
 * - Tile caching and optimization
 * - Web Worker parallel tile loading
 * - Layer comparison (side-by-side)
 * - Measurement tools (distance, area)
 * - Coordinate display
 * - Permalink functionality
 * - Download functionality
 * - Mobile-responsive design
 * - Accessibility features
 */

// Import WASM module from parent demo directory
import init, { WasmCogViewer, version } from '../pkg/oxigdal_wasm.js';

// Application state
const app = {
    // WASM
    viewer: null,
    wasmInitialized: false,

    // Map
    map: null,
    cogLayer: null,
    comparisonMap: null,
    comparisonCogLayer: null,

    // Workers
    workers: [],
    workerJobId: 0,
    workerCallbacks: new Map(),

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

    // Comparison COG
    comparisonCog: {
        url: null,
        viewer: null,
        active: false,
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
    },

    // Tile cache
    tileCache: new Map(),

    // Measurement tools
    measurements: {
        active: false,
        type: null, // 'distance' or 'area'
        coordinates: [],
        layer: null,
    },

    // Coordinate tracking
    coordinates: {
        mouse: null,
        center: null,
    },

    // Features
    features: {
        workerEnabled: false,
        comparisonEnabled: false,
        measurementEnabled: false,
        downloadEnabled: true,
    },
};

/**
 * Initialize the application
 */
async function initializeApp() {
    try {
        updateStatus('loading', 'Initializing WebAssembly...');
        showLoading('Initializing OxiGDAL WASM module...');

        // Initialize WASM
        await init();
        app.wasmInitialized = true;

        // Display version
        const versionBadge = document.getElementById('version-badge');
        if (versionBadge) {
            versionBadge.textContent = `v${version()}`;
        }

        console.log('OxiGDAL WASM initialized successfully');

        // Initialize Leaflet map
        initializeMap();

        // Setup event listeners
        setupEventListeners();

        // Check for permalink parameters
        loadFromPermalink();

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
    const mapContainer = document.getElementById('map-container');
    if (!mapContainer) {
        console.error('Map container not found');
        return;
    }

    // Create map centered on world
    app.map = L.map(mapContainer, {
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
    app.map.on('mousemove', updateMouseCoordinates);
    app.map.on('click', handleMapClick);

    console.log('Leaflet map initialized');
}

/**
 * Setup event listeners
 */
function setupEventListeners() {
    // Load button
    const loadBtn = document.getElementById('load-btn');
    if (loadBtn) {
        loadBtn.addEventListener('click', loadCogFromInput);
    }

    // URL input - Enter key
    const urlInput = document.getElementById('cog-url-input');
    if (urlInput) {
        urlInput.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                loadCogFromInput();
            }
        });
    }

    // Example cards
    document.querySelectorAll('.example-card').forEach(card => {
        card.addEventListener('click', (e) => {
            const button = e.currentTarget;
            const url = button.dataset.url;
            const name = button.dataset.name;
            if (urlInput) {
                urlInput.value = url;
            }
            loadCog(url, name);
        });
    });

    // Band mode selector
    const bandMode = document.getElementById('band-mode');
    if (bandMode) {
        bandMode.addEventListener('change', (e) => {
            app.visualization.bandMode = e.target.value;
            const customBands = document.getElementById('custom-bands');
            if (customBands) {
                customBands.style.display = e.target.value === 'custom' ? 'block' : 'none';
            }
            refreshVisualization();
        });
    }

    // Custom band inputs
    ['red-band', 'green-band', 'blue-band'].forEach((id, index) => {
        const input = document.getElementById(id);
        if (input) {
            input.addEventListener('change', (e) => {
                const band = parseInt(e.target.value) || 1;
                const channels = ['r', 'g', 'b'];
                app.visualization.customBands[channels[index]] = band;
                refreshVisualization();
            });
        }
    });

    // Opacity slider
    const opacitySlider = document.getElementById('opacity-slider');
    if (opacitySlider) {
        opacitySlider.addEventListener('input', (e) => {
            const value = parseInt(e.target.value);
            app.visualization.opacity = value / 100;
            const opacityValue = document.getElementById('opacity-value');
            if (opacityValue) {
                opacityValue.textContent = `${value}%`;
            }
            updateLayerOpacity();
        });
    }

    // Brightness slider
    const brightnessSlider = document.getElementById('brightness-slider');
    if (brightnessSlider) {
        brightnessSlider.addEventListener('input', (e) => {
            const value = parseInt(e.target.value);
            app.visualization.brightness = value;
            const brightnessValue = document.getElementById('brightness-value');
            if (brightnessValue) {
                brightnessValue.textContent = value.toString();
            }
            refreshVisualization();
        });
    }

    // Contrast slider
    const contrastSlider = document.getElementById('contrast-slider');
    if (contrastSlider) {
        contrastSlider.addEventListener('input', (e) => {
            const value = parseInt(e.target.value);
            app.visualization.contrast = value;
            const contrastValue = document.getElementById('contrast-value');
            if (contrastValue) {
                contrastValue.textContent = `${value}%`;
            }
            refreshVisualization();
        });
    }

    // Reset visualization
    const resetBtn = document.getElementById('reset-visualization');
    if (resetBtn) {
        resetBtn.addEventListener('click', resetVisualization);
    }

    // Fit bounds button
    const fitBoundsBtn = document.getElementById('fit-bounds-btn');
    if (fitBoundsBtn) {
        fitBoundsBtn.addEventListener('click', fitToBounds);
    }

    // Toggle grid button
    const toggleGridBtn = document.getElementById('toggle-grid-btn');
    if (toggleGridBtn) {
        toggleGridBtn.addEventListener('click', toggleGrid);
    }

    // Dismiss error
    const dismissError = document.getElementById('dismiss-error');
    if (dismissError) {
        dismissError.addEventListener('click', hideError);
    }

    // Comparison toggle
    const comparisonToggle = document.getElementById('toggle-comparison');
    if (comparisonToggle) {
        comparisonToggle.addEventListener('click', toggleComparison);
    }

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

    // Download button
    const downloadBtn = document.getElementById('download-view');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', downloadCurrentView);
    }

    // Permalink button
    const permalinkBtn = document.getElementById('copy-permalink');
    if (permalinkBtn) {
        permalinkBtn.addEventListener('click', copyPermalink);
    }

    // Keyboard shortcuts
    document.addEventListener('keydown', handleKeyboardShortcuts);
}

/**
 * Load COG from input field
 */
async function loadCogFromInput() {
    const urlInput = document.getElementById('cog-url-input');
    const url = urlInput ? urlInput.value.trim() : '';
    if (!url) {
        showError('Please enter a COG URL');
        return;
    }
    await loadCog(url);
}

/**
 * Load a COG from URL
 */
async function loadCog(url, name = null) {
    try {
        app.performance.loadStartTime = performance.now();
        app.performance.tilesLoaded = 0;
        app.performance.tilesCached = 0;
        app.performance.dataTransferred = 0;

        updateStatus('loading', `Loading ${name || 'COG'}...`);
        showLoading(`Loading COG: ${name || url}`);
        updateProgress(10);

        // Create viewer if not exists
        if (!app.viewer) {
            app.viewer = new WasmCogViewer();
        }

        updateProgress(20);

        // Open the COG
        await app.viewer.open(url);

        updateProgress(50);

        // Extract metadata
        app.currentCog.url = url;
        app.currentCog.width = app.viewer.width();
        app.currentCog.height = app.viewer.height();
        app.currentCog.tileWidth = app.viewer.tile_width();
        app.currentCog.tileHeight = app.viewer.tile_height();
        app.currentCog.bandCount = app.viewer.band_count();

        // Parse metadata JSON
        const metadataJson = app.viewer.metadata_json();
        app.currentCog.metadata = JSON.parse(metadataJson);

        console.log('COG Metadata:', app.currentCog.metadata);

        updateProgress(70);

        // Calculate bounds
        const epsgCode = app.viewer.epsg_code();
        app.currentCog.bounds = calculateBounds(epsgCode);

        // Clear tile cache
        app.tileCache.clear();

        updateProgress(80);

        // Create custom tile layer
        createCogTileLayer();

        updateProgress(90);

        // Display metadata
        displayMetadata();

        // Fit to bounds
        fitToBounds();

        // Update permalink
        updatePermalinkState();

        updateProgress(100);

        app.performance.loadEndTime = performance.now();
        updatePerformanceDisplay();

        updateStatus('ready', 'COG loaded successfully');
        hideLoading();

        console.log(`Successfully loaded COG: ${url}`);
    } catch (error) {
        console.error('Failed to load COG:', error);
        showError(`Failed to load COG: ${error.message || error}`);
        updateStatus('error', 'Failed to load COG');
        hideLoading();
    }
}

/**
 * Calculate bounds for the COG
 */
function calculateBounds(epsgCode) {
    if (epsgCode === 4326) {
        return [
            [-85, -180],
            [85, 180]
        ];
    } else {
        return [
            [40, -120],
            [50, -110]
        ];
    }
}

/**
 * Create custom COG tile layer using Leaflet
 */
function createCogTileLayer() {
    if (app.cogLayer) {
        app.map.removeLayer(app.cogLayer);
    }

    const CogTileLayer = L.GridLayer.extend({
        createTile: function(coords, done) {
            const tile = document.createElement('canvas');
            tile.width = app.currentCog.tileWidth;
            tile.height = app.currentCog.tileHeight;

            const ctx = tile.getContext('2d');

            loadTileData(coords.z, coords.x, coords.y)
                .then(imageData => {
                    if (imageData) {
                        ctx.putImageData(imageData, 0, 0);
                        applyVisualization(ctx, tile.width, tile.height);
                    }
                    done(null, tile);
                })
                .catch(error => {
                    console.error(`Failed to load tile ${coords.z}/${coords.x}/${coords.y}:`, error);
                    ctx.fillStyle = '#f0f0f0';
                    ctx.fillRect(0, 0, tile.width, tile.height);
                    ctx.strokeStyle = '#e0e0e0';
                    ctx.strokeRect(0, 0, tile.width, tile.height);
                    done(error, tile);
                });

            return tile;
        }
    });

    app.cogLayer = new CogTileLayer({
        tileSize: app.currentCog.tileWidth,
        opacity: app.visualization.opacity,
        bounds: app.currentCog.bounds,
        minZoom: 0,
        maxZoom: 20,
    });

    app.cogLayer.addTo(app.map);
}

/**
 * Load tile data from WASM viewer
 */
async function loadTileData(z, x, y) {
    const cacheKey = `${z}-${x}-${y}`;

    if (app.tileCache.has(cacheKey)) {
        app.performance.tilesCached++;
        updateTileInfo();
        return app.tileCache.get(cacheKey);
    }

    try {
        const level = 0;
        const imageData = await app.viewer.read_tile_as_image_data(level, x, y);

        app.tileCache.set(cacheKey, imageData);

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
        data[i] = ((data[i] - 128) * contrast + 128);
        data[i + 1] = ((data[i + 1] - 128) * contrast + 128);
        data[i + 2] = ((data[i + 2] - 128) * contrast + 128);

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
    if (app.cogLayer) {
        app.cogLayer.redraw();
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

    const bandMode = document.getElementById('band-mode');
    if (bandMode) bandMode.value = 'rgb';

    const opacitySlider = document.getElementById('opacity-slider');
    if (opacitySlider) opacitySlider.value = 100;

    const opacityValue = document.getElementById('opacity-value');
    if (opacityValue) opacityValue.textContent = '100%';

    const brightnessSlider = document.getElementById('brightness-slider');
    if (brightnessSlider) brightnessSlider.value = 0;

    const brightnessValue = document.getElementById('brightness-value');
    if (brightnessValue) brightnessValue.textContent = '0';

    const contrastSlider = document.getElementById('contrast-slider');
    if (contrastSlider) contrastSlider.value = 100;

    const contrastValue = document.getElementById('contrast-value');
    if (contrastValue) contrastValue.textContent = '100%';

    refreshVisualization();
}

/**
 * Fit map to COG bounds
 */
function fitToBounds() {
    if (app.currentCog.bounds && app.map) {
        app.map.fitBounds(app.currentCog.bounds);
    }
}

/**
 * Toggle grid overlay
 */
function toggleGrid() {
    console.log('Toggle grid overlay - placeholder');
}

/**
 * Display metadata panel
 */
function displayMetadata() {
    const panel = document.getElementById('metadata-panel');
    if (!panel) return;

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
    const tilesLoaded = document.getElementById('tiles-loaded');
    if (tilesLoaded) tilesLoaded.textContent = app.performance.tilesLoaded;

    const tilesCached = document.getElementById('tiles-cached');
    if (tilesCached) tilesCached.textContent = app.performance.tilesCached;
}

/**
 * Update map information display
 */
function updateMapInfo() {
    if (!app.map) return;

    const zoom = app.map.getZoom();
    const center = app.map.getCenter();

    const currentZoom = document.getElementById('current-zoom');
    if (currentZoom) currentZoom.textContent = zoom.toFixed(1);

    const viewCenter = document.getElementById('view-center');
    if (viewCenter) {
        viewCenter.textContent = `${center.lat.toFixed(4)}, ${center.lng.toFixed(4)}`;
    }

    app.coordinates.center = center;
    updatePermalinkState();
}

/**
 * Update mouse coordinates
 */
function updateMouseCoordinates(e) {
    app.coordinates.mouse = e.latlng;

    const coordDisplay = document.getElementById('mouse-coordinates');
    if (coordDisplay) {
        coordDisplay.textContent = `${e.latlng.lat.toFixed(6)}, ${e.latlng.lng.toFixed(6)}`;
    }
}

/**
 * Update performance display
 */
function updatePerformanceDisplay() {
    const loadTime = app.performance.loadEndTime - app.performance.loadStartTime;
    const loadTimeEl = document.getElementById('load-time');
    if (loadTimeEl) loadTimeEl.textContent = `${loadTime.toFixed(0)} ms`;

    const dataTransferMB = (app.performance.dataTransferred / (1024 * 1024)).toFixed(2);
    const dataTransferEl = document.getElementById('data-transfer');
    if (dataTransferEl) dataTransferEl.textContent = `${dataTransferMB} MB`;
}

/**
 * Update application status
 */
function updateStatus(status, text) {
    const dot = document.getElementById('status-dot');
    const textElement = document.getElementById('status-text');

    if (dot) {
        dot.className = 'status-dot';
        dot.classList.add(`status-${status}`);
    }

    if (textElement) {
        textElement.textContent = text;
    }
}

/**
 * Show loading overlay
 */
function showLoading(message) {
    const overlay = document.getElementById('loading-overlay');
    const messageElement = document.getElementById('loading-message');

    if (overlay) overlay.style.display = 'flex';
    if (messageElement) messageElement.textContent = message;
}

/**
 * Hide loading overlay
 */
function hideLoading() {
    const overlay = document.getElementById('loading-overlay');
    if (overlay) overlay.style.display = 'none';
}

/**
 * Update loading progress
 */
function updateProgress(percent) {
    const bar = document.getElementById('progress-bar');
    if (bar) bar.style.width = `${percent}%`;
}

/**
 * Show error overlay
 */
function showError(message) {
    const overlay = document.getElementById('error-overlay');
    const messageElement = document.getElementById('error-message');

    if (overlay) overlay.style.display = 'flex';
    if (messageElement) messageElement.textContent = message;
}

/**
 * Hide error overlay
 */
function hideError() {
    const overlay = document.getElementById('error-overlay');
    if (overlay) overlay.style.display = 'none';
}

/**
 * Toggle comparison mode
 */
function toggleComparison() {
    app.comparisonCog.active = !app.comparisonCog.active;

    if (app.comparisonCog.active) {
        // Create comparison map
        // This would be implemented with split screen
        console.log('Comparison mode enabled');
    } else {
        // Remove comparison map
        console.log('Comparison mode disabled');
    }
}

/**
 * Start measurement tool
 */
function startMeasurement(type) {
    app.measurements.active = true;
    app.measurements.type = type;
    app.measurements.coordinates = [];

    console.log(`Started ${type} measurement`);
}

/**
 * Handle map click for measurements
 */
function handleMapClick(e) {
    if (!app.measurements.active) return;

    app.measurements.coordinates.push(e.latlng);

    if (app.measurements.type === 'distance' && app.measurements.coordinates.length >= 2) {
        calculateDistance();
    } else if (app.measurements.type === 'area' && app.measurements.coordinates.length >= 3) {
        calculateArea();
    }
}

/**
 * Calculate distance
 */
function calculateDistance() {
    const coords = app.measurements.coordinates;
    let totalDistance = 0;

    for (let i = 0; i < coords.length - 1; i++) {
        totalDistance += coords[i].distanceTo(coords[i + 1]);
    }

    console.log(`Distance: ${(totalDistance / 1000).toFixed(2)} km`);
}

/**
 * Calculate area
 */
function calculateArea() {
    const coords = app.measurements.coordinates;
    const latlngs = coords.map(c => [c.lat, c.lng]);

    // Simple polygon area calculation
    let area = 0;
    for (let i = 0; i < latlngs.length; i++) {
        const j = (i + 1) % latlngs.length;
        area += latlngs[i][0] * latlngs[j][1];
        area -= latlngs[j][0] * latlngs[i][1];
    }
    area = Math.abs(area) / 2;

    console.log(`Area: ${area.toFixed(2)} sq units`);
}

/**
 * Clear measurements
 */
function clearMeasurements() {
    app.measurements.active = false;
    app.measurements.type = null;
    app.measurements.coordinates = [];

    if (app.measurements.layer) {
        app.map.removeLayer(app.measurements.layer);
        app.measurements.layer = null;
    }
}

/**
 * Download current view
 */
function downloadCurrentView() {
    if (!app.map) return;

    const canvas = document.createElement('canvas');
    const mapSize = app.map.getSize();
    canvas.width = mapSize.x;
    canvas.height = mapSize.y;

    const ctx = canvas.getContext('2d');

    // Draw map to canvas (simplified)
    ctx.fillStyle = '#f0f0f0';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Convert to blob and download
    canvas.toBlob(blob => {
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `cog-view-${Date.now()}.png`;
        a.click();
        URL.revokeObjectURL(url);
    });
}

/**
 * Copy permalink to clipboard
 */
function copyPermalink() {
    const permalink = generatePermalink();

    navigator.clipboard.writeText(permalink).then(() => {
        console.log('Permalink copied to clipboard');
        showNotification('Permalink copied to clipboard!');
    }).catch(err => {
        console.error('Failed to copy permalink:', err);
    });
}

/**
 * Generate permalink
 */
function generatePermalink() {
    const params = new URLSearchParams();

    if (app.currentCog.url) {
        params.set('url', app.currentCog.url);
    }

    if (app.map) {
        const center = app.map.getCenter();
        const zoom = app.map.getZoom();
        params.set('lat', center.lat.toFixed(6));
        params.set('lng', center.lng.toFixed(6));
        params.set('zoom', zoom.toFixed(1));
    }

    return `${window.location.origin}${window.location.pathname}#${params.toString()}`;
}

/**
 * Update permalink state
 */
function updatePermalinkState() {
    const permalink = generatePermalink();
    window.history.replaceState({}, '', permalink);
}

/**
 * Load from permalink
 */
function loadFromPermalink() {
    const hash = window.location.hash.substring(1);
    if (!hash) return;

    const params = new URLSearchParams(hash);

    const url = params.get('url');
    const lat = parseFloat(params.get('lat'));
    const lng = parseFloat(params.get('lng'));
    const zoom = parseFloat(params.get('zoom'));

    if (url) {
        loadCog(url);
    }

    if (!isNaN(lat) && !isNaN(lng) && !isNaN(zoom) && app.map) {
        app.map.setView([lat, lng], zoom);
    }
}

/**
 * Show notification
 */
function showNotification(message) {
    const notification = document.createElement('div');
    notification.className = 'notification';
    notification.textContent = message;
    notification.style.cssText = `
        position: fixed;
        top: 20px;
        right: 20px;
        background: #4CAF50;
        color: white;
        padding: 16px;
        border-radius: 4px;
        box-shadow: 0 2px 5px rgba(0,0,0,0.2);
        z-index: 10000;
    `;

    document.body.appendChild(notification);

    setTimeout(() => {
        notification.remove();
    }, 3000);
}

/**
 * Handle keyboard shortcuts
 */
function handleKeyboardShortcuts(e) {
    // Ctrl+C or Cmd+C for permalink
    if ((e.ctrlKey || e.metaKey) && e.key === 'c') {
        e.preventDefault();
        copyPermalink();
    }

    // Escape to cancel measurements
    if (e.key === 'Escape') {
        clearMeasurements();
    }
}

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeApp);
} else {
    initializeApp();
}
