//! # OxiGDAL WASM - WebAssembly Bindings for Browser-based Geospatial Processing
//!
//! This crate provides comprehensive WebAssembly bindings for OxiGDAL, enabling
//! high-performance browser-based geospatial data processing with a focus on
//! Cloud Optimized GeoTIFF (COG) visualization and manipulation.
//!
//! ## Features
//!
//! ### Core Capabilities
//! - **COG Viewing**: Efficient viewing of Cloud Optimized GeoTIFFs
//! - **Tile Management**: Advanced tile caching and pyramid management
//! - **Progressive Rendering**: Smooth progressive loading with adaptive quality
//! - **Image Processing**: Color manipulation, contrast enhancement, filters
//! - **Performance Profiling**: Built-in profiling and bottleneck detection
//! - **Worker Pool**: Parallel tile loading using Web Workers
//! - **Streaming**: Adaptive tile streaming with bandwidth estimation
//!
//! ### Advanced Features
//! - **Compression**: Multiple compression algorithms for bandwidth reduction
//! - **Color Operations**: Extensive color space conversions and palettes
//! - **TypeScript Bindings**: Auto-generated TypeScript definitions
//! - **Error Handling**: Comprehensive error types and recovery
//! - **Viewport Management**: Advanced viewport transformations and history
//!
//! ## Architecture
//!
//! The crate is organized into several modules:
//!
//! - `bindings`: TypeScript type definitions and documentation generation
//! - `canvas`: Image processing, resampling, and canvas rendering utilities
//! - `color`: Advanced color manipulation, palettes, and color correction
//! - `compression`: Tile compression algorithms (RLE, Delta, Huffman, LZ77)
//! - `error`: Comprehensive error types for all operations
//! - `fetch`: HTTP fetching with retry logic and parallel requests
//! - `profiler`: Performance profiling and bottleneck detection
//! - `rendering`: Canvas rendering, double buffering, and progressive rendering
//! - `streaming`: Adaptive tile streaming with bandwidth management
//! - `tile`: Tile coordinate systems, caching, and pyramid management
//! - `worker`: Web Worker pool for parallel processing
//!
//! ## Basic Usage Example (JavaScript)
//!
//! ```javascript
//! import init, { WasmCogViewer } from 'oxigdal-wasm';
//!
//! async function viewCog(url) {
//!     // Initialize the WASM module
//!     await init();
//!
//!     // Create a viewer instance
//!     const viewer = new WasmCogViewer();
//!
//!     // Open a COG file
//!     await viewer.open(url);
//!
//!     // Get image metadata
//!     console.log(`Image size: ${viewer.width()}x${viewer.height()}`);
//!     console.log(`Tile size: ${viewer.tile_width()}x${viewer.tile_height()}`);
//!     console.log(`Bands: ${viewer.band_count()}`);
//!     console.log(`Overviews: ${viewer.overview_count()}`);
//!
//!     // Read a tile as ImageData for canvas rendering
//!     const imageData = await viewer.read_tile_as_image_data(0, 0, 0);
//!
//!     // Render to canvas
//!     const canvas = document.getElementById('map-canvas');
//!     const ctx = canvas.getContext('2d');
//!     ctx.putImageData(imageData, 0, 0);
//! }
//! ```
//!
//! ## Advanced Usage Example (JavaScript)
//!
//! ```javascript
//! import init, {
//!     AdvancedCogViewer,
//!     WasmImageProcessor,
//!     WasmColorPalette,
//!     WasmProfiler,
//!     WasmTileCache
//! } from 'oxigdal-wasm';
//!
//! async function advancedProcessing() {
//!     await init();
//!
//!     // Create an advanced viewer with caching
//!     const viewer = new AdvancedCogViewer();
//!     await viewer.open('https://example.com/image.tif', 100); // 100MB cache
//!
//!     // Setup profiling
//!     const profiler = new WasmProfiler();
//!     profiler.startTimer('tile_load');
//!
//!     // Load and process a tile
//!     const imageData = await viewer.readTileAsImageData(0, 0, 0);
//!     profiler.stopTimer('tile_load');
//!
//!     // Apply color palette
//!     const palette = WasmColorPalette.createViridis();
//!     const imageBytes = new Uint8Array(imageData.data.buffer);
//!     palette.applyToGrayscale(imageBytes);
//!
//!     // Apply image processing
//!     WasmImageProcessor.linearStretch(imageBytes, imageData.width, imageData.height);
//!
//!     // Get cache statistics
//!     const cacheStats = viewer.getCacheStats();
//!     console.log('Cache hit rate:', JSON.parse(cacheStats).hit_count);
//!
//!     // Get profiling statistics
//!     const profStats = profiler.getAllStats();
//!     console.log('Performance:', profStats);
//! }
//! ```
//!
//! ## Progressive Loading Example (JavaScript)
//!
//! ```javascript
//! async function progressiveLoad(url, canvas) {
//!     const viewer = new AdvancedCogViewer();
//!     await viewer.open(url, 100);
//!
//!     // Start with low quality for quick feedback
//!     viewer.setViewportSize(canvas.width, canvas.height);
//!     viewer.fitToImage();
//!
//!     const ctx = canvas.getContext('2d');
//!
//!     // Load visible tiles progressively
//!     const viewport = JSON.parse(viewer.getViewport());
//!     for (let level = viewer.overview_count(); level >= 0; level--) {
//!         // Load tiles at this level
//!         const imageData = await viewer.readTileAsImageData(level, 0, 0);
//!         ctx.putImageData(imageData, 0, 0);
//!
//!         // Allow UI updates
//!         await new Promise(resolve => setTimeout(resolve, 0));
//!     }
//! }
//! ```
//!
//! ## Performance Considerations
//!
//! ### Memory Management
//! - The tile cache automatically evicts old tiles using LRU strategy
//! - Configure cache size based on available memory
//! - Use compression to reduce memory footprint
//!
//! ### Network Optimization
//! - HTTP range requests are used for partial file reads
//! - Retry logic handles network failures gracefully
//! - Parallel requests improve throughput
//! - Adaptive streaming adjusts quality based on bandwidth
//!
//! ### Rendering Performance
//! - Double buffering prevents flickering
//! - Progressive rendering provides quick feedback
//! - Web Workers enable parallel tile processing
//! - Canvas operations are optimized for WASM
//!
//! ## Error Handling
//!
//! All operations return `Result` types that can be converted to JavaScript
//! exceptions. Errors are categorized by type:
//!
//! - `FetchError`: Network and HTTP errors
//! - `CanvasError`: Canvas and rendering errors
//! - `WorkerError`: Web Worker errors
//! - `TileCacheError`: Cache management errors
//! - `JsInteropError`: JavaScript interop errors
//!
//! ```javascript
//! try {
//!     await viewer.open(url);
//! } catch (error) {
//!     if (error.message.includes('HTTP 404')) {
//!         console.error('File not found');
//!     } else if (error.message.includes('CORS')) {
//!         console.error('Cross-origin request blocked');
//!     } else {
//!         console.error('Unknown error:', error);
//!     }
//! }
//! ```
//!
//! ## Browser Compatibility
//!
//! This crate requires:
//! - WebAssembly support
//! - Fetch API with range request support
//! - Canvas API
//! - Web Workers (optional, for parallel processing)
//! - Performance API (optional, for profiling)
//!
//! Supported browsers:
//! - Chrome 57+
//! - Firefox 52+
//! - Safari 11+
//! - Edge 16+
//!
//! ## Building for Production
//!
//! ```bash
//! # Optimize for size
//! wasm-pack build --target web --release -- --features optimize-size
//!
//! # Optimize for speed
//! wasm-pack build --target web --release -- --features optimize-speed
//!
//! # Generate TypeScript definitions
//! wasm-pack build --target bundler --release
//! ```
//!
//! ## License
//!
//! This crate is part of the OxiGDAL project and follows the same licensing terms.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
// WASM crate allows - for internal implementation patterns
#![allow(clippy::needless_range_loop)]
#![allow(clippy::expect_used)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::new_without_default)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::type_complexity)]

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use web_sys::{ImageData, console};

