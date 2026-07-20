use super::ChunkedRaster;
use super::chunk::{Chunk, RasterError, RasterSource};

/// An iterator over the chunks of a [`ChunkedRaster`].
///
/// Yields `Result<Chunk, RasterError>` values in raster-scan order (row by row,
/// left to right).  Each chunk's `data` includes the surrounding halo cells,
/// zero-padded where the window would extend beyond the source raster boundary.
///
/// Edge chunks (those at the right or bottom boundary of the raster) will be
/// smaller than `chunk_size` in the truncated dimension.
///
/// # Halo data layout
///
/// `chunk.data` has dimensions `(chunk.width + 2 * halo) × (chunk.height + 2 * halo)`.
/// Row 0 is the top halo row, row `height + halo` is the last halo row.  Within
/// each row column 0 is the left halo column.  Pixels that fall outside the source
/// raster are `0.0`.  In particular, when the chunk is at the top-left corner of
/// the raster (`x=0, y=0`) the entire top halo row and left halo column contain
/// zeros.
pub struct ChunkIterator<'r, R: RasterSource> {
    raster: &'r ChunkedRaster<R>,
    current_x: usize,
    current_y: usize,
}

impl<'r, R: RasterSource> ChunkIterator<'r, R> {
    /// Creates a new `ChunkIterator` positioned at the top-left corner.
    pub fn new(raster: &'r ChunkedRaster<R>) -> Self {
        Self {
            raster,
            current_x: 0,
            current_y: 0,
        }
    }
}

impl<'r, R: RasterSource> Iterator for ChunkIterator<'r, R> {
    type Item = Result<Chunk, RasterError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Exhausted once we've advanced past the bottom of the raster.
        if self.current_y >= self.raster.total_height {
            return None;
        }

        let x = self.current_x;
        let y = self.current_y;
        let halo = self.raster.halo;

        // Core region dimensions — smaller for right/bottom edge chunks.
        let w = self.raster.chunk_size.min(self.raster.total_width - x);
        let h = self.raster.chunk_size.min(self.raster.total_height - y);

        // Full data buffer dimensions (core + halo on both sides).
        let full_w = w + 2 * halo;
        let full_h = h + 2 * halo;

        // Build the halo-padded data buffer manually.
        //
        // For each position (row, col) in the full_h × full_w buffer, compute
        // the corresponding source coordinate.  Positions that map outside the
        // source raster [0, total_width) × [0, total_height) are filled with 0.0.
        //
        // We do this by asking the source for the largest contiguous read window
        // that stays in-bounds, then scatter-write it into the correct position of
        // the output buffer (filling the rest with zeros).
        let mut data = vec![0.0_f32; full_w * full_h];

        // Source window: the slice of the source raster that overlaps with the
        // halo-extended chunk window.
        //
        // For each side: the halo extends `halo` pixels beyond the core origin.
        // If the core origin is < halo (e.g. x=0, halo=1) then part of the halo
        // falls outside the raster and must stay zero.
        //
        // `src_left` = actual source x of the first in-bounds halo pixel.
        // `left_pad` = number of zero columns to the left of `src_left` in `data`.
        let src_left = x.saturating_sub(halo);
        let src_top = y.saturating_sub(halo);
        let left_pad = halo.saturating_sub(x); // zero-pad columns on the left
        let top_pad = halo.saturating_sub(y); // zero-pad rows on the top

        // How many source columns / rows are actually available?
        let src_right = (x + w + halo).min(self.raster.total_width);
        let src_bottom = (y + h + halo).min(self.raster.total_height);
        let src_w = src_right.saturating_sub(src_left);
        let src_h = src_bottom.saturating_sub(src_top);

        if src_w > 0 && src_h > 0 {
            // Read the in-bounds source region.
            let src_data = match self
                .raster
                .source
                .read_window(src_left, src_top, src_w, src_h)
            {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };

            // Scatter-write into the correct position in `data`.
            for row in 0..src_h {
                let dst_row = row + top_pad;
                for col in 0..src_w {
                    let dst_col = col + left_pad;
                    data[dst_row * full_w + dst_col] = src_data[row * src_w + col];
                }
            }
        }

        // Advance the position cursor for the next call.
        self.current_x += w;
        if self.current_x >= self.raster.total_width {
            self.current_x = 0;
            self.current_y += h;
        }

        Some(Ok(Chunk {
            x,
            y,
            width: w,
            height: h,
            halo,
            data,
        }))
    }
}
