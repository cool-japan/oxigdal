//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

pub use super::memory_source::MemorySource;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{ImageData, console};

use super::functions::{decode_elevation, tile_to_rgba, to_js_error};
use crate::cog_reader;

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
    pub(super) url: Option<String>,
    /// Image width in pixels
    pub(super) width: u64,
    /// Image height in pixels
    pub(super) height: u64,
    /// Tile width in pixels (typically 256 or 512)
    pub(super) tile_width: u32,
    /// Tile height in pixels (typically 256 or 512)
    pub(super) tile_height: u32,
    /// Number of bands/channels in the image
    pub(super) band_count: u32,
    /// Number of overview/pyramid levels available
    pub(super) overview_count: usize,
    /// EPSG code for the coordinate reference system (if available)
    pub(super) epsg_code: Option<u32>,
    /// Bits per (first) sample — needed to decode raw elevation tiles
    pub(super) bits_per_sample: u16,
    /// TIFF SampleFormat (1=uint, 2=int, 3=float) — needed for elevation decode
    pub(super) sample_format: u16,
    /// GeoTIFF geotransform data (for calculating geographic bounds)
    pub(super) pixel_scale_x: Option<f64>,
    pub(super) pixel_scale_y: Option<f64>,
    pub(super) tiepoint_pixel_x: Option<f64>,
    pub(super) tiepoint_pixel_y: Option<f64>,
    pub(super) tiepoint_geo_x: Option<f64>,
    pub(super) tiepoint_geo_y: Option<f64>,
    /// In-memory reader for `openBytes` (drag-drop) sources with full codec
    /// support. When `Some`, tile reads use this instead of the URL fast path.
    pub(super) mem_reader: Option<oxigeo_geotiff::CogReader<MemorySource>>,
    /// Parsed reader for the URL path, kept alive across tile reads together
    /// with the URL it was opened from.
    ///
    /// Opening a COG costs a HEAD request plus one range request per IFD in the
    /// chain; re-doing that for every tile turned a pan across a 4x4 tile grid
    /// into ~16 redundant header round-trips. The reader is now parsed once and
    /// reused. `RefCell` because `read_tile` takes `&self` (a `#[wasm_bindgen]`
    /// method exposed to JS) — wasm32 is single-threaded, so the non-`Send`
    /// cell costs nothing there, and the cell is only ever borrowed for the
    /// duration of a clone, never across an `.await`.
    ///
    /// Keyed by URL so a re-`open` (or an `openBytes` that overwrites `url`
    /// with a display name) can never serve tiles from the previous file: a
    /// mismatch degrades to a re-open, not to wrong pixels.
    pub(super) url_reader: RefCell<Option<(String, Rc<cog_reader::WasmCogReader>)>>,
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
            bits_per_sample: 8,
            sample_format: 1,
            pixel_scale_x: None,
            pixel_scale_y: None,
            tiepoint_pixel_x: None,
            tiepoint_pixel_y: None,
            tiepoint_geo_x: None,
            tiepoint_geo_y: None,
            mem_reader: None,
            url_reader: RefCell::new(None),
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
        console::log_1(&format!("Opening COG: {}", url).into());
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
        self.bits_per_sample = metadata.bits_per_sample;
        self.sample_format = metadata.sample_format;
        self.mem_reader = None;
        self.pixel_scale_x = metadata.pixel_scale_x;
        self.pixel_scale_y = metadata.pixel_scale_y;
        self.tiepoint_pixel_x = metadata.tiepoint_pixel_x;
        self.tiepoint_pixel_y = metadata.tiepoint_pixel_y;
        self.tiepoint_geo_x = metadata.tiepoint_geo_x;
        self.tiepoint_geo_y = metadata.tiepoint_geo_y;
        *self.url_reader.borrow_mut() = Some((url.to_string(), Rc::new(reader)));
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
        serde_json::json!(
            { "url" : self.url, "width" : self.width, "height" : self.height, "tileWidth"
            : self.tile_width, "tileHeight" : self.tile_height, "bandCount" : self
            .band_count, "overviewCount" : self.overview_count, "epsgCode" : self
            .epsg_code, "geotransform" : { "pixelScaleX" : self.pixel_scale_x,
            "pixelScaleY" : self.pixel_scale_y, "tiepointPixelX" : self.tiepoint_pixel_x,
            "tiepointPixelY" : self.tiepoint_pixel_y, "tiepointGeoX" : self
            .tiepoint_geo_x, "tiepointGeoY" : self.tiepoint_geo_y, }, }
        )
        .to_string()
    }
    /// Returns pixel scale X (degrees/pixel in lon direction)
    #[wasm_bindgen]
    pub fn pixel_scale_x(&self) -> Option<f64> {
        self.pixel_scale_x
    }
    /// Returns pixel scale Y: the magnitude of degrees (or CRS units) per
    /// pixel in the lat/northing direction, always positive.
    ///
    /// This matches the GeoTIFF `ModelPixelScaleTag` convention — the tag is
    /// defined as strictly positive, and GDAL and other conforming writers
    /// store it that way. This crate does not compute a pixel-to-CRS affine
    /// transform, so applying the north-up sign (a negative Y step) when
    /// building one is the consumer's responsibility.
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
    /// Opens a COG/GeoTIFF from an in-memory byte buffer (e.g. a drag-and-dropped
    /// local file).
    ///
    /// Unlike the URL path — which streams via HTTP range requests and only
    /// supports uncompressed and DEFLATE tiles — the bytes path drives the full
    /// synchronous `oxigeo_geotiff::CogReader`, so it handles the complete codec
    /// set (None/Deflate/LZW/Zstd/PackBits/JPEG/WebP) plus horizontal/floating
    /// predictors. Tile reads afterwards use the in-memory reader.
    ///
    /// # Arguments
    ///
    /// * `data` - The full file contents.
    /// * `name` - Optional display name, stored as the viewer's `url`.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error if the bytes are not a valid TIFF/COG.
    #[wasm_bindgen(js_name = openBytes)]
    pub fn open_bytes(&mut self, data: &[u8], name: Option<String>) -> Result<(), JsValue> {
        let source = MemorySource::new(data.to_vec());
        let reader = oxigeo_geotiff::CogReader::open(source).map_err(|e| to_js_error(&e))?;
        let info = reader.primary_info();
        self.width = info.width;
        self.height = info.height;
        self.tile_width = info.tile_width.unwrap_or(256);
        self.tile_height = info.tile_height.unwrap_or(256);
        self.band_count = u32::from(info.samples_per_pixel);
        self.bits_per_sample = info.bits_per_sample.first().copied().unwrap_or(8);
        self.sample_format = info.sample_format as u16;
        self.overview_count = reader.overview_count();
        self.epsg_code = reader.geo_keys().and_then(|g| g.epsg_code());
        if let Ok(Some(gt)) = reader.geo_transform() {
            self.pixel_scale_x = Some(gt.pixel_width.abs());
            self.pixel_scale_y = Some(gt.pixel_height.abs());
            self.tiepoint_pixel_x = Some(0.0);
            self.tiepoint_pixel_y = Some(0.0);
            self.tiepoint_geo_x = Some(gt.origin_x);
            self.tiepoint_geo_y = Some(gt.origin_y);
        } else {
            self.pixel_scale_x = None;
            self.pixel_scale_y = None;
            self.tiepoint_pixel_x = None;
            self.tiepoint_pixel_y = None;
            self.tiepoint_geo_x = None;
            self.tiepoint_geo_y = None;
        }
        self.url = name;
        self.mem_reader = Some(reader);
        *self.url_reader.borrow_mut() = None;
        console::log_1(
            &format!(
                "Opened bytes: {}x{}, {} bands, {} overviews, sample_format={}, bits={}",
                self.width,
                self.height,
                self.band_count,
                self.overview_count,
                self.sample_format,
                self.bits_per_sample
            )
            .into(),
        );
        Ok(())
    }
    /// Reads a tile and returns raw (decoded) bytes.
    ///
    /// For bytes-opened viewers this delegates to the in-memory `CogReader`
    /// (decompression and predictor handled internally, full codec support).
    /// For URL-opened viewers it streams the tile over HTTP range requests.
    ///
    /// Both readers return samples in the **host's** byte order, so callers do
    /// not need to know which one produced the tile. That was not always true:
    /// until cool-japan/oxigeo#14 the URL reader returned samples in the file's
    /// order and this viewer carried a `little_endian` flag to compensate,
    /// which the `openBytes` path then had to set to a value that meant
    /// something different. Both readers now normalise internally and the flag
    /// is gone.
    #[wasm_bindgen]
    pub async fn read_tile(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> std::result::Result<Vec<u8>, JsValue> {
        if let Some(reader) = self.mem_reader.as_ref() {
            return reader
                .read_tile(level, tile_x, tile_y)
                .map_err(|e| to_js_error(&e));
        }
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;
        let reader = self.url_reader_for(url).await?;
        reader
            .read_tile_level(level, tile_x, tile_y)
            .await
            .map_err(|e| to_js_error(&e))
    }
    /// Returns the parsed URL reader for `url`, opening it on first use.
    ///
    /// The cached reader is keyed by URL, so a viewer re-pointed at a different
    /// file re-opens instead of serving stale tiles. A failed open stores
    /// nothing, so a transient network error does not poison the viewer — the
    /// next call retries.
    ///
    /// The `RefCell` is never borrowed across the `.await`: the existing entry
    /// is cloned out (an `Rc` bump) and the guard dropped before any suspension
    /// point, so concurrent in-flight tile reads cannot collide on it. Two
    /// simultaneous first-calls may both open; the second store simply wins and
    /// both get a valid reader.
    pub(super) async fn url_reader_for(
        &self,
        url: &str,
    ) -> std::result::Result<Rc<cog_reader::WasmCogReader>, JsValue> {
        let cached = self.url_reader.borrow().clone();
        if let Some((cached_url, reader)) = cached
            && cached_url == url
        {
            return Ok(reader);
        }
        let reader = Rc::new(
            cog_reader::WasmCogReader::open(url.to_string())
                .await
                .map_err(|e| to_js_error(&e))?,
        );
        *self.url_reader.borrow_mut() = Some((url.to_string(), Rc::clone(&reader)));
        Ok(reader)
    }
    /// Reads a tile and decodes its raw samples to `f32` elevation values.
    ///
    /// The decoding honours the source's SampleFormat and BitsPerSample
    /// (u8 / u16 / i16 / i32 / f32, plus u32 / f64), so elevation DEMs such as
    /// the SRTM `i16` tiles are returned as real heights. Works for both
    /// URL-opened and bytes-opened viewers, and for `II` and `MM` sources
    /// alike: [`Self::read_tile`] has already normalised the samples.
    #[wasm_bindgen(js_name = readTileElevation)]
    pub async fn read_tile_elevation(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> std::result::Result<Vec<f32>, JsValue> {
        let raw = self.read_tile(level, tile_x, tile_y).await?;
        Ok(decode_elevation(
            &raw,
            self.sample_format,
            self.bits_per_sample,
        ))
    }
    /// Returns the pixel geometry of the block `read_tile(level, _, tile_y)`
    /// decodes, which is what an image buffer for that tile must be sized from.
    ///
    /// It is *not* `tile_width()`/`tile_height()`: those describe the
    /// full-resolution IFD, and an overview may declare its own
    /// `TileWidth`/`TileLength` (GDAL's `-co BLOCKXSIZE` applies to the base
    /// image only; `gdaladdo` is free to use another). Now that `read_tile`
    /// honours its `level` argument, sizing from level 0 truncated or zero-padded
    /// every such overview tile.
    pub(super) async fn level_tile_size(
        &self,
        level: usize,
        tile_y: u32,
    ) -> std::result::Result<(u32, u32), JsValue> {
        if let Some(reader) = self.mem_reader.as_ref() {
            return reader
                .tile_pixel_size(level, tile_y)
                .map_err(|e| to_js_error(&e));
        }
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No file opened"))?;
        let reader = self.url_reader_for(url).await?;
        let metadata = reader.metadata();
        let level_meta = metadata
            .levels
            .get(level)
            .ok_or_else(|| JsValue::from_str(&format!("Overview level {} out of bounds", level)))?;
        Ok((level_meta.tile_width, level_meta.tile_height))
    }
    /// Reads a tile and converts to RGBA ImageData for canvas rendering
    #[wasm_bindgen]
    pub async fn read_tile_as_image_data(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> std::result::Result<ImageData, JsValue> {
        let (tile_width, tile_height) = self.level_tile_size(level, tile_y).await?;
        let tile_data = self.read_tile(level, tile_x, tile_y).await?;
        let rgba = tile_to_rgba(&tile_data, self.band_count, tile_width, tile_height);
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, tile_width, tile_height)
    }
}