use oxigdal_core::error::OxiGdalError;
use oxigdal_core::io::ByteRange;

mod animation;
mod bindings;
mod canvas;
mod cog_reader;
mod color;
mod compression;
mod error;
mod fetch;
mod profiler;
mod rendering;
mod streaming;
#[cfg(test)]
mod tests;
mod tile;
mod worker;

pub use animation::{
    Animation, Easing, EasingFunction, PanAnimation, SpringAnimation, ZoomAnimation,
};
pub use bindings::{
    DocGenerator, TsClass, TsFunction, TsInterface, TsModule, TsParameter, TsType, TsTypeAlias,
    create_oxigdal_wasm_docs,
};
pub use canvas::{
    ChannelHistogramJson, ContrastMethod, CustomBinHistogramJson, Histogram, HistogramJson, Hsv,
    ImageProcessor, ImageStats, ResampleMethod, Resampler, Rgb, WasmImageProcessor, YCbCr,
};
pub use color::{
    ChannelOps, ColorCorrectionMatrix, ColorPalette, ColorQuantizer, ColorTemperature,
    GradientGenerator, PaletteEntry, WasmColorPalette, WhiteBalance,
};
pub use compression::{
    CompressionAlgorithm, CompressionBenchmark, CompressionSelector, CompressionStats,
    DeltaCompressor, HuffmanCompressor, Lz77Compressor, RleCompressor, TileCompressor,
};
pub use error::{
    CanvasError, FetchError, JsInteropError, TileCacheError, WasmError, WasmResult, WorkerError,
};
pub use fetch::{
    EnhancedFetchBackend, FetchBackend, FetchStats, PrioritizedRequest, RequestPriority,
    RequestQueue, RetryConfig,
};
pub use profiler::{
    Bottleneck, BottleneckDetector, CounterStats, FrameRateStats, FrameRateTracker, MemoryMonitor,
    MemorySnapshot, MemoryStats, PerformanceCounter, Profiler, ProfilerSummary, WasmProfiler,
};
pub use rendering::{
    AnimationManager, AnimationStats, CanvasBuffer, CanvasRenderer, ProgressiveRenderStats,
    ProgressiveRenderer, RenderQuality, ViewportHistory, ViewportState, ViewportTransform,
};
pub use streaming::{
    BandwidthEstimator, ImportanceCalculator, LoadStrategy, MultiResolutionStreamer,
    PrefetchScheduler, ProgressiveLoader, QualityAdapter, StreamBuffer, StreamBufferStats,
    StreamingQuality, StreamingStats, TileStreamer,
};
pub use tile::{
    CacheStats, CachedTile, PrefetchStrategy, TileBounds, TileCache, TileCoord, TilePrefetcher,
    TilePyramid, WasmTileCache,
};
pub use worker::{
    JobId, JobStatus, PendingJob, PoolStats, WasmWorkerPool, WorkerInfo, WorkerJobRequest,
    WorkerJobResponse, WorkerPool, WorkerRequestType, WorkerResponseType,
};

