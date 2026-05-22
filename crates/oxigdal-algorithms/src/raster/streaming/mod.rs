/// Streaming chunked raster processing.
///
/// This module provides an out-of-core raster processing abstraction that
/// decomposes a large raster into overlapping tiles (chunks with halo cells)
/// and applies a kernel function to each tile, stitching results into a full
/// output array.
///
/// # Design
///
/// The [`ChunkedRaster`] holds a [`RasterSource`] reference and exposes a
/// [`ChunkIterator`] that emits [`Chunk`] values in raster-scan order.  Each
/// chunk carries the core pixel region **plus** a surrounding halo of
/// user-specified thickness; halo cells that would fall outside the source
/// raster are zero-padded.  This halo allows neighbourhood-based algorithms
/// (hillshade, slope, focal mean, …) to produce seamless results at chunk
/// boundaries.
///
/// [`process_streaming`] is the primary entry point: it iterates the chunks,
/// calls a user-provided kernel closure, and assembles the per-chunk outputs
/// (without halo) into a single contiguous `Vec<f32>` in the same pixel order
/// as the source raster.
///
/// Convenience functions [`streaming_hillshade`], [`streaming_slope`], and
/// [`streaming_focal_mean`] wrap the underlying buffer-based algorithms using
/// the conversion helpers defined here.
pub mod chunk;
pub mod iter;

pub use chunk::{Chunk, InMemoryRasterSource, RasterError, RasterSource};
pub use iter::ChunkIterator;

use crate::raster::focal::{BoundaryMode, WindowShape, focal_mean};
use crate::raster::hillshade::{HillshadeParams, hillshade};
use crate::raster::slope_aspect::slope;

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ---------------------------------------------------------------------------
// Internal conversion helpers
// ---------------------------------------------------------------------------

/// Creates a `RasterBuffer` from a flat `Vec<f32>` slice with given dimensions.
fn buffer_from_f32_slice(
    data: &[f32],
    width: usize,
    height: usize,
) -> Result<RasterBuffer, RasterError> {
    RasterBuffer::from_typed_vec(width, height, data.to_vec(), RasterDataType::Float32)
        .map_err(|e| RasterError::AlgorithmError(e.to_string()))
}

/// Extracts all pixel values from a `RasterBuffer` as `Vec<f32>`.
fn f32_vec_from_buffer(buf: &RasterBuffer) -> Result<Vec<f32>, RasterError> {
    buf.as_slice::<f32>()
        .map(|s| s.to_vec())
        .map_err(|e| RasterError::AlgorithmError(e.to_string()))
}

// ---------------------------------------------------------------------------
// ChunkedRaster
// ---------------------------------------------------------------------------

/// A tiled view of a [`RasterSource`] that yields overlapping chunks with halo
/// (ghost / border) cells for seamless neighbourhood processing.
///
/// Create with [`ChunkedRaster::new`] and iterate with [`ChunkedRaster::iter`].
pub struct ChunkedRaster<R: RasterSource> {
    /// The underlying raster data source.
    pub source: R,
    /// Total width of the source raster in pixels.
    pub total_width: usize,
    /// Total height of the source raster in pixels.
    pub total_height: usize,
    /// Target core width and height of each chunk (actual edge chunks may be smaller).
    pub chunk_size: usize,
    /// Number of halo pixels added on each side of every chunk.
    pub halo: usize,
}

impl<R: RasterSource> ChunkedRaster<R> {
    /// Creates a new `ChunkedRaster`.
    ///
    /// # Errors
    ///
    /// Returns [`RasterError::InvalidChunkSize`] if `chunk_size == 0`.
    pub fn new(source: R, chunk_size: usize, halo: usize) -> Result<Self, RasterError> {
        if chunk_size == 0 {
            return Err(RasterError::InvalidChunkSize);
        }
        let total_width = source.width();
        let total_height = source.height();
        Ok(Self {
            source,
            total_width,
            total_height,
            chunk_size,
            halo,
        })
    }

    /// Returns a [`ChunkIterator`] that yields all chunks in raster-scan order.
    pub fn iter(&self) -> ChunkIterator<'_, R> {
        ChunkIterator::new(self)
    }
}

// ---------------------------------------------------------------------------
// process_streaming
// ---------------------------------------------------------------------------

