/* tslint:disable */
/* eslint-disable */

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
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns the number of bands
     */
    band_count(): number;
    /**
     * Centers the viewport on a point
     */
    centerOn(x: number, y: number): void;
    /**
     * Clears the tile cache
     */
    clearCache(): void;
    /**
     * Computes histogram for a region
     */
    computeHistogram(level: number, tile_x: number, tile_y: number): Promise<string>;
    /**
     * Computes image statistics for a region
     */
    computeStats(level: number, tile_x: number, tile_y: number): Promise<string>;
    /**
     * Returns the EPSG code if available
     */
    epsg_code(): number | undefined;
    /**
     * Fits the viewport to the image
     */
    fitToImage(): void;
    /**
     * Returns cache statistics as JSON
     */
    getCacheStats(): string | undefined;
    /**
     * Returns comprehensive metadata as JSON
     */
    getMetadata(): string;
    /**
     * Returns the current viewport as JSON
     */
    getViewport(): string | undefined;
    /**
     * Returns the image height
     */
    height(): bigint;
    /**
     * Creates a new advanced COG viewer
     */
    constructor();
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
     */
    open(url: string, cache_size_mb: number): Promise<void>;
    /**
     * Returns the number of overview levels
     */
    overview_count(): number;
    /**
     * Pans the viewport
     */
    pan(dx: number, dy: number): void;
    /**
     * Reads a tile as ImageData with caching
     */
    readTileAsImageData(level: number, tile_x: number, tile_y: number): Promise<ImageData>;
    /**
     * Reads a tile with caching
     */
    readTileCached(level: number, tile_x: number, tile_y: number): Promise<Uint8Array>;
    /**
     * Applies contrast enhancement to a tile
     */
    readTileWithContrast(level: number, tile_x: number, tile_y: number, method: string): Promise<ImageData>;
    /**
     * Sets the prefetch strategy
     */
    setPrefetchStrategy(strategy: string): void;
    /**
     * Sets the viewport size
     */
    setViewportSize(width: number, height: number): void;
    /**
     * Sets the zoom level
     */
    setZoom(zoom: number): void;
    /**
     * Returns the tile height
     */
    tile_height(): number;
    /**
     * Returns the tile width
     */
    tile_width(): number;
    /**
     * Returns the URL
     */
    url(): string | undefined;
    /**
     * Returns the image width
     */
    width(): bigint;
    /**
     * Zooms in
     */
    zoomIn(): void;
    /**
     * Zooms out
     */
    zoomOut(): void;
}

/**
 * Batch tile loader for efficient multi-tile loading
 */
export class BatchTileLoader {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Loads multiple tiles in parallel
     */
    loadTilesBatch(level: number, tile_coords: Uint32Array): Promise<any[]>;
    /**
     * Creates a new batch tile loader
     */
    constructor(max_parallel: number);
    /**
     * Opens a COG
     */
    open(url: string, cache_size_mb: number): Promise<void>;
}

/**
 * GeoJSON export utilities
 */
export class GeoJsonExporter {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Exports image bounds as GeoJSON
     */
    static exportBounds(west: number, south: number, east: number, north: number, epsg?: number | null): string;
    /**
     * Exports a point as GeoJSON
     */
    static exportPoint(x: number, y: number, properties: string): string;
}

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
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns the number of bands
     */
    band_count(): number;
    /**
     * Returns the EPSG code if available
     */
    epsg_code(): number | undefined;
    /**
     * Returns the image height
     */
    height(): bigint;
    /**
     * Returns metadata as JSON
     */
    metadata_json(): string;
    /**
     * Creates a new COG viewer
     */
    constructor();
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
     */
    open(url: string): Promise<void>;
    /**
     * Returns the number of overview levels
     */
    overview_count(): number;
    /**
     * Returns pixel scale X (degrees/pixel in lon direction)
     */
    pixel_scale_x(): number | undefined;
    /**
     * Returns pixel scale Y (degrees/pixel in lat direction, negative)
     */
    pixel_scale_y(): number | undefined;
    /**
     * Reads a tile and returns raw bytes
     */
    read_tile(_level: number, tile_x: number, tile_y: number): Promise<Uint8Array>;
    /**
     * Reads a tile and converts to RGBA ImageData for canvas rendering
     */
    read_tile_as_image_data(level: number, tile_x: number, tile_y: number): Promise<ImageData>;
    /**
     * Returns tiepoint geo X (top-left longitude)
     */
    tiepoint_geo_x(): number | undefined;
    /**
     * Returns tiepoint geo Y (top-left latitude)
     */
    tiepoint_geo_y(): number | undefined;
    /**
     * Returns the tile height
     */
    tile_height(): number;
    /**
     * Returns the tile width
     */
    tile_width(): number;
    /**
     * Returns the URL
     */
    url(): string | undefined;
    /**
     * Returns the image width
     */
    width(): bigint;
}