/// Initialize the WASM module with better error handling
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// WASM-compatible COG (Cloud Optimized GeoTIFF) viewer
///
/// This is the basic COG viewer for browser-based geospatial data visualization.
/// It provides simple access to COG metadata and tile reading functionality.
///
/// # Features
///
/// - Efficient tile-based access to large GeoTIFF files
/// - Support for multi-band imagery
/// - Overview/pyramid level access for different zoom levels
/// - CORS-compatible HTTP range request support
/// - Automatic TIFF header parsing
/// - GeoTIFF metadata extraction (CRS, geotransform, etc.)
///
/// # Performance
///
/// The viewer uses HTTP range requests to fetch only the required portions
/// of the file, making it efficient for large files. However, for production
/// use cases with caching and advanced features, consider using
/// `AdvancedCogViewer` instead.
///
/// # Example
///
/// ```javascript
/// const viewer = new WasmCogViewer();
/// await viewer.open('<https://example.com/image.tif>');
/// console.log(`Size: ${viewer.width()}x${viewer.height()}`);
/// const tile = await viewer.read_tile_as_image_data(0, 0, 0);
/// ```
#[wasm_bindgen]
pub struct WasmCogViewer {
    /// URL of the opened COG file
    url: Option<String>,
    /// Image width in pixels
    width: u64,
    /// Image height in pixels
    height: u64,
    /// Tile width in pixels (typically 256 or 512)
    tile_width: u32,
    /// Tile height in pixels (typically 256 or 512)
    tile_height: u32,
    /// Number of bands/channels in the image
    band_count: u32,
    /// Number of overview/pyramid levels available
    overview_count: usize,
    /// EPSG code for the coordinate reference system (if available)
    epsg_code: Option<u32>,
    /// GeoTIFF geotransform data (for calculating geographic bounds)
    pixel_scale_x: Option<f64>,
    pixel_scale_y: Option<f64>,
    tiepoint_pixel_x: Option<f64>,
    tiepoint_pixel_y: Option<f64>,
    tiepoint_geo_x: Option<f64>,
    tiepoint_geo_y: Option<f64>,
}

#[wasm_bindgen]
impl WasmCogViewer {
    /// Creates a new COG viewer
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            url: None,
            width: 0,
            height: 0,
            tile_width: 256,
            tile_height: 256,
            band_count: 0,
            overview_count: 0,
            epsg_code: None,
            pixel_scale_x: None,
            pixel_scale_y: None,
            tiepoint_pixel_x: None,
            tiepoint_pixel_y: None,
            tiepoint_geo_x: None,
            tiepoint_geo_y: None,
        }
    }

    /// Opens a COG file from a URL
    ///
    /// This method performs the following operations:
    /// 1. Sends a HEAD request to determine file size and range support
    /// 2. Fetches the TIFF header to validate format
    /// 3. Parses IFD (Image File Directory) to extract metadata
    /// 4. Extracts GeoTIFF tags for coordinate system information
    /// 5. Counts overview levels for multi-resolution support
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the COG file to open. Must support HTTP range requests
    ///           for optimal performance. CORS must be properly configured.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a JavaScript error on failure.
    ///
    /// # Errors
    ///
    /// This method can fail for several reasons:
    /// - Network errors (no connection, timeout, etc.)
    /// - HTTP errors (404, 403, 500, etc.)
    /// - CORS errors (missing headers)
    /// - Invalid TIFF format
    /// - Unsupported TIFF variant
    ///
    /// # Example
    ///
    /// ```javascript
    /// const viewer = new WasmCogViewer();
    /// try {
    ///     await viewer.open('<https://example.com/landsat.tif>');
    ///     console.log('Successfully opened COG');
    /// } catch (error) {
    ///     console.error('Failed to open:', error);
    /// }
    /// ```
    #[wasm_bindgen]
    pub async fn open(&mut self, url: &str) -> std::result::Result<(), JsValue> {
        // Log the operation for debugging
        console::log_1(&format!("Opening COG: {}", url).into());

        // Use WASM-specific async COG reader
        let reader = cog_reader::WasmCogReader::open(url.to_string())
            .await
            .map_err(|e| to_js_error(&e))?;

        let metadata = reader.metadata();

        self.url = Some(url.to_string());
        self.width = metadata.width;
        self.height = metadata.height;
        self.tile_width = metadata.tile_width;
        self.tile_height = metadata.tile_height;
        self.band_count = u32::from(metadata.samples_per_pixel);
        self.overview_count = metadata.overview_count;
        self.epsg_code = metadata.epsg_code;

        // Extract geotransform for bounds calculation
        self.pixel_scale_x = metadata.pixel_scale_x;
        self.pixel_scale_y = metadata.pixel_scale_y;
        self.tiepoint_pixel_x = metadata.tiepoint_pixel_x;
        self.tiepoint_pixel_y = metadata.tiepoint_pixel_y;
        self.tiepoint_geo_x = metadata.tiepoint_geo_x;
        self.tiepoint_geo_y = metadata.tiepoint_geo_y;

        console::log_1(
            &format!(
                "Opened COG: {}x{}, {} bands, {} overviews",
                self.width, self.height, self.band_count, self.overview_count
            )
            .into(),
        );

        Ok(())
    }

    /// Returns the image width
    #[wasm_bindgen]
    pub fn width(&self) -> u64 {
        self.width
    }

    /// Returns the image height
    #[wasm_bindgen]
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Returns the tile width
    #[wasm_bindgen]
    pub fn tile_width(&self) -> u32 {
        self.tile_width
    }

    /// Returns the tile height
    #[wasm_bindgen]
    pub fn tile_height(&self) -> u32 {
        self.tile_height
    }

    /// Returns the number of bands
    #[wasm_bindgen]
    pub fn band_count(&self) -> u32 {
        self.band_count
    }

    /// Returns the number of overview levels
    #[wasm_bindgen]
    pub fn overview_count(&self) -> usize {
        self.overview_count
    }

    /// Returns the EPSG code if available
    #[wasm_bindgen]
    pub fn epsg_code(&self) -> Option<u32> {
        self.epsg_code
    }

    /// Returns the URL
    #[wasm_bindgen]
    pub fn url(&self) -> Option<String> {
        self.url.clone()
    }

    /// Returns metadata as JSON
    #[wasm_bindgen]
    pub fn metadata_json(&self) -> String {
        serde_json::json!({
            "url": self.url,
            "width": self.width,
            "height": self.height,
            "tileWidth": self.tile_width,
            "tileHeight": self.tile_height,
            "bandCount": self.band_count,
            "overviewCount": self.overview_count,
            "epsgCode": self.epsg_code,
            "geotransform": {
                "pixelScaleX": self.pixel_scale_x,
                "pixelScaleY": self.pixel_scale_y,
                "tiepointPixelX": self.tiepoint_pixel_x,
                "tiepointPixelY": self.tiepoint_pixel_y,
                "tiepointGeoX": self.tiepoint_geo_x,
                "tiepointGeoY": self.tiepoint_geo_y,
            },
        })
        .to_string()
    }

    /// Returns pixel scale X (degrees/pixel in lon direction)
    #[wasm_bindgen]
    pub fn pixel_scale_x(&self) -> Option<f64> {
        self.pixel_scale_x
    }

    /// Returns pixel scale Y (degrees/pixel in lat direction, negative)
    #[wasm_bindgen]
    pub fn pixel_scale_y(&self) -> Option<f64> {
        self.pixel_scale_y
    }

    /// Returns tiepoint geo X (top-left longitude)
    #[wasm_bindgen]
    pub fn tiepoint_geo_x(&self) -> Option<f64> {
        self.tiepoint_geo_x
    }

    /// Returns tiepoint geo Y (top-left latitude)
    #[wasm_bindgen]
    pub fn tiepoint_geo_y(&self) -> Option<f64> {
        self.tiepoint_geo_y
    }

    /// Reads a tile and returns raw bytes
    #[wasm_bindgen]
    pub async fn read_tile(
        &self,
        _level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> std::result::Result<Vec<u8>, JsValue> {
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;

        // Create WASM-specific async COG reader
        let reader = cog_reader::WasmCogReader::open(url.clone())
            .await
            .map_err(|e| to_js_error(&e))?;

        // Read tile with async I/O
        reader
            .read_tile(tile_x, tile_y)
            .await
            .map_err(|e| to_js_error(&e))
    }

    /// Reads a tile and converts to RGBA ImageData for canvas rendering
    #[wasm_bindgen]
    pub async fn read_tile_as_image_data(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> std::result::Result<ImageData, JsValue> {
        let tile_data = self.read_tile(level, tile_x, tile_y).await?;

        let pixel_count = (self.tile_width * self.tile_height) as usize;
        let mut rgba = vec![0u8; pixel_count * 4];

        // Convert to RGBA based on band count
        match self.band_count {
            1 => {
                // Grayscale
                for (i, &v) in tile_data.iter().take(pixel_count).enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                    rgba[i * 4 + 3] = 255;
                }
            }
            3 => {
                // RGB
                for i in 0..pixel_count.min(tile_data.len() / 3) {
                    rgba[i * 4] = tile_data[i * 3];
                    rgba[i * 4 + 1] = tile_data[i * 3 + 1];
                    rgba[i * 4 + 2] = tile_data[i * 3 + 2];
                    rgba[i * 4 + 3] = 255;
                }
            }
            4 => {
                // RGBA
                for i in 0..pixel_count.min(tile_data.len() / 4) {
                    rgba[i * 4] = tile_data[i * 4];
                    rgba[i * 4 + 1] = tile_data[i * 4 + 1];
                    rgba[i * 4 + 2] = tile_data[i * 4 + 2];
                    rgba[i * 4 + 3] = tile_data[i * 4 + 3];
                }
            }
            _ => {
                // Use first band as grayscale
                for (i, &v) in tile_data.iter().take(pixel_count).enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                    rgba[i * 4 + 3] = 255;
                }
            }
        }

        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, self.tile_width, self.tile_height)
    }
}

