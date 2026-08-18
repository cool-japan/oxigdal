//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

pub use super::fetch::FetchBackend;
use crate::buffered_source::BufferedRangeSource;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

/// The geometry of one parsed COG level, taken from the very IFD the reader
/// resolves that level to.
///
/// `tiles_x`/`tiles_y` are the reader's own block-grid dimensions
/// (`ImageInfo::tiles_across`/`tiles_down`), so a tile index this reports as
/// valid is one the reader's tile lookups accept. For a striped level the
/// "tile" is a strip: full image width by `RowsPerStrip` rows.
#[derive(Debug, Clone, Copy)]
pub(super) struct CogLevelGeometry {
    /// This level's image width in pixels.
    pub(super) width: u64,
    /// This level's image height in pixels.
    pub(super) height: u64,
    /// Width of one block (tile, or strip = image width).
    pub(super) tile_width: u32,
    /// Height of one block (tile, or strip = `RowsPerStrip`).
    pub(super) tile_height: u32,
    /// Blocks across this level.
    pub(super) tiles_x: u32,
    /// Blocks down this level.
    pub(super) tiles_y: u32,
}
/// An opened remote COG: the parsed reader, the buffer its reads are served
/// from, and the transport that fills that buffer.
///
/// All three are needed on every tile read, because a tile whose bytes have not
/// been downloaded yet has to be pulled in mid-read — see
/// [`crate::buffered_source`]. Cloning is three reference-count bumps.
#[derive(Clone)]
pub(super) struct CogSession {
    /// URL this session was opened from; a mismatch forces a re-open.
    pub(super) url: String,
    /// The parsed COG, reading through [`Self::source`].
    pub(super) reader: Rc<oxigeo_geotiff::CogReader<BufferedRangeSource>>,
    /// The byte ranges downloaded so far, shared with `reader`'s own copy.
    pub(super) source: BufferedRangeSource,
    /// The transport used to fill `source`.
    pub(super) fetcher: Rc<FetchBackend>,
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
        let x_scale = (image_width as f64) / (self.width as f64);
        let y_scale = (image_height as f64) / (self.height as f64);
        let scale = x_scale.max(y_scale);
        self.zoom = scale.log2().ceil() as u32;
    }
}
