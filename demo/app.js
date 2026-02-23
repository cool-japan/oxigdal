/**
 * OxiGDAL COG Viewer Demo Application
 *
 * This application demonstrates the capabilities of OxiGDAL WASM bindings
 * for viewing Cloud Optimized GeoTIFFs in the browser.
 */

// WASM module imports
import init, { WasmCogViewer, version } from './pkg/oxigdal_wasm.js';

// Application state
const state = {
    viewer: null,
    canvas: null,
    ctx: null,
    currentUrl: null,

    // Viewport state
    viewport: {
        x: 0,
        y: 0,
        scale: 1.0,
        isDragging: false,
        dragStart: { x: 0, y: 0 },
    },

    // Image state
    image: {
        width: 0,
        height: 0,
        tileWidth: 256,
        tileHeight: 256,
    },

    // Tile cache
    tileCache: new Map(),
};

/**
 * Initialize the application
 */
async function initApp() {
    try {
        // Initialize WASM module
        await init();
        console.log('OxiGDAL WASM initialized');

        // Display version
        const versionInfo = document.getElementById('version-info');
        if (versionInfo) {
            versionInfo.textContent = `OxiGDAL v${version()}`;
        }

        // Setup canvas
        state.canvas = document.getElementById('viewer-canvas');
        state.ctx = state.canvas.getContext('2d');
        resizeCanvas();

        // Setup event listeners
        setupEventListeners();

        console.log('Application initialized successfully');
    } catch (error) {
        console.error('Failed to initialize application:', error);
        showError('Failed to initialize application: ' + error.message);
    }
}

/**
 * Setup event listeners
 */
function setupEventListeners() {
    // Load button
    document.getElementById('load-btn').addEventListener('click', loadCogFromInput);

    // URL input - Enter key
    document.getElementById('url-input').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') {
            loadCogFromInput();
        }
    });

    // Example buttons
    document.querySelectorAll('.example-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const url = e.target.dataset.url;
            document.getElementById('url-input').value = url;
            loadCog(url);
        });
    });

    // Zoom controls
    document.getElementById('zoom-in-btn').addEventListener('click', () => zoomBy(1.5));
    document.getElementById('zoom-out-btn').addEventListener('click', () => zoomBy(0.67));
    document.getElementById('reset-view-btn').addEventListener('click', resetView);

    // Canvas mouse events
    state.canvas.addEventListener('mousedown', handleMouseDown);
    state.canvas.addEventListener('mousemove', handleMouseMove);
    state.canvas.addEventListener('mouseup', handleMouseUp);
    state.canvas.addEventListener('mouseleave', handleMouseUp);
    state.canvas.addEventListener('wheel', handleWheel);

    // Window resize
    window.addEventListener('resize', resizeCanvas);
}

/**
 * Load COG from input field
 */
async function loadCogFromInput() {
    const url = document.getElementById('url-input').value.trim();
    if (!url) {
        showError('Please enter a COG URL');
        return;
    }
    await loadCog(url);
}

/**
 * Load a COG from URL
 */
async function loadCog(url) {
    try {
        showLoading('Loading COG...');

        // Create viewer if not exists
        if (!state.viewer) {
            state.viewer = new WasmCogViewer();
        }

        // Open the COG
        await state.viewer.open(url);

        // Update state
        state.currentUrl = url;
        state.image.width = state.viewer.width();
        state.image.height = state.viewer.height();
        state.image.tileWidth = state.viewer.tile_width();
        state.image.tileHeight = state.viewer.tile_height();

        // Clear tile cache
        state.tileCache.clear();

        // Reset viewport
        resetView();

        // Display metadata
        displayMetadata();

        // Render initial view
        await renderView();

        hideLoading();
        console.log(`Loaded COG: ${url}`);
    } catch (error) {
        console.error('Failed to load COG:', error);
        hideLoading();
        showError('Failed to load COG: ' + error);
    }
}

/**
 * Display metadata
 */