impl Default for WasmCogViewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts an `OxiGdalError` to a `JsValue`
fn to_js_error(err: &OxiGdalError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

/// Version information
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Checks if the given URL points to a TIFF file by reading the header
///
/// # Errors
/// Returns an error if the URL cannot be fetched or the header cannot be read
#[wasm_bindgen]
pub async fn is_tiff_url(url: &str) -> std::result::Result<bool, JsValue> {
    let backend = FetchBackend::new(url.to_string())
        .await
        .map_err(|e| to_js_error(&e))?;
    let header = backend
        .read_range_async(ByteRange::from_offset_length(0, 8))
        .await
        .map_err(|e| to_js_error(&e))?;
    Ok(oxigdal_geotiff::is_tiff(&header))
}

/// Viewport for managing the visible area of the image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    /// Center X coordinate in image space
    pub center_x: f64,
    /// Center Y coordinate in image space
    pub center_y: f64,
    /// Zoom level (0 = most zoomed out)
    pub zoom: u32,
    /// Viewport width in pixels
    pub width: u32,
    /// Viewport height in pixels
    pub height: u32,
}

impl Viewport {
    /// Creates a new viewport
    pub const fn new(center_x: f64, center_y: f64, zoom: u32, width: u32, height: u32) -> Self {
        Self {
            center_x,
            center_y,
            zoom,
            width,
            height,
        }
    }

    /// Returns the visible bounds in image coordinates
    pub const fn bounds(&self) -> (f64, f64, f64, f64) {
        let half_width = (self.width as f64) / 2.0;
        let half_height = (self.height as f64) / 2.0;

        let min_x = self.center_x - half_width;
        let min_y = self.center_y - half_height;
        let max_x = self.center_x + half_width;
        let max_y = self.center_y + half_height;

        (min_x, min_y, max_x, max_y)
    }

    /// Pans the viewport by the given delta
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.center_x += dx;
        self.center_y += dy;
    }

    /// Zooms in (increases zoom level)
    pub fn zoom_in(&mut self) {
        self.zoom = self.zoom.saturating_add(1);
    }

    /// Zooms out (decreases zoom level)
    pub fn zoom_out(&mut self) {
        self.zoom = self.zoom.saturating_sub(1);
    }

    /// Sets the zoom level
    pub fn set_zoom(&mut self, zoom: u32) {
        self.zoom = zoom;
    }

    /// Centers the viewport on a point
    pub fn center_on(&mut self, x: f64, y: f64) {
        self.center_x = x;
        self.center_y = y;
    }

    /// Fits the viewport to the given image size
    pub fn fit_to_image(&mut self, image_width: u64, image_height: u64) {
        self.center_x = (image_width as f64) / 2.0;
        self.center_y = (image_height as f64) / 2.0;

        // Calculate zoom level to fit image
        let x_scale = (image_width as f64) / (self.width as f64);
        let y_scale = (image_height as f64) / (self.height as f64);
        let scale = x_scale.max(y_scale);

        self.zoom = scale.log2().ceil() as u32;
    }
}