/**
 * WASM bindings for color operations
 */
export class WasmColorPalette {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Applies the palette to grayscale data
     */
    applyToGrayscale(data: Uint8Array): void;
    /**
     * Creates a plasma palette
     */
    static createPlasma(): WasmColorPalette;
    /**
     * Creates a terrain palette
     */
    static createTerrain(): WasmColorPalette;
    /**
     * Creates a viridis palette
     */
    static createViridis(): WasmColorPalette;
}

/**
 * WASM bindings for canvas operations
 */
export class WasmImageProcessor {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Computes histogram as JSON
     */
    static computeHistogram(data: Uint8Array, width: number, height: number): string;
    /**
     * Computes statistics as JSON
     */
    static computeStats(data: Uint8Array, width: number, height: number): string;
    /**
     * Creates ImageData from RGBA bytes
     */
    static createImageData(data: Uint8Array, width: number, height: number): ImageData;
    /**
     * Applies histogram equalization
     */
    static histogramEqualization(data: Uint8Array, width: number, height: number): void;
    /**
     * Applies linear stretch
     */
    static linearStretch(data: Uint8Array, width: number, height: number): void;
}

/**
 * WASM bindings for profiler
 */
export class WasmProfiler {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns all statistics as JSON
     */
    getAllStats(): string;
    /**
     * Returns counter statistics as JSON
     */
    getCounterStats(name: string): string | undefined;
    /**
     * Creates a new profiler
     */
    constructor();
    /**
     * Records a timing sample
     */
    record(name: string, duration_ms: number): void;
    /**
     * Records current memory usage
     */
    recordMemory(): void;
    /**
     * Resets all counters
     */
    reset(): void;
    /**
     * Starts a timer
     */
    startTimer(name: string): void;
    /**
     * Stops a timer
     */
    stopTimer(name: string): void;
}

/**
 * WASM bindings for tile management
 */
export class WasmTileCache {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Clears the cache
     */
    clear(): void;
    /**
     * Gets cache statistics as JSON
     */
    getStats(): string;
    /**
     * Returns the cache hit rate
     */
    hitRate(): number;
    /**
     * Creates a new tile cache
     */
    constructor(max_size_mb: number);
}

/**
 * WASM bindings for worker pool (for demonstration/testing)
 */
export class WasmWorkerPool {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns a message about worker support
     */
    getInfo(): string;
    /**
     * Creates a new worker pool
     */
    constructor(pool_size: number);
    /**
     * Returns the pool size
     */
    poolSize(): number;
}

/**
 * Initialize the WASM module with better error handling
 */
export function init(): void;

/**
 * Checks if the given URL points to a TIFF file by reading the header
 *
 * # Errors
 * Returns an error if the URL cannot be fetched or the header cannot be read
 */
export function is_tiff_url(url: string): Promise<boolean>;