function displayMetadata() {
    const container = document.getElementById('metadata-content');
    if (!state.viewer) {
        container.innerHTML = '<p class="empty-state">No COG loaded</p>';
        return;
    }

    const metadata = [
        { label: 'Width', value: state.viewer.width() + ' px' },
        { label: 'Height', value: state.viewer.height() + ' px' },
        { label: 'Tile Size', value: `${state.viewer.tile_width()} x ${state.viewer.tile_height()}` },
        { label: 'Bands', value: state.viewer.band_count() },
        { label: 'Overviews', value: state.viewer.overview_count() },
    ];

    const epsgCode = state.viewer.epsg_code();
    if (epsgCode) {
        metadata.push({ label: 'CRS (EPSG)', value: epsgCode });
    }

    metadata.push({ label: 'URL', value: state.currentUrl });

    container.innerHTML = metadata
        .map(item => `
            <div class="metadata-item">
                <strong>${item.label}:</strong>
                <span>${item.value}</span>
            </div>
        `)
        .join('');
}

/**
 * Resize canvas to fit container
 */
function resizeCanvas() {
    if (!state.canvas) return;

    const wrapper = state.canvas.parentElement;
    state.canvas.width = wrapper.clientWidth;
    state.canvas.height = wrapper.clientHeight;

    if (state.viewer) {
        renderView();
    }
}

/**
 * Reset viewport to fit image
 */
function resetView() {
    if (!state.canvas || state.image.width === 0) return;

    // Calculate scale to fit image in canvas
    const scaleX = state.canvas.width / state.image.width;
    const scaleY = state.canvas.height / state.image.height;
    state.viewport.scale = Math.min(scaleX, scaleY, 1.0);

    // Center the image
    state.viewport.x = (state.canvas.width - state.image.width * state.viewport.scale) / 2;
    state.viewport.y = (state.canvas.height - state.image.height * state.viewport.scale) / 2;

    updateZoomDisplay();
    renderView();
}

/**
 * Zoom by factor
 */
function zoomBy(factor) {
    if (!state.canvas || state.image.width === 0) return;

    const oldScale = state.viewport.scale;
    state.viewport.scale *= factor;

    // Limit zoom range
    const minScale = Math.min(state.canvas.width / state.image.width, state.canvas.height / state.image.height) * 0.1;
    const maxScale = 10.0;
    state.viewport.scale = Math.max(minScale, Math.min(maxScale, state.viewport.scale));

    // Zoom towards center
    const centerX = state.canvas.width / 2;
    const centerY = state.canvas.height / 2;

    const scaleDiff = state.viewport.scale / oldScale;
    state.viewport.x = centerX - (centerX - state.viewport.x) * scaleDiff;
    state.viewport.y = centerY - (centerY - state.viewport.y) * scaleDiff;

    updateZoomDisplay();
    renderView();
}

/**
 * Update zoom level display
 */
function updateZoomDisplay() {
    const element = document.getElementById('zoom-level');
    if (element) {
        element.textContent = `Zoom: ${state.viewport.scale.toFixed(2)}x`;
    }
}

/**
 * Mouse down handler
 */
function handleMouseDown(e) {
    state.viewport.isDragging = true;
    state.viewport.dragStart = {
        x: e.clientX - state.viewport.x,
        y: e.clientY - state.viewport.y,
    };
}

/**
 * Mouse move handler
 */
function handleMouseMove(e) {
    if (!state.viewport.isDragging) return;

    state.viewport.x = e.clientX - state.viewport.dragStart.x;
    state.viewport.y = e.clientY - state.viewport.dragStart.y;

    renderView();
}

/**
 * Mouse up handler
 */
function handleMouseUp() {
    state.viewport.isDragging = false;
}

/**
 * Mouse wheel handler
 */
function handleWheel(e) {
    e.preventDefault();

    const factor = e.deltaY < 0 ? 1.2 : 0.83;
    const oldScale = state.viewport.scale;
    state.viewport.scale *= factor;

    // Limit zoom range
    const minScale = Math.min(state.canvas.width / state.image.width, state.canvas.height / state.image.height) * 0.1;
    const maxScale = 10.0;
    state.viewport.scale = Math.max(minScale, Math.min(maxScale, state.viewport.scale));

    // Zoom towards mouse position
    const rect = state.canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const scaleDiff = state.viewport.scale / oldScale;
    state.viewport.x = mouseX - (mouseX - state.viewport.x) * scaleDiff;
    state.viewport.y = mouseY - (mouseY - state.viewport.y) * scaleDiff;

    updateZoomDisplay();
    renderView();
}