/// Advanced COG viewer with comprehensive tile management and caching
///
/// This is the recommended viewer for production applications. It provides
/// advanced features including:
///
/// - **LRU Tile Caching**: Automatic memory management with configurable size
/// - **Viewport Management**: Pan, zoom, and viewport history (undo/redo)
/// - **Prefetching**: Intelligent prefetching of nearby tiles
/// - **Multi-resolution**: Automatic selection of appropriate overview level
/// - **Image Processing**: Built-in contrast enhancement and statistics
/// - **Performance Tracking**: Cache hit rates and loading metrics
///
/// # Memory Management
///
/// The viewer uses an LRU (Least Recently Used) cache to manage memory
/// efficiently. When the cache is full, the least recently accessed tiles
/// are evicted. Configure the cache size based on your application's memory
/// constraints and typical usage patterns.
///
/// Recommended cache sizes:
/// - Mobile devices: 50-100 MB
/// - Desktop browsers: 100-500 MB
/// - High-end workstations: 500-1000 MB
///
/// # Prefetching Strategies
///
/// The viewer supports multiple prefetching strategies:
///
/// - **None**: No prefetching (lowest memory, highest latency)
/// - **Neighbors**: Prefetch immediately adjacent tiles
/// - **Pyramid**: Prefetch parent and child tiles (smooth zooming)
///
/// # Performance Optimization
///
/// For best performance:
/// 1. Use an appropriate cache size (100-200 MB recommended)
/// 2. Enable prefetching for smoother user experience
/// 3. Use viewport management to minimize unnecessary tile loads
/// 4. Monitor cache statistics to tune parameters
///
/// # Example
///
/// ```javascript
/// const viewer = new AdvancedCogViewer();
/// await viewer.open('<https://example.com/image.tif>', 100); // 100MB cache
///
/// // Setup viewport
/// viewer.setViewportSize(800, 600);
/// viewer.fitToImage();
///
/// // Enable prefetching
/// viewer.setPrefetchStrategy('neighbors');
///
/// // Load and display tiles
/// const imageData = await viewer.readTileAsImageData(0, 0, 0);
/// ctx.putImageData(imageData, 0, 0);
///
/// // Check performance
/// const stats = JSON.parse(viewer.getCacheStats());
/// console.log(`Hit rate: ${stats.hit_count / (stats.hit_count + stats.miss_count)}`);
/// ```
#[wasm_bindgen]
pub struct AdvancedCogViewer {
    /// URL of the opened COG file
    url: Option<String>,

    /// Image metadata - width in pixels
    width: u64,
    /// Image metadata - height in pixels
    height: u64,
    /// Tile dimensions - width in pixels
    tile_width: u32,
    /// Tile dimensions - height in pixels
    tile_height: u32,
    /// Number of spectral bands in the image
    band_count: u32,
    /// Number of overview/pyramid levels
    overview_count: usize,
    /// EPSG code for coordinate reference system
    epsg_code: Option<u32>,

    /// Tile pyramid structure for multi-resolution access
    pyramid: Option<TilePyramid>,
    /// LRU tile cache for efficient memory management
    cache: Option<TileCache>,
    /// Current viewport state (pan, zoom, bounds)
    viewport: Option<Viewport>,
    /// Strategy for prefetching nearby tiles
    prefetch_strategy: PrefetchStrategy,
}

