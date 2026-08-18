//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

pub use super::canvas::{Histogram, ImageProcessor, ImageStats};
pub use super::fetch::FetchBackend;
pub use super::tile::{PrefetchStrategy, TileCache, TileCoord};
use crate::buffered_source;
use crate::buffered_source::BufferedRangeSource;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{ImageData, console};

use super::functions::{collect_level_geometry, tile_to_rgba, to_js_error};
use super::types_5::{CogLevelGeometry, CogSession, Viewport};

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
    pub(super) url: Option<String>,
    /// Image metadata - width in pixels
    pub(super) width: u64,
    /// Image metadata - height in pixels
    pub(super) height: u64,
    /// Tile dimensions - width in pixels
    pub(super) tile_width: u32,
    /// Tile dimensions - height in pixels
    pub(super) tile_height: u32,
    /// Number of spectral bands in the image
    pub(super) band_count: u32,
    /// Number of overview/pyramid levels
    pub(super) overview_count: usize,
    /// EPSG code for coordinate reference system
    pub(super) epsg_code: Option<u32>,
    /// Geometry of every level the opened file actually has, in level order
    /// (index 0 = full resolution), as the reader that serves the tiles
    /// understands them.
    ///
    /// This is the source of the metadata JSON's `pyramid` block. It used to be
    /// a [`TilePyramid`] synthesised from the image dimensions alone — halving
    /// down to a single tile regardless of what the file contains — so the block
    /// routinely contradicted `overviewCount`, which comes from the parsed IFD
    /// chain. `TilePyramid` is untouched and still exported for tile-scheme
    /// math; it simply is no longer what this viewer reports.
    pub(super) levels: Vec<CogLevelGeometry>,
    /// LRU tile cache for efficient memory management
    pub(super) cache: Option<TileCache>,
    /// Current viewport state (pan, zoom, bounds)
    pub(super) viewport: Option<Viewport>,
    /// Strategy for prefetching nearby tiles
    pub(super) prefetch_strategy: PrefetchStrategy,
    /// Everything needed to serve tiles from the opened URL, kept alive across
    /// tile reads.
    ///
    /// Every cache miss used to build a fresh `FetchBackend` (a HEAD request)
    /// and re-parse the whole TIFF directory before it could read one tile, so
    /// the tile cache only ever hid part of the cost it was there to hide.
    /// Keyed by URL so re-opening a different file can never serve tiles from
    /// the previous one; see [`WasmCogViewer::url_reader`] for the borrow
    /// discipline this follows.
    pub(super) session: RefCell<Option<CogSession>>,
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
            levels: Vec::new(),
            cache: None,
            viewport: None,
            prefetch_strategy: PrefetchStrategy::Neighbors,
            session: RefCell::new(None),
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
    /// 5. **Level Geometry**: Records the dimensions and block grid of every level
    ///    the file actually has, taken from the reader's own level → IFD map
    /// 6. **Cache Initialization**: Creates LRU cache with specified size
    /// 7. **Viewport Setup**: Initializes viewport with default settings
    ///
    /// Steps 2-5 are synchronous work over an asynchronous transport: the
    /// parser is run against a buffer of downloaded ranges, and every range it
    /// turns out to need is fetched and the parse retried (see
    /// the crate-private `buffered_source` module). Before that machinery existed
    /// this method could not work in a browser at all — the parser's first read hit
    /// `FetchBackend`'s synchronous `read_range`, which cannot fetch anything.
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
    /// Opening a COG typically requires 2 HTTP requests:
    /// 1. HEAD request (~10ms)
    /// 2. One 64 KiB range covering the header, the whole IFD chain and the
    ///    GeoTIFF tags (~50ms) — a cloud-optimized file puts all of them at the
    ///    front, so one block answers every read the parser makes
    ///
    /// A file whose directory chain is spread further out costs one extra
    /// request per additional block, since each round asks for exactly the
    /// ranges the previous one turned out to need.
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
        console::log_1(&format!("Opening COG with caching: {}", url).into());
        let session = Self::open_session(url).await?;
        let reader = Rc::clone(&session.reader);
        let info = reader.primary_info();
        self.url = Some(url.to_string());
        self.width = info.width;
        self.height = info.height;
        self.tile_width = info.tile_width.unwrap_or(256);
        self.tile_height = info.tile_height.unwrap_or(256);
        self.band_count = u32::from(info.samples_per_pixel);
        self.overview_count = reader.overview_count();
        if let Some(geo_keys) = reader.geo_keys() {
            self.epsg_code = geo_keys.epsg_code();
        }
        self.levels = buffered_source::pull_until_ready(&session.source, &*session.fetcher, || {
            collect_level_geometry(&reader, &session.source)
        })
        .await
        .map_err(|e| to_js_error(&e))?;
        *self.session.borrow_mut() = Some(session);
        let cache_size = cache_size_mb * 1024 * 1024;
        self.cache = Some(TileCache::new(cache_size));
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
    ///
    /// The `pyramid` block describes the levels the **file** has — the same
    /// mask-filtered IFD chain `overviewCount` is taken from — so `numLevels`
    /// is always `overviewCount + 1` and every entry of `tilesPerLevel` is the
    /// block grid of a level whose tiles this viewer can actually read. It used
    /// to be synthesised from the image dimensions alone (halving until one tile
    /// remained), which described a pyramid the file need not contain: a COG
    /// with no overviews at all still reported several levels.
    ///
    /// Keys are unchanged; `levels` is added alongside them with each level's
    /// own dimensions and block size.
    #[wasm_bindgen(js_name = getMetadata)]
    pub fn get_metadata(&self) -> String {
        let pyramid_info = (!self.levels.is_empty()).then(|| {
            serde_json::json!(
                { "numLevels" : self.levels.len(), "totalTiles" : self.levels.iter()
                .map(| level | u64::from(level.tiles_x) * u64::from(level.tiles_y))
                .sum::< u64 > (), "tilesPerLevel" : self.levels.iter().map(| level |
                [level.tiles_x, level.tiles_y]).collect::< Vec < _ >> (), "levels" :
                self.levels.iter().map(| level | serde_json::json!({ "width" : level
                .width, "height" : level.height, "tileWidth" : level.tile_width,
                "tileHeight" : level.tile_height, "tilesX" : level.tiles_x, "tilesY"
                : level.tiles_y, })).collect::< Vec < _ >> (), }
            )
        });
        serde_json::json!(
            { "url" : self.url, "width" : self.width, "height" : self.height, "tileWidth"
            : self.tile_width, "tileHeight" : self.tile_height, "bandCount" : self
            .band_count, "overviewCount" : self.overview_count, "epsgCode" : self
            .epsg_code, "pyramid" : pyramid_info, }
        )
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
        let (tile_width, tile_height) = self.level_tile_size(level, tile_y).await?;
        let tile_data = self.read_tile_internal(level, tile_x, tile_y).await?;
        let rgba = tile_to_rgba(&tile_data, self.band_count, tile_width, tile_height);
        let stats = ImageStats::from_rgba(&rgba, tile_width, tile_height)
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
        let (tile_width, tile_height) = self.level_tile_size(level, tile_y).await?;
        let tile_data = self.read_tile_internal(level, tile_x, tile_y).await?;
        let rgba = tile_to_rgba(&tile_data, self.band_count, tile_width, tile_height);
        let hist = Histogram::from_rgba(&rgba, tile_width, tile_height)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        hist.to_json_string(tile_width, tile_height)
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
        if let Some(ref mut cache) = self.cache
            && let Some(data) = cache.get(&coord, timestamp)
        {
            return Ok(data);
        }
        let data = self.read_tile_internal(level, tile_x, tile_y).await?;
        if let Some(ref mut cache) = self.cache {
            let _ = cache.put(coord, data.clone(), timestamp);
        }
        Ok(data)
    }
    /// Internal tile reading.
    ///
    /// The tile's bytes are usually not downloaded yet, so the synchronous read
    /// is driven through the pull loop: it is attempted, the ranges it turns out
    /// to need are fetched, and it is attempted again. A tile whose block is
    /// already buffered costs no request at all.
    pub(super) async fn read_tile_internal(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;
        let session = self.session_for(url).await?;
        buffered_source::pull_until_ready(&session.source, &*session.fetcher, || {
            session.reader.read_tile(level, tile_x, tile_y)
        })
        .await
        .map_err(|e| to_js_error(&e))
    }
    /// Opens `url`: HEAD for the size, then the COG itself, parsed by driving
    /// the synchronous reader over asynchronously fetched byte ranges.
    ///
    /// The buffering source outlives the call inside the returned session, so
    /// the bytes the header walk downloaded are still there when the first tile
    /// read needs them.
    pub(super) async fn open_session(url: &str) -> Result<CogSession, JsValue> {
        let fetcher = Rc::new(
            FetchBackend::new(url.to_string())
                .await
                .map_err(|e| to_js_error(&e))?,
        );
        let source = BufferedRangeSource::new(fetcher.total_size_hint());
        let reader = buffered_source::pull_until_ready(&source, &*fetcher, || {
            oxigeo_geotiff::CogReader::open(source.clone())
        })
        .await
        .map_err(|e| to_js_error(&e))?;
        Ok(CogSession {
            url: url.to_string(),
            reader: Rc::new(reader),
            source,
            fetcher,
        })
    }
    /// Returns the session for `url`, opening it on first use.
    ///
    /// Mirrors [`WasmCogViewer::url_reader_for`]: keyed by URL so a re-opened
    /// viewer never serves stale tiles, nothing is stored on failure so a
    /// transient error is retried on the next call, and the `RefCell` is only
    /// borrowed to clone the handles — never across the `.await`.
    pub(super) async fn session_for(&self, url: &str) -> Result<CogSession, JsValue> {
        let cached = self.session.borrow().clone();
        if let Some(session) = cached
            && session.url == url
        {
            return Ok(session);
        }
        let session = Self::open_session(url).await?;
        *self.session.borrow_mut() = Some(session.clone());
        Ok(session)
    }
    /// Returns the pixel geometry of the block
    /// `read_tile_internal(level, _, tile_y)` decodes.
    ///
    /// Mirrors [`WasmCogViewer::level_tile_size`], and for the same reason: an
    /// overview may declare its own `TileWidth`/`TileLength`, so sizing an RGBA
    /// buffer from the full-resolution `tile_width`/`tile_height` truncated or
    /// zero-padded every tile of such a level. The geometry comes from the same
    /// reader — and the same computation — that produces the bytes.
    pub(super) async fn level_tile_size(
        &self,
        level: usize,
        tile_y: u32,
    ) -> Result<(u32, u32), JsValue> {
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;
        let session = self.session_for(url).await?;
        buffered_source::pull_until_ready(&session.source, &*session.fetcher, || {
            session.reader.tile_pixel_size(level, tile_y)
        })
        .await
        .map_err(|e| to_js_error(&e))
    }
    /// Reads a tile as ImageData with caching
    #[wasm_bindgen(js_name = readTileAsImageData)]
    pub async fn read_tile_as_image_data(
        &mut self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<ImageData, JsValue> {
        let (tile_width, tile_height) = self.level_tile_size(level, tile_y).await?;
        let tile_data = self.read_tile_cached(level, tile_x, tile_y).await?;
        let rgba = tile_to_rgba(&tile_data, self.band_count, tile_width, tile_height);
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, tile_width, tile_height)
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
        let (tile_width, tile_height) = self.level_tile_size(level, tile_y).await?;
        let tile_data = self.read_tile_cached(level, tile_x, tile_y).await?;
        let mut rgba = tile_to_rgba(&tile_data, self.band_count, tile_width, tile_height);
        use crate::canvas::ContrastMethod;
        let contrast_method = match method {
            "linear" => ContrastMethod::LinearStretch,
            "histogram" => ContrastMethod::HistogramEqualization,
            "adaptive" => ContrastMethod::AdaptiveHistogramEqualization,
            _ => ContrastMethod::LinearStretch,
        };
        ImageProcessor::enhance_contrast(&mut rgba, tile_width, tile_height, contrast_method)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, tile_width, tile_height)
    }
}