/**
 * Render the current view
 */
async function renderView() {
    if (!state.ctx || !state.viewer) return;

    // Clear canvas
    state.ctx.fillStyle = '#f0f0f0';
    state.ctx.fillRect(0, 0, state.canvas.width, state.canvas.height);

    // Calculate visible tiles
    const visibleTiles = calculateVisibleTiles();

    // Render tiles
    for (const tile of visibleTiles) {
        await renderTile(tile);
    }
}

/**
 * Calculate which tiles are visible in the current viewport
 */
function calculateVisibleTiles() {
    const tiles = [];

    // Use level 0 for now (full resolution)
    const level = 0;

    // Calculate tile grid dimensions
    const tilesX = Math.ceil(state.image.width / state.image.tileWidth);
    const tilesY = Math.ceil(state.image.height / state.image.tileHeight);

    // Calculate visible area in image coordinates
    const viewLeft = -state.viewport.x / state.viewport.scale;
    const viewTop = -state.viewport.y / state.viewport.scale;
    const viewRight = viewLeft + state.canvas.width / state.viewport.scale;
    const viewBottom = viewTop + state.canvas.height / state.viewport.scale;

    // Calculate tile range
    const startTileX = Math.max(0, Math.floor(viewLeft / state.image.tileWidth));
    const endTileX = Math.min(tilesX - 1, Math.ceil(viewRight / state.image.tileWidth));
    const startTileY = Math.max(0, Math.floor(viewTop / state.image.tileHeight));
    const endTileY = Math.min(tilesY - 1, Math.ceil(viewBottom / state.image.tileHeight));

    // Collect visible tiles
    for (let ty = startTileY; ty <= endTileY; ty++) {
        for (let tx = startTileX; tx <= endTileX; tx++) {
            tiles.push({ level, x: tx, y: ty });
        }
    }

    return tiles;
}

/**
 * Render a single tile
 */
async function renderTile(tile) {
    const cacheKey = `${tile.level}-${tile.x}-${tile.y}`;

    // Check cache
    let imageData = state.tileCache.get(cacheKey);

    if (!imageData) {
        try {
            // Load tile
            imageData = await state.viewer.read_tile_as_image_data(tile.level, tile.x, tile.y);
            state.tileCache.set(cacheKey, imageData);
        } catch (error) {
            console.error(`Failed to load tile ${cacheKey}:`, error);
            return;
        }
    }

    // Calculate tile position on canvas
    const tileX = tile.x * state.image.tileWidth;
    const tileY = tile.y * state.image.tileHeight;

    const screenX = tileX * state.viewport.scale + state.viewport.x;
    const screenY = tileY * state.viewport.scale + state.viewport.y;
    const screenWidth = state.image.tileWidth * state.viewport.scale;
    const screenHeight = state.image.tileHeight * state.viewport.scale;

    // Draw tile
    state.ctx.putImageData(imageData, screenX, screenY);

    // If zoomed, scale the tile
    if (state.viewport.scale !== 1.0) {
        // Create temporary canvas for scaling
        const tempCanvas = document.createElement('canvas');
        tempCanvas.width = state.image.tileWidth;
        tempCanvas.height = state.image.tileHeight;
        const tempCtx = tempCanvas.getContext('2d');
        tempCtx.putImageData(imageData, 0, 0);

        // Draw scaled
        state.ctx.drawImage(tempCanvas, screenX, screenY, screenWidth, screenHeight);
    }
}

/**
 * Show loading overlay
 */
function showLoading(message) {
    const overlay = document.getElementById('loading-overlay');
    const text = document.getElementById('loading-text');
    if (overlay) {
        overlay.classList.remove('hidden');
        if (text && message) {
            text.textContent = message;
        }
    }
}

/**
 * Hide loading overlay
 */
function hideLoading() {
    const overlay = document.getElementById('loading-overlay');
    if (overlay) {
        overlay.classList.add('hidden');
    }
}

/**
 * Show error message
 */
function showError(message) {
    const container = document.getElementById('metadata-content');
    if (container) {
        container.innerHTML = `
            <div class="error-message">
                <strong>Error:</strong> ${message}
            </div>
        `;
    }
}

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initApp);
} else {
    initApp();
}
