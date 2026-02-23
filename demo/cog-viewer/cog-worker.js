/**
 * COG Viewer Web Worker
 *
 * This worker handles parallel tile loading for the COG viewer,
 * offloading computationally intensive tasks from the main thread.
 *
 * Features:
 * - Parallel tile fetching
 * - Tile decompression
 * - Image processing
 * - Progress reporting
 */

// Import WASM module
import init, { WasmCogViewer } from '../pkg/oxigdal_wasm.js';

// Worker state
let wasmInitialized = false;
let viewers = new Map(); // URL -> WasmCogViewer
let tileCache = new Map(); // Cache key -> tile data

/**
 * Initialize WASM module
 */
async function initializeWasm() {
    if (wasmInitialized) {
        return;
    }

    try {
        await init();
        wasmInitialized = true;
        console.log('[Worker] WASM initialized successfully');
    } catch (error) {
        console.error('[Worker] Failed to initialize WASM:', error);
        throw error;
    }
}

/**
 * Get or create a viewer for the given URL
 */
async function getViewer(url) {
    if (viewers.has(url)) {
        return viewers.get(url);
    }

    const viewer = new WasmCogViewer();
    await viewer.open(url);
    viewers.set(url, viewer);

    return viewer;
}

/**
 * Load a single tile
 */
async function loadTile(url, coord) {
    const { level, x, y } = coord;
    const cacheKey = `${url}:${level}:${x}:${y}`;

    // Check cache
    if (tileCache.has(cacheKey)) {
        return {
            coord,
            data: tileCache.get(cacheKey),
            cached: true,
        };
    }

    // Fetch tile
    const viewer = await getViewer(url);
    const imageData = await viewer.read_tile_as_image_data(level, x, y);

    // Convert ImageData to transferable format
    const data = {
        width: imageData.width,
        height: imageData.height,
        data: imageData.data.buffer, // Transfer ArrayBuffer
    };

    // Cache the result
    tileCache.set(cacheKey, data);

    // Limit cache size (keep last 100 tiles)
    if (tileCache.size > 100) {
        const firstKey = tileCache.keys().next().value;
        tileCache.delete(firstKey);
    }

    return {
        coord,
        data,
        cached: false,
    };
}

/**
 * Load multiple tiles
 */
async function loadTiles(url, coords) {
    const results = await Promise.all(
        coords.map(coord => loadTile(url, coord))
    );

    return results;
}

/**
 * Prefetch tiles for better performance
 */
async function prefetchTiles(url, coords) {
    let loaded = 0;

    for (const coord of coords) {
        try {
            await loadTile(url, coord);
            loaded++;

            // Report progress
            self.postMessage({
                type: 'progress',
                loaded,
                total: coords.length,
            });
        } catch (error) {
            console.error('[Worker] Failed to prefetch tile:', error);
        }
    }

    return loaded;
}

/**
 * Get metadata for a COG
 */
async function getMetadata(url) {
    const viewer = await getViewer(url);
    return JSON.parse(viewer.metadata_json());
}

/**
 * Message handler
 */
self.addEventListener('message', async (event) => {
    const { job_id, request_type } = event.data;

    try {
        // Initialize WASM if needed
        if (!wasmInitialized) {
            await initializeWasm();
        }

        let response;

        switch (request_type.type) {
            case 'LoadTile':
                const tileResult = await loadTile(
                    request_type.url,
                    request_type.coord
                );
                response = {
                    type: 'TileLoaded',
                    coord: tileResult.coord,
                    data: tileResult.data.data, // ArrayBuffer
                };
                break;

            case 'LoadTiles':
                const tilesResult = await loadTiles(
                    request_type.url,
                    request_type.coords
                );
                response = {
                    type: 'TilesLoaded',
                    tiles: tilesResult.map(r => [r.coord, r.data.data]),
                };
                break;

            case 'Prefetch':
                const count = await prefetchTiles(
                    request_type.url,
                    request_type.coords
                );
                response = {
                    type: 'PrefetchCompleted',
                    count,
                };
                break;

            case 'GetMetadata':
                const metadata = await getMetadata(request_type.url);
                response = {
                    type: 'Metadata',
                    metadata: JSON.stringify(metadata),
                };
                break;

            default:
                throw new Error(`Unknown request type: ${request_type.type}`);
        }

        // Send successful response
        self.postMessage({
            job_id,
            response_type: response,
        });
    } catch (error) {
        console.error('[Worker] Job failed:', error);

        // Send error response
        self.postMessage({
            job_id,
            response_type: {
                type: 'Error',
                message: error.message || error.toString(),
            },
        });
    }
});

/**
 * Error handler
 */
self.addEventListener('error', (event) => {
    console.error('[Worker] Uncaught error:', event.error);
});

/**
 * Unhandled rejection handler
 */
self.addEventListener('unhandledrejection', (event) => {
    console.error('[Worker] Unhandled rejection:', event.reason);
});

console.log('[Worker] COG viewer worker initialized and ready');