/// Applies a kernel function chunk-by-chunk and stitches the results into a
/// full `Vec<f32>` output raster.
///
/// The kernel closure receives a [`Chunk`] (which includes surrounding halo
/// cells in its `data` field) and **must** return exactly
/// `chunk.width * chunk.height` values (the core region, without halo) in
/// row-major order.  Results are assembled at the correct position in the
/// output raster.
///
/// # Errors
///
/// Propagates any [`RasterError`] emitted by the iterator.  Also panics
/// (via `assert_eq!`) if the kernel returns a vector of the wrong length —
/// this is an API contract violation in the caller's closure.
pub fn process_streaming<R, F>(
    raster: &ChunkedRaster<R>,
    mut kernel: F,
) -> Result<Vec<f32>, RasterError>
where
    R: RasterSource,
    F: FnMut(&Chunk) -> Result<Vec<f32>, RasterError>,
{
    let mut output = vec![0.0_f32; raster.total_width * raster.total_height];

    for chunk_result in raster.iter() {
        let chunk = chunk_result?;
        let chunk_output = kernel(&chunk)?;

        assert_eq!(
            chunk_output.len(),
            chunk.width * chunk.height,
            "streaming kernel must return width×height values (no halo)"
        );

        // Scatter the per-chunk output into the correct position in `output`.
        for row in 0..chunk.height {
            for col in 0..chunk.width {
                let src_idx = row * chunk.width + col;
                let dst_x = chunk.x + col;
                let dst_y = chunk.y + row;
                if dst_x < raster.total_width && dst_y < raster.total_height {
                    output[dst_y * raster.total_width + dst_x] = chunk_output[src_idx];
                }
            }
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Inner-region extraction helper
// ---------------------------------------------------------------------------

/// Extracts the inner sub-region from a 2-D output array, stripping off `halo`
/// rows/columns on each side.
///
/// `data` must have dimensions `full_w × full_h`.  The returned vector has
/// `inner_w × inner_h` elements.  Indices that fall outside `[0, full_w) ×
/// [0, full_h)` are clamped and filled with `0.0`.
pub fn extract_inner(
    data: &[f32],
    full_w: usize,
    full_h: usize,
    halo: usize,
    inner_w: usize,
    inner_h: usize,
) -> Vec<f32> {
    let mut inner = Vec::with_capacity(inner_w * inner_h);
    for row in halo..(halo + inner_h) {
        for col in halo..(halo + inner_w) {
            if row < full_h && col < full_w {
                inner.push(data[row * full_w + col]);
            } else {
                inner.push(0.0_f32);
            }
        }
    }
    inner
}

// ---------------------------------------------------------------------------
// streaming_hillshade
// ---------------------------------------------------------------------------

/// Processes hillshade in a streaming (chunk-by-chunk) fashion.
///
/// Each chunk is inflated by `raster.halo` cells on all sides before the
/// hillshade algorithm runs; only the inner `width × height` result pixels are
/// kept and assembled into the full output.  Using `halo ≥ 1` ensures correct
/// Horn-gradient computation at chunk boundaries.
///
/// # Errors
///
/// Returns any [`RasterError`] produced by chunk iteration or the hillshade
/// algorithm.
pub fn streaming_hillshade<R: RasterSource>(
    raster: &ChunkedRaster<R>,
    azimuth: f64,
    altitude: f64,
) -> Result<Vec<f32>, RasterError> {
    process_streaming(raster, |chunk| {
        let full_w = chunk.full_width();
        let full_h = chunk.full_height();

        let dem = buffer_from_f32_slice(&chunk.data, full_w, full_h)?;

        // Use validated HillshadeParams; clamp azimuth/altitude to valid ranges
        // so callers do not have to worry about boundary values.
        let params = HillshadeParams::new(azimuth.clamp(0.0, 360.0), altitude.clamp(0.0, 90.0));

        let result_buf =
            hillshade(&dem, params).map_err(|e| RasterError::AlgorithmError(e.to_string()))?;

        let result_data = f32_vec_from_buffer(&result_buf)?;

        // Strip the halo rows/columns to obtain the core region.
        Ok(extract_inner(
            &result_data,
            full_w,
            full_h,
            chunk.halo,
            chunk.width,
            chunk.height,
        ))
    })
}

// ---------------------------------------------------------------------------
// streaming_slope
// ---------------------------------------------------------------------------

/// Processes slope in a streaming (chunk-by-chunk) fashion.
///
/// Uses `pixel_size = 1.0` and `z_factor = 1.0`.  Halo cells prevent
/// discontinuities at chunk borders.
///
/// # Errors
///
/// Returns any [`RasterError`] produced by chunk iteration or the slope
/// algorithm.
pub fn streaming_slope<R: RasterSource>(
    raster: &ChunkedRaster<R>,
) -> Result<Vec<f32>, RasterError> {
    process_streaming(raster, |chunk| {
        let full_w = chunk.full_width();
        let full_h = chunk.full_height();

        let dem = buffer_from_f32_slice(&chunk.data, full_w, full_h)?;

        let result_buf =
            slope(&dem, 1.0, 1.0).map_err(|e| RasterError::AlgorithmError(e.to_string()))?;

        let result_data = f32_vec_from_buffer(&result_buf)?;

        Ok(extract_inner(
            &result_data,
            full_w,
            full_h,
            chunk.halo,
            chunk.width,
            chunk.height,
        ))
    })
}

// ---------------------------------------------------------------------------
// streaming_focal_mean
// ---------------------------------------------------------------------------

/// Processes focal mean in a streaming (chunk-by-chunk) fashion using a
/// circular window of the given `radius`.
///
/// The chunk halo must be at least `ceil(radius)` pixels to avoid boundary
/// artefacts.  The caller is responsible for selecting a suitable halo when
/// constructing the [`ChunkedRaster`].
///
/// # Errors
///
/// Returns any [`RasterError`] produced by chunk iteration or the focal-mean
/// algorithm.
pub fn streaming_focal_mean<R: RasterSource>(
    raster: &ChunkedRaster<R>,
    radius: usize,
) -> Result<Vec<f32>, RasterError> {
    process_streaming(raster, |chunk| {
        let full_w = chunk.full_width();
        let full_h = chunk.full_height();

        let src_buf = buffer_from_f32_slice(&chunk.data, full_w, full_h)?;

        // Build a circular window matching the requested radius.
        let window = WindowShape::circular(radius as f64)
            .map_err(|e| RasterError::AlgorithmError(e.to_string()))?;

        let result_buf = focal_mean(&src_buf, &window, &BoundaryMode::Edge)
            .map_err(|e| RasterError::AlgorithmError(e.to_string()))?;

        let result_data = f32_vec_from_buffer(&result_buf)?;

        Ok(extract_inner(
            &result_data,
            full_w,
            full_h,
            chunk.halo,
            chunk.width,
            chunk.height,
        ))
    })
}
