/* @ts-self-types="./oxigdal_wasm.d.ts" */

/**
 * Advanced COG viewer with comprehensive tile management and caching
 *
 * This is the recommended viewer for production applications. It provides
 * advanced features including:
 *
 * - **LRU Tile Caching**: Automatic memory management with configurable size
 * - **Viewport Management**: Pan, zoom, and viewport history (undo/redo)
 * - **Prefetching**: Intelligent prefetching of nearby tiles
 * - **Multi-resolution**: Automatic selection of appropriate overview level
 * - **Image Processing**: Built-in contrast enhancement and statistics
 * - **Performance Tracking**: Cache hit rates and loading metrics
 *
 * # Memory Management
 *
 * The viewer uses an LRU (Least Recently Used) cache to manage memory
 * efficiently. When the cache is full, the least recently accessed tiles
 * are evicted. Configure the cache size based on your application's memory
 * constraints and typical usage patterns.
 *
 * Recommended cache sizes:
 * - Mobile devices: 50-100 MB
 * - Desktop browsers: 100-500 MB
 * - High-end workstations: 500-1000 MB
 *
 * # Prefetching Strategies
 *
 * The viewer supports multiple prefetching strategies:
 *
 * - **None**: No prefetching (lowest memory, highest latency)
 * - **Neighbors**: Prefetch immediately adjacent tiles
 * - **Pyramid**: Prefetch parent and child tiles (smooth zooming)
 *
 * # Performance Optimization
 *
 * For best performance:
 * 1. Use an appropriate cache size (100-200 MB recommended)
 * 2. Enable prefetching for smoother user experience
 * 3. Use viewport management to minimize unnecessary tile loads
 * 4. Monitor cache statistics to tune parameters
 *
 * # Example
 *
 * ```javascript
 * const viewer = new AdvancedCogViewer();
 * await viewer.open('https://example.com/image.tif', 100); // 100MB cache
 *
 * // Setup viewport
 * viewer.setViewportSize(800, 600);
 * viewer.fitToImage();
 *
 * // Enable prefetching
 * viewer.setPrefetchStrategy('neighbors');
 *
 * // Load and display tiles
 * const imageData = await viewer.readTileAsImageData(0, 0, 0);
 * ctx.putImageData(imageData, 0, 0);
 *
 * // Check performance
 * const stats = JSON.parse(viewer.getCacheStats());
 * console.log(`Hit rate: ${stats.hit_count / (stats.hit_count + stats.miss_count)}`);
 * ```
 */
