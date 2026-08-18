//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

/// Composite one decoded tile's level-shifted component samples into the RGB
/// output raster.
///
/// `canvas` is the full-image RGB buffer (`out_w` pixels wide). The tile
/// occupies output pixels `[ox, tx1) × [oy, ty1)`; within the tile's own sample
/// buffers (`shifted`, one per component, each `tile_width`-strided) the local
/// coordinate is `(px - ox, py - oy)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_tile_samples(
    canvas: &mut [u8],
    out_w: usize,
    shifted: &[Vec<u8>],
    num_components: usize,
    tile_width: usize,
    ox: usize,
    oy: usize,
    tx1: usize,
    ty1: usize,
) {
    for py in oy..ty1 {
        for px in ox..tx1 {
            let local_idx = (py - oy) * tile_width + (px - ox);
            let dst = (py * out_w + px) * 3;
            if dst + 2 >= canvas.len() {
                continue;
            }
            if num_components >= 3 && shifted.len() >= 3 {
                canvas[dst] = shifted[0].get(local_idx).copied().unwrap_or(128);
                canvas[dst + 1] = shifted[1].get(local_idx).copied().unwrap_or(128);
                canvas[dst + 2] = shifted[2].get(local_idx).copied().unwrap_or(128);
            } else if !shifted.is_empty() {
                let gray = shifted[0].get(local_idx).copied().unwrap_or(128);
                canvas[dst] = gray;
                canvas[dst + 1] = gray;
                canvas[dst + 2] = gray;
            }
        }
    }
}