/**
 * Version information
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_advancedcogviewer_free: (a: number, b: number) => void;
    readonly __wbg_batchtileloader_free: (a: number, b: number) => void;
    readonly __wbg_geojsonexporter_free: (a: number, b: number) => void;
    readonly __wbg_wasmcogviewer_free: (a: number, b: number) => void;
    readonly __wbg_wasmcolorpalette_free: (a: number, b: number) => void;
    readonly __wbg_wasmprofiler_free: (a: number, b: number) => void;
    readonly __wbg_wasmtilecache_free: (a: number, b: number) => void;
    readonly __wbg_wasmworkerpool_free: (a: number, b: number) => void;
    readonly advancedcogviewer_band_count: (a: number) => number;
    readonly advancedcogviewer_centerOn: (a: number, b: number, c: number) => void;
    readonly advancedcogviewer_clearCache: (a: number) => void;
    readonly advancedcogviewer_computeHistogram: (a: number, b: number, c: number, d: number) => any;
    readonly advancedcogviewer_computeStats: (a: number, b: number, c: number, d: number) => any;
    readonly advancedcogviewer_epsg_code: (a: number) => number;
    readonly advancedcogviewer_fitToImage: (a: number) => void;
    readonly advancedcogviewer_getCacheStats: (a: number) => [number, number];
    readonly advancedcogviewer_getMetadata: (a: number) => [number, number];
    readonly advancedcogviewer_getViewport: (a: number) => [number, number];
    readonly advancedcogviewer_height: (a: number) => bigint;
    readonly advancedcogviewer_new: () => number;
    readonly advancedcogviewer_open: (a: number, b: number, c: number, d: number) => any;
    readonly advancedcogviewer_overview_count: (a: number) => number;
    readonly advancedcogviewer_pan: (a: number, b: number, c: number) => void;
    readonly advancedcogviewer_readTileAsImageData: (a: number, b: number, c: number, d: number) => any;
    readonly advancedcogviewer_readTileCached: (a: number, b: number, c: number, d: number) => any;
    readonly advancedcogviewer_readTileWithContrast: (a: number, b: number, c: number, d: number, e: number, f: number) => any;
    readonly advancedcogviewer_setPrefetchStrategy: (a: number, b: number, c: number) => void;
    readonly advancedcogviewer_setViewportSize: (a: number, b: number, c: number) => void;
    readonly advancedcogviewer_setZoom: (a: number, b: number) => void;
    readonly advancedcogviewer_tile_height: (a: number) => number;
    readonly advancedcogviewer_tile_width: (a: number) => number;
    readonly advancedcogviewer_url: (a: number) => [number, number];
    readonly advancedcogviewer_width: (a: number) => bigint;
    readonly advancedcogviewer_zoomIn: (a: number) => void;
    readonly advancedcogviewer_zoomOut: (a: number) => void;
    readonly batchtileloader_loadTilesBatch: (a: number, b: number, c: number, d: number) => any;
    readonly batchtileloader_new: (a: number) => number;
    readonly batchtileloader_open: (a: number, b: number, c: number, d: number) => any;
    readonly geojsonexporter_exportBounds: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly geojsonexporter_exportPoint: (a: number, b: number, c: number, d: number) => [number, number];
    readonly is_tiff_url: (a: number, b: number) => any;
    readonly version: () => [number, number];
    readonly wasmcogviewer_band_count: (a: number) => number;
    readonly wasmcogviewer_epsg_code: (a: number) => number;
    readonly wasmcogviewer_height: (a: number) => bigint;
    readonly wasmcogviewer_metadata_json: (a: number) => [number, number];
    readonly wasmcogviewer_new: () => number;
    readonly wasmcogviewer_open: (a: number, b: number, c: number) => any;
    readonly wasmcogviewer_overview_count: (a: number) => number;
    readonly wasmcogviewer_pixel_scale_x: (a: number) => [number, number];
    readonly wasmcogviewer_pixel_scale_y: (a: number) => [number, number];
    readonly wasmcogviewer_read_tile: (a: number, b: number, c: number, d: number) => any;
    readonly wasmcogviewer_read_tile_as_image_data: (a: number, b: number, c: number, d: number) => any;
    readonly wasmcogviewer_tiepoint_geo_x: (a: number) => [number, number];
    readonly wasmcogviewer_tiepoint_geo_y: (a: number) => [number, number];
    readonly wasmcogviewer_tile_height: (a: number) => number;
    readonly wasmcogviewer_tile_width: (a: number) => number;
    readonly wasmcogviewer_url: (a: number) => [number, number];
    readonly wasmcogviewer_width: (a: number) => bigint;
    readonly wasmcolorpalette_applyToGrayscale: (a: number, b: number, c: number, d: any) => [number, number];
    readonly wasmcolorpalette_createPlasma: () => number;
    readonly wasmcolorpalette_createTerrain: () => number;
    readonly wasmcolorpalette_createViridis: () => number;
    readonly wasmimageprocessor_computeHistogram: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly wasmimageprocessor_computeStats: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly wasmimageprocessor_createImageData: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly wasmimageprocessor_histogramEqualization: (a: number, b: number, c: any, d: number, e: number) => [number, number];
    readonly wasmimageprocessor_linearStretch: (a: number, b: number, c: any, d: number, e: number) => [number, number];
    readonly wasmprofiler_getAllStats: (a: number) => [number, number];
    readonly wasmprofiler_getCounterStats: (a: number, b: number, c: number) => [number, number];
    readonly wasmprofiler_new: () => number;
    readonly wasmprofiler_record: (a: number, b: number, c: number, d: number) => void;
    readonly wasmprofiler_recordMemory: (a: number) => void;
    readonly wasmprofiler_reset: (a: number) => void;
    readonly wasmprofiler_startTimer: (a: number, b: number, c: number) => void;
    readonly wasmprofiler_stopTimer: (a: number, b: number, c: number) => void;
    readonly wasmtilecache_clear: (a: number) => void;
    readonly wasmtilecache_getStats: (a: number) => [number, number];
    readonly wasmtilecache_hitRate: (a: number) => number;
    readonly wasmtilecache_new: (a: number) => number;
    readonly wasmworkerpool_getInfo: (a: number) => [number, number];
    readonly wasmworkerpool_poolSize: (a: number) => number;
    readonly init: () => void;
    readonly wasmworkerpool_new: (a: number) => number;
    readonly __wbg_wasmimageprocessor_free: (a: number, b: number) => void;
    readonly wasm_bindgen_2fd503ddb8ec2886___closure__destroy___dyn_core_eda1fd0e4a34c130___ops__function__FnMut__wasm_bindgen_2fd503ddb8ec2886___JsValue____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue__wasm_bindgen_2fd503ddb8ec2886___JsValue_____: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_2fd503ddb8ec2886___convert__closures_____invoke___wasm_bindgen_2fd503ddb8ec2886___JsValue_____: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_drop_slice: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