export class AdvancedCogViewer {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        AdvancedCogViewerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_advancedcogviewer_free(ptr, 0);
    }
    /**
     * Returns the number of bands
     * @returns {number}
     */
    band_count() {
        const ret = wasm.advancedcogviewer_band_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Centers the viewport on a point
     * @param {number} x
     * @param {number} y
     */
    centerOn(x, y) {
        wasm.advancedcogviewer_centerOn(this.__wbg_ptr, x, y);
    }
    /**
     * Clears the tile cache
     */
    clearCache() {
        wasm.advancedcogviewer_clearCache(this.__wbg_ptr);
    }
    /**
     * Computes histogram for a region
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<string>}
     */
    computeHistogram(level, tile_x, tile_y) {
        const ret = wasm.advancedcogviewer_computeHistogram(this.__wbg_ptr, level, tile_x, tile_y);
        return ret;
    }
    /**
     * Computes image statistics for a region
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<string>}
     */
    computeStats(level, tile_x, tile_y) {
        const ret = wasm.advancedcogviewer_computeStats(this.__wbg_ptr, level, tile_x, tile_y);
        return ret;
    }
    /**
     * Returns the EPSG code if available
     * @returns {number | undefined}
     */
    epsg_code() {
        const ret = wasm.advancedcogviewer_epsg_code(this.__wbg_ptr);
        return ret === 0x100000001 ? undefined : ret;
    }
    /**
     * Fits the viewport to the image
     */
    fitToImage() {
        wasm.advancedcogviewer_fitToImage(this.__wbg_ptr);
    }
    /**
     * Returns cache statistics as JSON
     * @returns {string | undefined}
     */
    getCacheStats() {
        const ret = wasm.advancedcogviewer_getCacheStats(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Returns comprehensive metadata as JSON
     * @returns {string}
     */
    getMetadata() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.advancedcogviewer_getMetadata(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Returns the current viewport as JSON
     * @returns {string | undefined}
     */
    getViewport() {
        const ret = wasm.advancedcogviewer_getViewport(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Returns the image height
     * @returns {bigint}
     */
    height() {
        const ret = wasm.advancedcogviewer_height(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
    /**
     * Creates a new advanced COG viewer
     */
    constructor() {
        const ret = wasm.advancedcogviewer_new();
        this.__wbg_ptr = ret >>> 0;
        AdvancedCogViewerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Opens a COG file from a URL with advanced caching enabled
     *
     * This method initializes the viewer with full caching and viewport management.
     * It performs the following operations:
     *
     * 1. **Initial Connection**: Sends HEAD request to validate URL and check range support
     * 2. **Header Parsing**: Fetches and parses TIFF header (8-16 bytes)
     * 3. **Metadata Extraction**: Parses IFD to extract image dimensions, tile size, bands
     * 4. **GeoTIFF Tags**: Extracts coordinate system information (EPSG, geotransform)
     * 5. **Pyramid Creation**: Builds tile pyramid structure for all overview levels
     * 6. **Cache Initialization**: Creates LRU cache with specified size
     * 7. **Viewport Setup**: Initializes viewport with default settings
     *
     * # Arguments
     *
     * * `url` - The URL of the COG file. Must support HTTP range requests (Accept-Ranges: bytes)
     *           and have proper CORS headers configured.
     * * `cache_size_mb` - Size of the tile cache in megabytes. Recommended values:
     *   - Mobile: 50-100 MB
     *   - Desktop: 100-500 MB
     *   - High-end: 500-1000 MB
     *
     * # Returns
     *
     * Returns `Ok(())` on successful initialization, or a JavaScript error on failure.
     *
     * # Errors
     *
     * This method can fail for several reasons:
     *
     * ## Network Errors
     * - Connection timeout
     * - DNS resolution failure
     * - SSL/TLS errors
     *
     * ## HTTP Errors
     * - 404 Not Found: File doesn't exist at the URL
     * - 403 Forbidden: Access denied
     * - 500 Server Error: Server-side issues
     *
     * ## CORS Errors
     * - Missing Access-Control-Allow-Origin header
     * - Missing Access-Control-Allow-Headers for range requests
     *
     * ## Format Errors
     * - Invalid TIFF magic bytes
     * - Corrupted IFD structure
     * - Unsupported TIFF variant
     * - Missing required tags
     *
     * # Performance Considerations
     *
     * Opening a COG typically requires 2-4 HTTP requests:
     * 1. HEAD request (~10ms)
     * 2. Header fetch (~20ms for 16 bytes)
     * 3. IFD fetch (~50ms for typical IFD)
     * 4. GeoTIFF tags fetch (~30ms if separate)
     *
     * Total typical open time: 100-200ms on good connections.
     *
     * # Example
     *
     * ```javascript
     * const viewer = new AdvancedCogViewer();
     *
     * try {
     *     // Open with 100MB cache
     *     await viewer.open('https://example.com/landsat8.tif', 100);
     *
     *     console.log(`Opened: ${viewer.width()}x${viewer.height()}`);
     *     console.log(`Tiles: ${viewer.tile_width()}x${viewer.tile_height()}`);
     *     console.log(`Cache size: 100 MB`);
     * } catch (error) {
     *     if (error.message.includes('404')) {
     *         console.error('File not found');
     *     } else if (error.message.includes('CORS')) {
     *         console.error('CORS not configured. Add these headers:');
     *         console.error('  Access-Control-Allow-Origin: *');
     *         console.error('  Access-Control-Allow-Headers: Range');
     *     } else {
     *         console.error('Failed to open:', error.message);
     *     }
     * }
     * ```
     *
     * # See Also
     *
     * - `WasmCogViewer::open()` - Simple version without caching
     * - `set_prefetch_strategy()` - Configure prefetching after opening
     * - `get_cache_stats()` - Monitor cache performance
     * @param {string} url
     * @param {number} cache_size_mb
     * @returns {Promise<void>}
     */
    open(url, cache_size_mb) {
        const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.advancedcogviewer_open(this.__wbg_ptr, ptr0, len0, cache_size_mb);
        return ret;
    }
    /**
     * Returns the number of overview levels
     * @returns {number}
     */
    overview_count() {
        const ret = wasm.advancedcogviewer_overview_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Pans the viewport
     * @param {number} dx
     * @param {number} dy
     */
    pan(dx, dy) {
        wasm.advancedcogviewer_pan(this.__wbg_ptr, dx, dy);
    }
    /**
     * Reads a tile as ImageData with caching
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<ImageData>}
     */
    readTileAsImageData(level, tile_x, tile_y) {
        const ret = wasm.advancedcogviewer_readTileAsImageData(this.__wbg_ptr, level, tile_x, tile_y);
        return ret;
    }
    /**
     * Reads a tile with caching
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<Uint8Array>}
     */
    readTileCached(level, tile_x, tile_y) {
        const ret = wasm.advancedcogviewer_readTileCached(this.__wbg_ptr, level, tile_x, tile_y);
        return ret;
    }
    /**
     * Applies contrast enhancement to a tile
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @param {string} method
     * @returns {Promise<ImageData>}
     */
    readTileWithContrast(level, tile_x, tile_y, method) {
        const ptr0 = passStringToWasm0(method, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.advancedcogviewer_readTileWithContrast(this.__wbg_ptr, level, tile_x, tile_y, ptr0, len0);
        return ret;
    }
    /**
     * Sets the prefetch strategy
     * @param {string} strategy
     */
    setPrefetchStrategy(strategy) {
        const ptr0 = passStringToWasm0(strategy, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.advancedcogviewer_setPrefetchStrategy(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Sets the viewport size
     * @param {number} width
     * @param {number} height
     */
    setViewportSize(width, height) {
        wasm.advancedcogviewer_setViewportSize(this.__wbg_ptr, width, height);
    }
    /**
     * Sets the zoom level
     * @param {number} zoom
     */
    setZoom(zoom) {
        wasm.advancedcogviewer_setZoom(this.__wbg_ptr, zoom);
    }
    /**
     * Returns the tile height
     * @returns {number}
     */
    tile_height() {
        const ret = wasm.advancedcogviewer_tile_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the tile width
     * @returns {number}
     */
    tile_width() {
        const ret = wasm.advancedcogviewer_tile_width(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the URL
     * @returns {string | undefined}
     */
    url() {
        const ret = wasm.advancedcogviewer_url(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Returns the image width
     * @returns {bigint}
     */
    width() {
        const ret = wasm.advancedcogviewer_width(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
    /**
     * Zooms in
     */
    zoomIn() {
        wasm.advancedcogviewer_zoomIn(this.__wbg_ptr);
    }
    /**
     * Zooms out
     */
    zoomOut() {
        wasm.advancedcogviewer_zoomOut(this.__wbg_ptr);
    }
}
if (Symbol.dispose) AdvancedCogViewer.prototype[Symbol.dispose] = AdvancedCogViewer.prototype.free;

/**
 * Batch tile loader for efficient multi-tile loading
 */
export class BatchTileLoader {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BatchTileLoaderFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_batchtileloader_free(ptr, 0);
    }
    /**
     * Loads multiple tiles in parallel
     * @param {number} level
     * @param {Uint32Array} tile_coords
     * @returns {Promise<any[]>}
     */
    loadTilesBatch(level, tile_coords) {
        const ptr0 = passArray32ToWasm0(tile_coords, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.batchtileloader_loadTilesBatch(this.__wbg_ptr, level, ptr0, len0);
        return ret;
    }
    /**
     * Creates a new batch tile loader
     * @param {number} max_parallel
     */
    constructor(max_parallel) {
        const ret = wasm.batchtileloader_new(max_parallel);
        this.__wbg_ptr = ret >>> 0;
        BatchTileLoaderFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Opens a COG
     * @param {string} url
     * @param {number} cache_size_mb
     * @returns {Promise<void>}
     */
    open(url, cache_size_mb) {
        const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.batchtileloader_open(this.__wbg_ptr, ptr0, len0, cache_size_mb);
        return ret;
    }
}
if (Symbol.dispose) BatchTileLoader.prototype[Symbol.dispose] = BatchTileLoader.prototype.free;

/**
 * GeoJSON export utilities
 */
export class GeoJsonExporter {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        GeoJsonExporterFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_geojsonexporter_free(ptr, 0);
    }
    /**
     * Exports image bounds as GeoJSON
     * @param {number} west
     * @param {number} south
     * @param {number} east
     * @param {number} north
     * @param {number | null} [epsg]
     * @returns {string}
     */
    static exportBounds(west, south, east, north, epsg) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.geojsonexporter_exportBounds(west, south, east, north, isLikeNone(epsg) ? 0x100000001 : (epsg) >>> 0);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Exports a point as GeoJSON
     * @param {number} x
     * @param {number} y
     * @param {string} properties
     * @returns {string}
     */
    static exportPoint(x, y, properties) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(properties, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.geojsonexporter_exportPoint(x, y, ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
}
if (Symbol.dispose) GeoJsonExporter.prototype[Symbol.dispose] = GeoJsonExporter.prototype.free;

/**
 * WASM-compatible COG (Cloud Optimized GeoTIFF) viewer
 *
 * This is the basic COG viewer for browser-based geospatial data visualization.
 * It provides simple access to COG metadata and tile reading functionality.
 *
 * # Features
 *
 * - Efficient tile-based access to large GeoTIFF files
 * - Support for multi-band imagery
 * - Overview/pyramid level access for different zoom levels
 * - CORS-compatible HTTP range request support
 * - Automatic TIFF header parsing
 * - GeoTIFF metadata extraction (CRS, geotransform, etc.)
 *
 * # Performance
 *
 * The viewer uses HTTP range requests to fetch only the required portions
 * of the file, making it efficient for large files. However, for production
 * use cases with caching and advanced features, consider using
 * `AdvancedCogViewer` instead.
 *
 * # Example
 *
 * ```javascript
 * const viewer = new WasmCogViewer();
 * await viewer.open('https://example.com/image.tif');
 * console.log(`Size: ${viewer.width()}x${viewer.height()}`);
 * const tile = await viewer.read_tile_as_image_data(0, 0, 0);
 * ```
 */
export class WasmCogViewer {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmCogViewerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmcogviewer_free(ptr, 0);
    }
    /**
     * Returns the number of bands
     * @returns {number}
     */
    band_count() {
        const ret = wasm.wasmcogviewer_band_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the EPSG code if available
     * @returns {number | undefined}
     */
    epsg_code() {
        const ret = wasm.wasmcogviewer_epsg_code(this.__wbg_ptr);
        return ret === 0x100000001 ? undefined : ret;
    }
    /**
     * Returns the image height
     * @returns {bigint}
     */
    height() {
        const ret = wasm.wasmcogviewer_height(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
    /**
     * Returns metadata as JSON
     * @returns {string}
     */
    metadata_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmcogviewer_metadata_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Creates a new COG viewer
     */
    constructor() {
        const ret = wasm.wasmcogviewer_new();
        this.__wbg_ptr = ret >>> 0;
        WasmCogViewerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Opens a COG file from a URL
     *
     * This method performs the following operations:
     * 1. Sends a HEAD request to determine file size and range support
     * 2. Fetches the TIFF header to validate format
     * 3. Parses IFD (Image File Directory) to extract metadata
     * 4. Extracts GeoTIFF tags for coordinate system information
     * 5. Counts overview levels for multi-resolution support
     *
     * # Arguments
     *
     * * `url` - The URL of the COG file to open. Must support HTTP range requests
     *           for optimal performance. CORS must be properly configured.
     *
     * # Returns
     *
     * Returns `Ok(())` on success, or a JavaScript error on failure.
     *
     * # Errors
     *
     * This method can fail for several reasons:
     * - Network errors (no connection, timeout, etc.)
     * - HTTP errors (404, 403, 500, etc.)
     * - CORS errors (missing headers)
     * - Invalid TIFF format
     * - Unsupported TIFF variant
     *
     * # Example
     *
     * ```javascript
     * const viewer = new WasmCogViewer();
     * try {
     *     await viewer.open('https://example.com/landsat.tif');
     *     console.log('Successfully opened COG');
     * } catch (error) {
     *     console.error('Failed to open:', error);
     * }
     * ```
     * @param {string} url
     * @returns {Promise<void>}
     */
    open(url) {
        const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmcogviewer_open(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    /**
     * Returns the number of overview levels
     * @returns {number}
     */
    overview_count() {
        const ret = wasm.wasmcogviewer_overview_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns pixel scale X (degrees/pixel in lon direction)
     * @returns {number | undefined}
     */
    pixel_scale_x() {
        const ret = wasm.wasmcogviewer_pixel_scale_x(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Returns pixel scale Y (degrees/pixel in lat direction, negative)
     * @returns {number | undefined}
     */
    pixel_scale_y() {
        const ret = wasm.wasmcogviewer_pixel_scale_y(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Reads a tile and returns raw bytes
     * @param {number} _level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<Uint8Array>}
     */
    read_tile(_level, tile_x, tile_y) {
        const ret = wasm.wasmcogviewer_read_tile(this.__wbg_ptr, _level, tile_x, tile_y);
        return ret;
    }
    /**
     * Reads a tile and converts to RGBA ImageData for canvas rendering
     * @param {number} level
     * @param {number} tile_x
     * @param {number} tile_y
     * @returns {Promise<ImageData>}
     */
    read_tile_as_image_data(level, tile_x, tile_y) {
        const ret = wasm.wasmcogviewer_read_tile_as_image_data(this.__wbg_ptr, level, tile_x, tile_y);
        return ret;
    }
    /**
     * Returns tiepoint geo X (top-left longitude)
     * @returns {number | undefined}
     */
    tiepoint_geo_x() {
        const ret = wasm.wasmcogviewer_tiepoint_geo_x(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Returns tiepoint geo Y (top-left latitude)
     * @returns {number | undefined}
     */
    tiepoint_geo_y() {
        const ret = wasm.wasmcogviewer_tiepoint_geo_y(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Returns the tile height
     * @returns {number}
     */
    tile_height() {
        const ret = wasm.wasmcogviewer_tile_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the tile width
     * @returns {number}
     */
    tile_width() {
        const ret = wasm.wasmcogviewer_tile_width(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the URL
     * @returns {string | undefined}
     */
    url() {
        const ret = wasm.wasmcogviewer_url(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Returns the image width
     * @returns {bigint}
     */
    width() {
        const ret = wasm.wasmcogviewer_width(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
}
if (Symbol.dispose) WasmCogViewer.prototype[Symbol.dispose] = WasmCogViewer.prototype.free;

/**
 * WASM bindings for color operations
 */
export class WasmColorPalette {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(WasmColorPalette.prototype);
        obj.__wbg_ptr = ptr;
        WasmColorPaletteFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmColorPaletteFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmcolorpalette_free(ptr, 0);
    }
    /**
     * Applies the palette to grayscale data
     * @param {Uint8Array} data
     */
    applyToGrayscale(data) {
        var ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmcolorpalette_applyToGrayscale(this.__wbg_ptr, ptr0, len0, data);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Creates a plasma palette
     * @returns {WasmColorPalette}
     */
    static createPlasma() {
        const ret = wasm.wasmcolorpalette_createPlasma();
        return WasmColorPalette.__wrap(ret);
    }
    /**
     * Creates a terrain palette
     * @returns {WasmColorPalette}
     */
    static createTerrain() {
        const ret = wasm.wasmcolorpalette_createTerrain();
        return WasmColorPalette.__wrap(ret);
    }
    /**
     * Creates a viridis palette
     * @returns {WasmColorPalette}
     */
    static createViridis() {
        const ret = wasm.wasmcolorpalette_createViridis();
        return WasmColorPalette.__wrap(ret);
    }
}
if (Symbol.dispose) WasmColorPalette.prototype[Symbol.dispose] = WasmColorPalette.prototype.free;

/**
 * WASM bindings for canvas operations
 */
export class WasmImageProcessor {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmImageProcessorFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmimageprocessor_free(ptr, 0);
    }
    /**
     * Computes histogram as JSON
     * @param {Uint8Array} data
     * @param {number} width
     * @param {number} height
     * @returns {string}
     */
    static computeHistogram(data, width, height) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.wasmimageprocessor_computeHistogram(ptr0, len0, width, height);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Computes statistics as JSON
     * @param {Uint8Array} data
     * @param {number} width
     * @param {number} height
     * @returns {string}
     */
    static computeStats(data, width, height) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.wasmimageprocessor_computeStats(ptr0, len0, width, height);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Creates ImageData from RGBA bytes
     * @param {Uint8Array} data
     * @param {number} width
     * @param {number} height
     * @returns {ImageData}
     */
    static createImageData(data, width, height) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmimageprocessor_createImageData(ptr0, len0, width, height);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    /**
     * Applies histogram equalization
     * @param {Uint8Array} data
     * @param {number} width
     * @param {number} height
     */
    static histogramEqualization(data, width, height) {
        var ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmimageprocessor_histogramEqualization(ptr0, len0, data, width, height);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Applies linear stretch
     * @param {Uint8Array} data
     * @param {number} width
     * @param {number} height
     */
    static linearStretch(data, width, height) {
        var ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmimageprocessor_linearStretch(ptr0, len0, data, width, height);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
}
if (Symbol.dispose) WasmImageProcessor.prototype[Symbol.dispose] = WasmImageProcessor.prototype.free;

/**
 * WASM bindings for profiler
 */
export class WasmProfiler {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmProfilerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmprofiler_free(ptr, 0);
    }
    /**
     * Returns all statistics as JSON
     * @returns {string}
     */
    getAllStats() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmprofiler_getAllStats(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Returns counter statistics as JSON
     * @param {string} name
     * @returns {string | undefined}
     */
    getCounterStats(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmprofiler_getCounterStats(this.__wbg_ptr, ptr0, len0);
        let v2;
        if (ret[0] !== 0) {
            v2 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v2;
    }
    /**
     * Creates a new profiler
     */
    constructor() {
        const ret = wasm.wasmprofiler_new();
        this.__wbg_ptr = ret >>> 0;
        WasmProfilerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Records a timing sample
     * @param {string} name
     * @param {number} duration_ms
     */
    record(name, duration_ms) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.wasmprofiler_record(this.__wbg_ptr, ptr0, len0, duration_ms);
    }
    /**
     * Records current memory usage
     */
    recordMemory() {
        wasm.wasmprofiler_recordMemory(this.__wbg_ptr);
    }
    /**
     * Resets all counters
     */
    reset() {
        wasm.wasmprofiler_reset(this.__wbg_ptr);
    }
    /**
     * Starts a timer
     * @param {string} name
     */
    startTimer(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.wasmprofiler_startTimer(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Stops a timer
     * @param {string} name
     */
    stopTimer(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.wasmprofiler_stopTimer(this.__wbg_ptr, ptr0, len0);
    }
}
if (Symbol.dispose) WasmProfiler.prototype[Symbol.dispose] = WasmProfiler.prototype.free;

/**
 * WASM bindings for tile management
 */
export class WasmTileCache {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmTileCacheFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmtilecache_free(ptr, 0);
    }
    /**
     * Clears the cache
     */
    clear() {
        wasm.wasmtilecache_clear(this.__wbg_ptr);
    }
    /**
     * Gets cache statistics as JSON
     * @returns {string}
     */
    getStats() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmtilecache_getStats(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Returns the cache hit rate
     * @returns {number}
     */
    hitRate() {
        const ret = wasm.wasmtilecache_hitRate(this.__wbg_ptr);
        return ret;
    }
    /**
     * Creates a new tile cache
     * @param {number} max_size_mb
     */
    constructor(max_size_mb) {
        const ret = wasm.wasmtilecache_new(max_size_mb);
        this.__wbg_ptr = ret >>> 0;
        WasmTileCacheFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
}
if (Symbol.dispose) WasmTileCache.prototype[Symbol.dispose] = WasmTileCache.prototype.free;

/**
 * WASM bindings for worker pool (for demonstration/testing)
 */
export class WasmWorkerPool {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmWorkerPoolFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmworkerpool_free(ptr, 0);
    }
    /**
     * Returns a message about worker support
     * @returns {string}
     */
    getInfo() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmworkerpool_getInfo(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Creates a new worker pool
     * @param {number} pool_size
     */
    constructor(pool_size) {
        const ret = wasm.wasmworkerpool_new(pool_size);
        this.__wbg_ptr = ret >>> 0;
        WasmWorkerPoolFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Returns the pool size
     * @returns {number}
     */
    poolSize() {
        const ret = wasm.wasmworkerpool_poolSize(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) WasmWorkerPool.prototype[Symbol.dispose] = WasmWorkerPool.prototype.free;

/**
 * Initialize the WASM module with better error handling
 */
export function init() {
    wasm.init();
}

/**
 * Checks if the given URL points to a TIFF file by reading the header
 *
 * # Errors
 * Returns an error if the URL cannot be fetched or the header cannot be read
 * @param {string} url
 * @returns {Promise<boolean>}
 */
export function is_tiff_url(url) {
    const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.is_tiff_url(ptr0, len0);
    return ret;
}

/**
 * Version information
 * @returns {string}
 */
export function version() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.version();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_copy_to_typed_array_fc0809a4dec43528: function(arg0, arg1, arg2) {
            new Uint8Array(arg2.buffer, arg2.byteOffset, arg2.byteLength).set(getArrayU8FromWasm0(arg0, arg1));
        },
        __wbg___wbindgen_debug_string_0bc8482c6e3508ae: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_0095a73b8b156f76: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_undefined_9e4d92534c42d778: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_throw_be289d5034ed271b: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_d9b87ff7982e3b21: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_arrayBuffer_bb54076166006c39: function() { return handleError(function (arg0) {
            const ret = arg0.arrayBuffer();
            return ret;
        }, arguments); },
        __wbg_call_389efe28435a9388: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_call_4708e0c13bdc8e95: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_error_7534b8e9a36f1ab4: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_fetch_e6e8e0a221783759: function(arg0, arg1) {
            const ret = arg0.fetch(arg1);
            return ret;
        },
        __wbg_get_941633a1d2f510cb: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg1.get(getStringFromWasm0(arg2, arg3));
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_headers_59a2938db9f80985: function(arg0) {
            const ret = arg0.headers;
            return ret;
        },
        __wbg_instanceof_Response_ee1d54d79ae41977: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Response;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_ed49b2db8df90359: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_length_32ed9a279acd054c: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_log_6b5ca2e6124b2808: function(arg0) {
            console.log(arg0);
        },
        __wbg_new_361308b2356cecd0: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_64284bd487f9d239: function() { return handleError(function () {
            const ret = new Headers();
            return ret;
        }, arguments); },
        __wbg_new_8a6f238a6ece86ea: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_b5d9e2fb389fef91: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue__wasm_bindgen_2fd503ddb8ec2886___JsValue_____(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = state0.b = 0;
            }
        },
        __wbg_new_dd2b680c8bf6ae29: function(arg0) {
            const ret = new Uint8Array(arg0);
            return ret;
        },
        __wbg_new_no_args_1c7c842f08d00ebb: function(arg0, arg1) {
            const ret = new Function(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_with_str_and_init_a61cbc6bdef21614: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new Request(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_new_with_u8_clamped_array_and_sh_0c0b789ceb2eab31: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = new ImageData(getClampedArrayU8FromWasm0(arg0, arg1), arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_now_a3af9a2f4bbaa4d1: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_ok_87f537440a0acf85: function(arg0) {
            const ret = arg0.ok;
            return ret;
        },
        __wbg_prototypesetcall_bdcdcc5842e4d77d: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_queueMicrotask_0aa0a927f78f5d98: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queueMicrotask_5bb536982f78a56f: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_resolve_002c4b7d9d8f6b64: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_set_db769d02949a271d: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.set(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_set_headers_cfc5f4b2c1f20549: function(arg0, arg1) {
            arg0.headers = arg1;
        },
        __wbg_set_method_c3e20375f5ae7fac: function(arg0, arg1, arg2) {
            arg0.method = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_mode_b13642c312648202: function(arg0, arg1) {
            arg0.mode = __wbindgen_enum_RequestMode[arg1];
        },
        __wbg_stack_0ed75d68575b0f3c: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_12837167ad935116: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_e628e89ab3b1c95f: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_a621d3dfbb60d0ce: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_f8727f0cf888e0bd: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_statusText_556131a02d60f5cd: function(arg0, arg1) {
            const ret = arg1.statusText;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_status_89d7e803db911ee7: function(arg0) {
            const ret = arg0.status;
            return ret;
        },
        __wbg_then_0d9fe2c7b1857d32: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_b9e7b3b5f1a9e1b5: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 85, function: Function { arguments: [Externref], shim_idx: 86, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.wasm_bindgen_2fd503ddb8ec2886___closure__destroy___dyn_core_eda1fd0e4a34c130___ops__function__FnMut__wasm_bindgen_2fd503ddb8ec2886___JsValue____Output_______, wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue_____);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            var v0 = getArrayJsValueFromWasm0(arg0, arg1).slice();
            wasm.__wbindgen_free(arg0, arg1 * 4, 4);
            // Cast intrinsic for `Vector(Externref) -> Externref`.
            const ret = v0;
            return ret;
        },
        __wbindgen_cast_0000000000000004: function(arg0, arg1) {
            var v0 = getArrayU8FromWasm0(arg0, arg1).slice();
            wasm.__wbindgen_free(arg0, arg1 * 1, 1);
            // Cast intrinsic for `Vector(U8) -> Externref`.
            const ret = v0;
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./oxigdal_wasm_bg.js": import0,
    };
}

function wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue_____(arg0, arg1, arg2) {
    wasm.wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue_____(arg0, arg1, arg2);
}

function wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue__wasm_bindgen_2fd503ddb8ec2886___JsValue_____(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue__wasm_bindgen_2fd503ddb8ec2886___JsValue_____(arg0, arg1, arg2, arg3);
}


const __wbindgen_enum_RequestMode = ["same-origin", "no-cors", "cors", "navigate"];
const AdvancedCogViewerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_advancedcogviewer_free(ptr >>> 0, 1));
const BatchTileLoaderFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_batchtileloader_free(ptr >>> 0, 1));
const GeoJsonExporterFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_geojsonexporter_free(ptr >>> 0, 1));
const WasmCogViewerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmcogviewer_free(ptr >>> 0, 1));
const WasmColorPaletteFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmcolorpalette_free(ptr >>> 0, 1));
const WasmImageProcessorFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmimageprocessor_free(ptr >>> 0, 1));
const WasmProfilerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmprofiler_free(ptr >>> 0, 1));
const WasmTileCacheFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmtilecache_free(ptr >>> 0, 1));
const WasmWorkerPoolFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmworkerpool_free(ptr >>> 0, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => state.dtor(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__externref_drop_slice(ptr, len);
    return result;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getClampedArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ClampedArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

let cachedUint8ClampedArrayMemory0 = null;
function getUint8ClampedArrayMemory0() {
    if (cachedUint8ClampedArrayMemory0 === null || cachedUint8ClampedArrayMemory0.byteLength === 0) {
        cachedUint8ClampedArrayMemory0 = new Uint8ClampedArray(wasm.memory.buffer);
    }
    return cachedUint8ClampedArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, dtor, f) {
    const state = { a: arg0, b: arg1, cnt: 1, dtor };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            state.dtor(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    cachedUint8ClampedArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('oxigdal_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
