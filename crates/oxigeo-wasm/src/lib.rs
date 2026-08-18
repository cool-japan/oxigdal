//! # OxiGeo WASM - WebAssembly Bindings for Browser-based Geospatial Processing
//!
//! This crate provides comprehensive WebAssembly bindings for OxiGeo, enabling
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
//! import init, { WasmCogViewer } from '@cooljapan/oxigeo';
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
//! } from '@cooljapan/oxigeo';
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
//! This crate is part of the OxiGeo project and follows the same licensing terms.

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

mod animation;
mod anomaly;
mod bindings;
mod buffered_source;
mod canvas;
mod cog_reader;
mod color;
mod compression;
mod error;
mod fetch;
mod memory_source;
mod profiler;
mod rendering;
mod streaming;
mod terrain;
#[cfg(test)]
mod tests;
mod tile;
mod vault;
mod worker;

// Extracted from this file's own inline definitions (COOLJAPAN <2000-line
// policy) — see the `pub use` block below for what each re-exports.
mod advancedcogviewer_traits;
mod functions;
mod types;
mod types_4;
mod types_5;
mod types_6;
mod wasmcogviewer_traits;

// WASM Component Model (wasm32-wasip2) support
pub mod component;
pub mod wasm_memory;

// GeoSentinel — in-browser Sentinel-2 change detection (UTM, STAC, COG pipeline)
pub mod sentinel;

pub use animation::{
    Animation, Easing, EasingFunction, PanAnimation, SpringAnimation, ZoomAnimation,
};
pub use bindings::{
    DocGenerator, TsClass, TsFunction, TsInterface, TsModule, TsParameter, TsType, TsTypeAlias,
    create_oxigeo_wasm_docs,
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
pub use functions::{decode_elevation, init, is_tiff_url, to_js_error, version};
pub use memory_source::MemorySource;
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
pub use terrain::WasmTerrain;
pub use tile::{
    CacheStats, CachedTile, PrefetchStrategy, TileBounds, TileCache, TileCoord, TilePrefetcher,
    TilePyramid, WasmTileCache,
};
pub use types::{BatchTileLoader, GeoJsonExporter};
pub use types_4::AdvancedCogViewer;
pub use types_5::Viewport;
pub use types_6::WasmCogViewer;
pub use worker::{
    JobId, JobStatus, PendingJob, PoolStats, WasmWorkerPool, WorkerInfo, WorkerJobRequest,
    WorkerJobResponse, WorkerPool, WorkerRequestType, WorkerResponseType,
};
