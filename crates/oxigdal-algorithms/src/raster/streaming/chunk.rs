/// Error type for streaming raster operations.
#[derive(Debug, Clone, PartialEq)]
pub enum RasterError {
    OutOfBounds,
    InvalidChunkSize,
    IoError(String),
    /// Wraps an error from core algorithm operations
    AlgorithmError(String),
}

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds => write!(f, "raster read out of bounds"),
            Self::InvalidChunkSize => write!(f, "chunk size must be > 0"),
            Self::IoError(s) => write!(f, "IO error: {}", s),
            Self::AlgorithmError(s) => write!(f, "algorithm error: {}", s),
        }
    }
}

impl std::error::Error for RasterError {}

/// Trait for raster data sources that can be read in rectangular windows.
///
/// Implementations must provide row-major f32 data for any sub-window.
/// Out-of-bounds accesses (when x + w > width or y + h > height) must be
/// zero-padded rather than returning an error, to support halo reads at
/// raster boundaries.
pub trait RasterSource {
    /// Total width of the raster in pixels.
    fn width(&self) -> usize;

    /// Total height of the raster in pixels.
    fn height(&self) -> usize;

    /// Read a rectangular window of pixels into a `Vec<f32>`, row-major order.
    ///
    /// `(x, y)` is the top-left corner of the window.  The returned vector
    /// always has exactly `w * h` elements.  Pixels that fall outside the
    /// raster extent are returned as `0.0` (zero-padding for halo support).
    ///
    /// Returns `Err(RasterError::OutOfBounds)` when the *origin* `(x, y)` is
    /// completely outside the raster (i.e. `x >= width || y >= height`).
    fn read_window(&self, x: usize, y: usize, w: usize, h: usize) -> Result<Vec<f32>, RasterError>;
}

/// In-memory raster source backed by a `Vec<f32>` for testing and small workloads.
///
/// This implementation stores raster data in a flat row-major `Vec<f32>` and
/// supports efficient sub-window reads with zero-padding for out-of-bounds pixels.
pub struct InMemoryRasterSource {
    data: Vec<f32>,
    width: usize,
    height: usize,
}

impl InMemoryRasterSource {
    /// Creates a new in-memory raster source.
    ///
    /// # Panics
    ///
    /// Panics if `data.len() != width * height`.
    pub fn new(data: Vec<f32>, width: usize, height: usize) -> Self {
        assert_eq!(
            data.len(),
            width * height,
            "data length must equal width * height"
        );
        Self {
            data,
            width,
            height,
        }
    }

    /// Returns a reference to the underlying data slice.
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}

impl RasterSource for InMemoryRasterSource {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn read_window(&self, x: usize, y: usize, w: usize, h: usize) -> Result<Vec<f32>, RasterError> {
        // Reject origins that are entirely outside the raster.
        if x >= self.width || y >= self.height {
            return Err(RasterError::OutOfBounds);
        }

        let mut out = Vec::with_capacity(w * h);
        for row in 0..h {
            for col in 0..w {
                let src_x = x + col;
                let src_y = y + row;
                if src_x < self.width && src_y < self.height {
                    out.push(self.data[src_y * self.width + src_x]);
                } else {
                    // Zero-pad pixels outside the raster extent (halo edge handling).
                    out.push(0.0_f32);
                }
            }
        }
        Ok(out)
    }
}

/// A chunk of raster data extracted from a `RasterSource`, including surrounding
/// halo (ghost/overlap) cells.
///
/// The `data` field holds `(width + 2 * halo) × (height + 2 * halo)` pixels
/// in row-major order.  Coordinate `(0, 0)` in `data` corresponds to raster
/// position `(x.saturating_sub(halo), y.saturating_sub(halo))`; pixels that
/// would fall outside the source raster are zero-padded.
pub struct Chunk {
    /// Top-left x of the **core** region (without halo) in source coordinates.
    pub x: usize,
    /// Top-left y of the **core** region (without halo) in source coordinates.
    pub y: usize,
    /// Width of the **core** region (pixels, without halo columns).
    pub width: usize,
    /// Height of the **core** region (pixels, without halo rows).
    pub height: usize,
    /// Halo size on each of the four sides (in pixels).
    pub halo: usize,
    /// Row-major data of size `(width + 2*halo) × (height + 2*halo)`.
    ///
    /// Row 0 and row `height + 2*halo - 1` are halo rows; column 0 and
    /// column `width + 2*halo - 1` are halo columns.  The origin `(0, 0)`
    /// maps to source position `(x.saturating_sub(halo), y.saturating_sub(halo))`.
    pub data: Vec<f32>,
}

impl Chunk {
    /// Returns the value at `(col, row)` within the **core** region, applying
    /// the halo offset transparently.
    ///
    /// `col` must be in `0..self.width` and `row` in `0..self.height`.
    #[inline]
    pub fn get_core(&self, col: usize, row: usize) -> f32 {
        let full_w = self.width + 2 * self.halo;
        self.data[(row + self.halo) * full_w + (col + self.halo)]
    }

    /// Returns the full width of `data` (core width + 2 * halo).
    #[inline]
    pub fn full_width(&self) -> usize {
        self.width + 2 * self.halo
    }

    /// Returns the full height of `data` (core height + 2 * halo).
    #[inline]
    pub fn full_height(&self) -> usize {
        self.height + 2 * self.halo
    }
}