#[wasm_bindgen]
impl AdvancedCogViewer {
    /// Creates a new advanced COG viewer
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            url: None,
            width: 0,
            height: 0,
            tile_width: 256,
            tile_height: 256,
            band_count: 0,
            overview_count: 0,
            epsg_code: None,
            pyramid: None,
            cache: None,
            viewport: None,
            prefetch_strategy: PrefetchStrategy::Neighbors,
        }
    }

    /// Opens a COG file from a URL with advanced caching enabled
    ///
    /// This method initializes the viewer with full caching and viewport management.
    /// It performs the following operations:
    ///
    /// 1. **Initial Connection**: Sends HEAD request to validate URL and check range support
    /// 2. **Header Parsing**: Fetches and parses TIFF header (8-16 bytes)
    /// 3. **Metadata Extraction**: Parses IFD to extract image dimensions, tile size, bands
    /// 4. **GeoTIFF Tags**: Extracts coordinate system information (EPSG, geotransform)
    /// 5. **Pyramid Creation**: Builds tile pyramid structure for all overview levels
    /// 6. **Cache Initialization**: Creates LRU cache with specified size
    /// 7. **Viewport Setup**: Initializes viewport with default settings
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the COG file. Must support HTTP range requests (Accept-Ranges: bytes)
    ///           and have proper CORS headers configured.
    /// * `cache_size_mb` - Size of the tile cache in megabytes. Recommended values:
    ///   - Mobile: 50-100 MB
    ///   - Desktop: 100-500 MB
    ///   - High-end: 500-1000 MB
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful initialization, or a JavaScript error on failure.
    ///
    /// # Errors
    ///
    /// This method can fail for several reasons:
    ///
    /// ## Network Errors
    /// - Connection timeout
    /// - DNS resolution failure
    /// - SSL/TLS errors
    ///
    /// ## HTTP Errors
    /// - 404 Not Found: File doesn't exist at the URL
    /// - 403 Forbidden: Access denied
    /// - 500 Server Error: Server-side issues
    ///
    /// ## CORS Errors
    /// - Missing Access-Control-Allow-Origin header
    /// - Missing Access-Control-Allow-Headers for range requests
    ///
    /// ## Format Errors
    /// - Invalid TIFF magic bytes
    /// - Corrupted IFD structure
    /// - Unsupported TIFF variant
    /// - Missing required tags
    ///
    /// # Performance Considerations
    ///
    /// Opening a COG typically requires 2-4 HTTP requests:
    /// 1. HEAD request (~10ms)
    /// 2. Header fetch (~20ms for 16 bytes)
    /// 3. IFD fetch (~50ms for typical IFD)
    /// 4. GeoTIFF tags fetch (~30ms if separate)
    ///
    /// Total typical open time: 100-200ms on good connections.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const viewer = new AdvancedCogViewer();
    ///
    /// try {
    ///     // Open with 100MB cache
    ///     await viewer.open('<https://example.com/landsat8.tif>', 100);
    ///
    ///     console.log(`Opened: ${viewer.width()}x${viewer.height()}`);
    ///     console.log(`Tiles: ${viewer.tile_width()}x${viewer.tile_height()}`);
    ///     console.log(`Cache size: 100 MB`);
    /// } catch (error) {
    ///     if (error.message.includes('404')) {
    ///         console.error('File not found');
    ///     } else if (error.message.includes('CORS')) {
    ///         console.error('CORS not configured. Add these headers:');
    ///         console.error('  Access-Control-Allow-Origin: *');
    ///         console.error('  Access-Control-Allow-Headers: Range');
    ///     } else {
    ///         console.error('Failed to open:', error.message);
    ///     }
    /// }
    /// ```
    ///
    /// # See Also
    ///
    /// - `WasmCogViewer::open()` - Simple version without caching
    /// - `set_prefetch_strategy()` - Configure prefetching after opening
    /// - `get_cache_stats()` - Monitor cache performance
    #[wasm_bindgen]
    pub async fn open(&mut self, url: &str, cache_size_mb: usize) -> Result<(), JsValue> {
        // Log operation for debugging and performance tracking
        console::log_1(&format!("Opening COG with caching: {}", url).into());

        let backend = FetchBackend::new(url.to_string())
            .await
            .map_err(|e| to_js_error(&e))?;

        // Read header
        let header_bytes = backend
            .read_range_async(ByteRange::from_offset_length(0, 16))
            .await
            .map_err(|e| to_js_error(&e))?;

        let header =
            oxigdal_geotiff::TiffHeader::parse(&header_bytes).map_err(|e| to_js_error(&e))?;

        // Parse the full file
        let tiff = oxigdal_geotiff::TiffFile::parse(&backend).map_err(|e| to_js_error(&e))?;

        // Get image info
        let info = oxigdal_geotiff::ImageInfo::from_ifd(
            tiff.primary_ifd(),
            &backend,
            header.byte_order,
            header.variant,
        )
        .map_err(|e| to_js_error(&e))?;

        self.url = Some(url.to_string());
        self.width = info.width;
        self.height = info.height;
        self.tile_width = info.tile_width.unwrap_or(256);
        self.tile_height = info.tile_height.unwrap_or(256);
        self.band_count = u32::from(info.samples_per_pixel);
        self.overview_count = tiff.image_count().saturating_sub(1);

        // Get EPSG code
        if let Ok(Some(geo_keys)) = oxigdal_geotiff::geokeys::GeoKeyDirectory::from_ifd(
            tiff.primary_ifd(),
            &backend,
            header.byte_order,
            header.variant,
        ) {
            self.epsg_code = geo_keys.epsg_code();
        }

        // Create tile pyramid
        self.pyramid = Some(TilePyramid::new(
            self.width,
            self.height,
            self.tile_width,
            self.tile_height,
        ));

        // Create tile cache
        let cache_size = cache_size_mb * 1024 * 1024;
        self.cache = Some(TileCache::new(cache_size));

        // Create default viewport
        let mut viewport = Viewport::new(
            (self.width as f64) / 2.0,
            (self.height as f64) / 2.0,
            0,
            800,
            600,
        );
        viewport.fit_to_image(self.width, self.height);
        self.viewport = Some(viewport);

        console::log_1(
            &format!(
                "Opened COG: {}x{}, {} bands, {} overviews, cache: {}MB",
                self.width, self.height, self.band_count, self.overview_count, cache_size_mb
            )
            .into(),
        );

        Ok(())
    }

    /// Returns the image width
    #[wasm_bindgen]
    pub fn width(&self) -> u64 {
        self.width
    }

    /// Returns the image height
    #[wasm_bindgen]
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Returns the tile width
    #[wasm_bindgen]
    pub fn tile_width(&self) -> u32 {
        self.tile_width
    }

    /// Returns the tile height
    #[wasm_bindgen]
    pub fn tile_height(&self) -> u32 {
        self.tile_height
    }

    /// Returns the number of bands
    #[wasm_bindgen]
    pub fn band_count(&self) -> u32 {
        self.band_count
    }

    /// Returns the number of overview levels
    #[wasm_bindgen]
    pub fn overview_count(&self) -> usize {
        self.overview_count
    }

    /// Returns the EPSG code if available
    #[wasm_bindgen]
    pub fn epsg_code(&self) -> Option<u32> {
        self.epsg_code
    }

    /// Returns the URL
    #[wasm_bindgen]
    pub fn url(&self) -> Option<String> {
        self.url.clone()
    }

    /// Sets the viewport size
    #[wasm_bindgen(js_name = setViewportSize)]
    pub fn set_viewport_size(&mut self, width: u32, height: u32) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.width = width;
            viewport.height = height;
        }
    }

    /// Pans the viewport
    #[wasm_bindgen]
    pub fn pan(&mut self, dx: f64, dy: f64) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.pan(dx, dy);
        }
    }

    /// Zooms in
    #[wasm_bindgen(js_name = zoomIn)]
    pub fn zoom_in(&mut self) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.zoom_in();
        }
    }

    /// Zooms out
    #[wasm_bindgen(js_name = zoomOut)]
    pub fn zoom_out(&mut self) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.zoom_out();
        }
    }

    /// Sets the zoom level
    #[wasm_bindgen(js_name = setZoom)]
    pub fn set_zoom(&mut self, zoom: u32) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.set_zoom(zoom);
        }
    }

    /// Centers the viewport on a point
    #[wasm_bindgen(js_name = centerOn)]
    pub fn center_on(&mut self, x: f64, y: f64) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.center_on(x, y);
        }
    }

    /// Fits the viewport to the image
    #[wasm_bindgen(js_name = fitToImage)]
    pub fn fit_to_image(&mut self) {
        if let Some(ref mut viewport) = self.viewport {
            viewport.fit_to_image(self.width, self.height);
        }
    }

    /// Returns the current viewport as JSON
    #[wasm_bindgen(js_name = getViewport)]
    pub fn get_viewport(&self) -> Option<String> {
        self.viewport
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok())
    }

    /// Returns cache statistics as JSON
    #[wasm_bindgen(js_name = getCacheStats)]
    pub fn get_cache_stats(&self) -> Option<String> {
        self.cache
            .as_ref()
            .and_then(|c| serde_json::to_string(&c.stats()).ok())
    }

    /// Clears the tile cache
    #[wasm_bindgen(js_name = clearCache)]
    pub fn clear_cache(&mut self) {
        if let Some(ref mut cache) = self.cache {
            cache.clear();
        }
    }

    /// Sets the prefetch strategy
    #[wasm_bindgen(js_name = setPrefetchStrategy)]
    pub fn set_prefetch_strategy(&mut self, strategy: &str) {
        self.prefetch_strategy = match strategy {
            "none" => PrefetchStrategy::None,
            "neighbors" => PrefetchStrategy::Neighbors,
            "pyramid" => PrefetchStrategy::Pyramid,
            _ => PrefetchStrategy::Neighbors,
        };
    }

    /// Returns comprehensive metadata as JSON
    #[wasm_bindgen(js_name = getMetadata)]
    pub fn get_metadata(&self) -> String {
        let pyramid_info = self.pyramid.as_ref().map(|p| {
            serde_json::json!({
                "numLevels": p.num_levels,
                "totalTiles": p.total_tiles(),
                "tilesPerLevel": p.tiles_per_level,
            })
        });

        serde_json::json!({
            "url": self.url,
            "width": self.width,
            "height": self.height,
            "tileWidth": self.tile_width,
            "tileHeight": self.tile_height,
            "bandCount": self.band_count,
            "overviewCount": self.overview_count,
            "epsgCode": self.epsg_code,
            "pyramid": pyramid_info,
        })
        .to_string()
    }

    /// Computes image statistics for a region
    #[wasm_bindgen(js_name = computeStats)]
    pub async fn compute_stats(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<String, JsValue> {
        let tile_data = self.read_tile_internal(level, tile_x, tile_y).await?;

        let pixel_count = (self.tile_width * self.tile_height) as usize;
        let mut rgba = vec![0u8; pixel_count * 4];

        // Convert to RGBA
        self.convert_to_rgba(&tile_data, &mut rgba)?;

        let stats = ImageStats::from_rgba(&rgba, self.tile_width, self.tile_height)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        serde_json::to_string(&stats).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Computes histogram for a region (tile)
    ///
    /// Returns a comprehensive JSON object containing:
    /// - Image dimensions (width, height, total_pixels)
    /// - Per-channel histograms (red, green, blue, luminance)
    /// - Statistics for each channel (min, max, mean, median, std_dev, count)
    /// - Histogram bins (256 bins for 8-bit values)
    ///
    /// # Arguments
    ///
    /// * `level` - Overview/pyramid level (0 = full resolution)
    /// * `tile_x` - Tile X coordinate
    /// * `tile_y` - Tile Y coordinate
    ///
    /// # Example
    ///
    /// ```javascript
    /// const viewer = new AdvancedCogViewer();
    /// await viewer.open('<https://example.com/image.tif>', 100);
    ///
    /// // Get histogram for tile at (0, 0) at full resolution
    /// const histogramJson = await viewer.computeHistogram(0, 0, 0);
    /// const histogram = JSON.parse(histogramJson);
    ///
    /// console.log(`Luminance mean: ${histogram.luminance.mean}`);
    /// console.log(`Luminance std_dev: ${histogram.luminance.std_dev}`);
    /// console.log(`Red min/max: ${histogram.red.min} - ${histogram.red.max}`);
    /// ```
    #[wasm_bindgen(js_name = computeHistogram)]
    pub async fn compute_histogram(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<String, JsValue> {
        let tile_data = self.read_tile_internal(level, tile_x, tile_y).await?;

        let pixel_count = (self.tile_width * self.tile_height) as usize;
        let mut rgba = vec![0u8; pixel_count * 4];

        self.convert_to_rgba(&tile_data, &mut rgba)?;

        let hist = Histogram::from_rgba(&rgba, self.tile_width, self.tile_height)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        hist.to_json_string(self.tile_width, self.tile_height)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Reads a tile with caching
    #[wasm_bindgen(js_name = readTileCached)]
    pub async fn read_tile_cached(
        &mut self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let coord = TileCoord::new(level as u32, tile_x, tile_y);
        let timestamp = js_sys::Date::now() / 1000.0;

        // Check cache
        if let Some(ref mut cache) = self.cache {
            if let Some(data) = cache.get(&coord, timestamp) {
                return Ok(data);
            }
        }

        // Cache miss - fetch tile
        let data = self.read_tile_internal(level, tile_x, tile_y).await?;

        // Store in cache
        if let Some(ref mut cache) = self.cache {
            let _ = cache.put(coord, data.clone(), timestamp);
        }

        Ok(data)
    }

    /// Internal tile reading
    async fn read_tile_internal(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;

        let backend = FetchBackend::new(url.clone())
            .await
            .map_err(|e| to_js_error(&e))?;

        let reader = oxigdal_geotiff::CogReader::open(backend).map_err(|e| to_js_error(&e))?;
        reader
            .read_tile(level, tile_x, tile_y)
            .map_err(|e| to_js_error(&e))
    }

    /// Converts tile data to RGBA
    fn convert_to_rgba(&self, tile_data: &[u8], rgba: &mut [u8]) -> Result<(), JsValue> {
        let pixel_count = (self.tile_width * self.tile_height) as usize;

        match self.band_count {
            1 => {
                // Grayscale
                for (i, &v) in tile_data.iter().take(pixel_count).enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                    rgba[i * 4 + 3] = 255;
                }
            }
            3 => {
                // RGB
                for i in 0..pixel_count.min(tile_data.len() / 3) {
                    rgba[i * 4] = tile_data[i * 3];
                    rgba[i * 4 + 1] = tile_data[i * 3 + 1];
                    rgba[i * 4 + 2] = tile_data[i * 3 + 2];
                    rgba[i * 4 + 3] = 255;
                }
            }
            4 => {
                // RGBA
                for i in 0..pixel_count.min(tile_data.len() / 4) {
                    rgba[i * 4] = tile_data[i * 4];
                    rgba[i * 4 + 1] = tile_data[i * 4 + 1];
                    rgba[i * 4 + 2] = tile_data[i * 4 + 2];
                    rgba[i * 4 + 3] = tile_data[i * 4 + 3];
                }
            }
            _ => {
                // Use first band as grayscale
                for (i, &v) in tile_data.iter().take(pixel_count).enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                    rgba[i * 4 + 3] = 255;
                }
            }
        }

        Ok(())
    }

    /// Reads a tile as ImageData with caching
    #[wasm_bindgen(js_name = readTileAsImageData)]
    pub async fn read_tile_as_image_data(
        &mut self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<ImageData, JsValue> {
        let tile_data = self.read_tile_cached(level, tile_x, tile_y).await?;

        let pixel_count = (self.tile_width * self.tile_height) as usize;
        let mut rgba = vec![0u8; pixel_count * 4];

        self.convert_to_rgba(&tile_data, &mut rgba)?;

        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, self.tile_width, self.tile_height)
    }

    /// Applies contrast enhancement to a tile
    #[wasm_bindgen(js_name = readTileWithContrast)]
    pub async fn read_tile_with_contrast(
        &mut self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
        method: &str,
    ) -> Result<ImageData, JsValue> {
        let tile_data = self.read_tile_cached(level, tile_x, tile_y).await?;

        let pixel_count = (self.tile_width * self.tile_height) as usize;
        let mut rgba = vec![0u8; pixel_count * 4];

        self.convert_to_rgba(&tile_data, &mut rgba)?;

        // Apply contrast enhancement
        use crate::canvas::ContrastMethod;
        let contrast_method = match method {
            "linear" => ContrastMethod::LinearStretch,
            "histogram" => ContrastMethod::HistogramEqualization,
            "adaptive" => ContrastMethod::AdaptiveHistogramEqualization,
            _ => ContrastMethod::LinearStretch,
        };

        ImageProcessor::enhance_contrast(
            &mut rgba,
            self.tile_width,
            self.tile_height,
            contrast_method,
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, self.tile_width, self.tile_height)
    }
}

impl Default for AdvancedCogViewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch tile loader for efficient multi-tile loading
#[wasm_bindgen]
pub struct BatchTileLoader {
    viewer: AdvancedCogViewer,
    max_parallel: usize,
}

#[wasm_bindgen]
impl BatchTileLoader {
    /// Creates a new batch tile loader
    #[wasm_bindgen(constructor)]
    pub fn new(max_parallel: usize) -> Self {
        Self {
            viewer: AdvancedCogViewer::new(),
            max_parallel,
        }
    }

    /// Opens a COG
    #[wasm_bindgen]
    pub async fn open(&mut self, url: &str, cache_size_mb: usize) -> Result<(), JsValue> {
        self.viewer.open(url, cache_size_mb).await
    }

    /// Loads multiple tiles in parallel
    #[wasm_bindgen(js_name = loadTilesBatch)]
    pub async fn load_tiles_batch(
        &mut self,
        level: usize,
        tile_coords: Vec<u32>, // Flattened [x1, y1, x2, y2, ...]
    ) -> Result<Vec<JsValue>, JsValue> {
        let mut results = Vec::new();

        for chunk in tile_coords.chunks_exact(2).take(self.max_parallel) {
            let tile_x = chunk[0];
            let tile_y = chunk[1];

            match self
                .viewer
                .read_tile_as_image_data(level, tile_x, tile_y)
                .await
            {
                Ok(image_data) => results.push(image_data.into()),
                Err(e) => results.push(e),
            }
        }

        Ok(results)
    }
}

/// GeoJSON export utilities
#[wasm_bindgen]
pub struct GeoJsonExporter;

#[wasm_bindgen]
impl GeoJsonExporter {
    /// Exports image bounds as GeoJSON
    #[wasm_bindgen(js_name = exportBounds)]
    pub fn export_bounds(
        west: f64,
        south: f64,
        east: f64,
        north: f64,
        epsg: Option<u32>,
    ) -> String {
        serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[
                    [west, south],
                    [east, south],
                    [east, north],
                    [west, north],
                    [west, south]
                ]]
            },
            "properties": {
                "epsg": epsg
            }
        })
        .to_string()
    }

    /// Exports a point as GeoJSON
    #[wasm_bindgen(js_name = exportPoint)]
    pub fn export_point(x: f64, y: f64, properties: &str) -> String {
        let props: serde_json::Value =
            serde_json::from_str(properties).unwrap_or(serde_json::json!({}));

        serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [x, y]
            },
            "properties": props
        })
        .to_string()
    }
}
