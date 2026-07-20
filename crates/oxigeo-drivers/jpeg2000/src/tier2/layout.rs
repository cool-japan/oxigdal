//! Tile-component / resolution-level / subband / code-block geometry
//!
//! This module builds the *real* JPEG2000 spatial layout of a tile-component
//! from the COD parameters (number of decomposition levels and nominal
//! code-block size), so that Tier-2 packet parsing can slice the correct
//! code-block contribution bytes for each (resolution, subband, code-block)
//! rather than assuming a flat even-division grid.
//!
//! # Structure (ISO 15444-1 Annex B / Annex E)
//!
//! A tile-component with `N` decomposition levels has `N + 1` *resolution
//! levels*:
//!
//! - Resolution `0` holds a single `LL` subband (the coarsest approximation).
//! - Resolution `r` (`1..=N`) holds three detail subbands `HL, LH, HH` at
//!   decomposition level `nb = N - r + 1`.
//!
//! Each subband is partitioned into a grid of code-blocks of nominal size
//! `cbw × cbh`.  With the (mandatory here) maximum precinct size, there is
//! exactly one precinct per resolution level that spans the whole resolution,
//! so the code-block grid of a subband is simply `ceil(nw / cbw)` by
//! `ceil(nh / cbh)`.

use crate::tier1::SubbandType;

/// Ceiling division `ceil(a / b)` for `b > 0`.
#[inline]
fn div_ceil(a: usize, b: usize) -> usize {
    if b == 0 { 0 } else { a.div_ceil(b) }
}

/// Number of samples of a subband along one axis.
///
/// For a tile-component extent `size` (with tile-component origin at 0), a
/// subband at decomposition level `nb >= 1` with orientation bit `ob`
/// (0 = low-pass along this axis, 1 = high-pass) has
/// `ceil((size - ob * 2^(nb-1)) / 2^nb)` samples (ISO 15444-1 Eq. B-15 with
/// `tbx0 = tby0 = 0`).
pub fn band_dim(size: usize, ob: u32, nb: u32) -> usize {
    debug_assert!(nb >= 1, "band_dim requires nb >= 1");
    let pow = 1usize << nb;
    let half = 1usize << (nb - 1);
    let offset = (ob as usize) * half;
    if size > offset {
        div_ceil(size - offset, pow)
    } else {
        0
    }
}

/// Reduced dimension of a resolution level: `ceil(size / 2^(N - r))`.
pub fn resolution_dim(size: usize, num_levels: u32, r: u32) -> usize {
    let shift = num_levels - r;
    div_ceil(size, 1usize << shift)
}

/// Geometry of one subband within a resolution level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubbandLayout {
    /// Subband orientation (LL / HL / LH / HH).
    pub subband_type: SubbandType,
    /// Subband width in samples.
    pub width: usize,
    /// Subband height in samples.
    pub height: usize,
    /// Number of code-blocks along X.
    pub cblk_nx: usize,
    /// Number of code-blocks along Y.
    pub cblk_ny: usize,
    /// Index of this subband in the QCD step-size / exponent list.
    pub exponent_index: usize,
}

impl SubbandLayout {
    /// Dimensions of code-block `(bx, by)` (last blocks may be smaller than
    /// the nominal `cbw × cbh`).
    pub fn cblk_dims(&self, bx: usize, by: usize, cbw: usize, cbh: usize) -> (usize, usize) {
        let w = cbw.min(self.width.saturating_sub(bx * cbw));
        let h = cbh.min(self.height.saturating_sub(by * cbh));
        (w, h)
    }
}

/// Geometry of one resolution level (its subbands, in packet scan order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionLayout {
    /// Reduced width of this resolution level.
    pub width: usize,
    /// Reduced height of this resolution level.
    pub height: usize,
    /// Subbands present at this resolution, in the order they appear inside a
    /// packet (`LL` alone at resolution 0; `HL, LH, HH` otherwise).
    pub subbands: Vec<SubbandLayout>,
}

/// Full geometry of a tile-component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileComponentLayout {
    /// Tile-component width in samples.
    pub width: usize,
    /// Tile-component height in samples.
    pub height: usize,
    /// Nominal code-block width.
    pub cbw: usize,
    /// Nominal code-block height.
    pub cbh: usize,
    /// Number of decomposition levels.
    pub num_levels: u32,
    /// Resolution levels, index `0..=num_levels`.
    pub resolutions: Vec<ResolutionLayout>,
}

impl TileComponentLayout {
    /// Number of resolution levels (`num_levels + 1`).
    pub fn num_resolutions(&self) -> usize {
        self.resolutions.len()
    }
}

/// Build the tile-component layout from COD parameters.
///
/// * `comp_w`, `comp_h` — tile-component extent in samples.
/// * `num_levels` — number of wavelet decomposition levels (`N`).
/// * `cbw`, `cbh` — nominal code-block dimensions (already `2^(xcb+2)` etc.).
pub fn build_tile_component_layout(
    comp_w: usize,
    comp_h: usize,
    num_levels: u32,
    cbw: usize,
    cbh: usize,
) -> TileComponentLayout {
    let cbw = cbw.max(1);
    let cbh = cbh.max(1);
    let mut resolutions = Vec::with_capacity(num_levels as usize + 1);

    // Resolution 0: the coarsest LL subband.
    let ll_w = resolution_dim(comp_w, num_levels, 0);
    let ll_h = resolution_dim(comp_h, num_levels, 0);
    resolutions.push(ResolutionLayout {
        width: ll_w,
        height: ll_h,
        subbands: vec![SubbandLayout {
            subband_type: SubbandType::Ll,
            width: ll_w,
            height: ll_h,
            cblk_nx: div_ceil(ll_w, cbw),
            cblk_ny: div_ceil(ll_h, cbh),
            exponent_index: 0,
        }],
    });

    // Resolutions 1..=N: three detail subbands each.
    for r in 1..=num_levels {
        let nb = num_levels - r + 1; // decomposition level of these bands
        let res_w = resolution_dim(comp_w, num_levels, r);
        let res_h = resolution_dim(comp_h, num_levels, r);
        // Exponent indices: 0 = LL, then HL,LH,HH per resolution outward.
        let base = 1 + 3 * (r as usize - 1);
        let mut subbands = Vec::with_capacity(3);
        for (offset, (subband_type, obx, oby)) in [
            (SubbandType::Hl, 1u32, 0u32),
            (SubbandType::Lh, 0u32, 1u32),
            (SubbandType::Hh, 1u32, 1u32),
        ]
        .into_iter()
        .enumerate()
        {
            let w = band_dim(comp_w, obx, nb);
            let h = band_dim(comp_h, oby, nb);
            subbands.push(SubbandLayout {
                subband_type,
                width: w,
                height: h,
                cblk_nx: div_ceil(w, cbw),
                cblk_ny: div_ceil(h, cbh),
                exponent_index: base + offset,
            });
        }
        resolutions.push(ResolutionLayout {
            width: res_w,
            height: res_h,
            subbands,
        });
    }

    TileComponentLayout {
        width: comp_w,
        height: comp_h,
        cbw,
        cbh,
        num_levels,
        resolutions,
    }
}

/// Total magnitude bit-planes `Mb` of a subband (ISO 15444-1 §E.1, Eq. E-2):
/// `Mb = G + εb - 1`.
///
/// A robust floor is applied for degenerate parameter sets (e.g. synthetic
/// streams that signal `G = εb = 0`): in that case a precision-derived budget
/// is used so Tier-1 still has a sane number of bit-planes to decode.
pub fn subband_num_bitplanes(guard_bits: u8, exponent: u8, precision: u8) -> usize {
    let mb = i32::from(guard_bits) + i32::from(exponent) - 1;
    if mb >= 1 {
        mb as usize
    } else {
        usize::from(precision).max(1) + 1
    }
}

/// Number of coded magnitude bit-planes for a code-block:
/// `Mb - zbp`, floored at 1 whenever the code-block carries data.
pub fn code_block_bitplanes(guard_bits: u8, exponent: u8, precision: u8, zbp: u32) -> usize {
    subband_num_bitplanes(guard_bits, exponent, precision)
        .saturating_sub(zbp as usize)
        .max(1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_band_dim_basic() {
        // 4-wide tile-component, one decomposition level (nb = 1).
        // LL (ob=0): ceil(4/2) = 2 ; HL/HH (ob=1): ceil((4-1)/2) = 2.
        assert_eq!(band_dim(4, 0, 1), 2);
        assert_eq!(band_dim(4, 1, 1), 2);
        // 5-wide: LL ceil(5/2)=3 ; H ceil((5-1)/2)=2.
        assert_eq!(band_dim(5, 0, 1), 3);
        assert_eq!(band_dim(5, 1, 1), 2);
    }

    #[test]
    fn test_layout_num_levels_zero_single_ll() {
        let layout = build_tile_component_layout(4, 4, 0, 16, 16);
        assert_eq!(layout.num_resolutions(), 1);
        let res0 = &layout.resolutions[0];
        assert_eq!(res0.subbands.len(), 1);
        let ll = &res0.subbands[0];
        assert_eq!(ll.subband_type, SubbandType::Ll);
        assert_eq!((ll.width, ll.height), (4, 4));
        // 4x4 subband, 16x16 code-blocks => single code-block.
        assert_eq!((ll.cblk_nx, ll.cblk_ny), (1, 1));
        assert_eq!(ll.exponent_index, 0);
    }

    #[test]
    fn test_layout_one_level_subbands() {
        // 4x4, one decomposition level: res0 LL 2x2, res1 HL/LH/HH 2x2 each.
        let layout = build_tile_component_layout(4, 4, 1, 16, 16);
        assert_eq!(layout.num_resolutions(), 2);
        let res0 = &layout.resolutions[0];
        assert_eq!((res0.width, res0.height), (2, 2));
        assert_eq!(res0.subbands[0].subband_type, SubbandType::Ll);
        assert_eq!((res0.subbands[0].width, res0.subbands[0].height), (2, 2));

        let res1 = &layout.resolutions[1];
        assert_eq!((res1.width, res1.height), (4, 4));
        assert_eq!(res1.subbands.len(), 3);
        assert_eq!(res1.subbands[0].subband_type, SubbandType::Hl);
        assert_eq!(res1.subbands[1].subband_type, SubbandType::Lh);
        assert_eq!(res1.subbands[2].subband_type, SubbandType::Hh);
        for sb in &res1.subbands {
            assert_eq!((sb.width, sb.height), (2, 2));
        }
        // Exponent indices are 0 (LL), then 1,2,3 for the res1 detail bands.
        assert_eq!(res1.subbands[0].exponent_index, 1);
        assert_eq!(res1.subbands[1].exponent_index, 2);
        assert_eq!(res1.subbands[2].exponent_index, 3);
    }

    #[test]
    fn test_layout_reconstruction_dims_sum() {
        // For a two-level 8x8 decomposition every sample must be covered once.
        let layout = build_tile_component_layout(8, 8, 2, 64, 64);
        assert_eq!(layout.num_resolutions(), 3);
        // res0 LL 2x2 ; res1 bands 2x2 ; res2 bands 4x4.
        assert_eq!(
            (layout.resolutions[0].width, layout.resolutions[0].height),
            (2, 2)
        );
        assert_eq!(
            (layout.resolutions[1].width, layout.resolutions[1].height),
            (4, 4)
        );
        assert_eq!(
            (layout.resolutions[2].width, layout.resolutions[2].height),
            (8, 8)
        );
        // Total distinct coefficients across all subbands equals 8*8.
        let mut total = 0usize;
        for res in &layout.resolutions {
            for sb in &res.subbands {
                total += sb.width * sb.height;
            }
        }
        assert_eq!(total, 64);
    }

    #[test]
    fn test_multiple_code_blocks_grid() {
        // 32x16 LL with 16x16 code-blocks => 2x1 code-block grid.
        let layout = build_tile_component_layout(32, 16, 0, 16, 16);
        let ll = &layout.resolutions[0].subbands[0];
        assert_eq!((ll.cblk_nx, ll.cblk_ny), (2, 1));
        assert_eq!(ll.cblk_dims(0, 0, 16, 16), (16, 16));
        assert_eq!(ll.cblk_dims(1, 0, 16, 16), (16, 16));
    }

    #[test]
    fn test_code_block_bitplanes_floor() {
        // G=2, eps=8, precision=8 => Mb = 9 ; zbp 3 => 6 planes.
        assert_eq!(code_block_bitplanes(2, 8, 8, 3), 6);
        // Degenerate G=0,eps=0 falls back to precision+1 budget.
        assert_eq!(subband_num_bitplanes(0, 0, 8), 9);
        // Never returns 0 when data is present.
        assert_eq!(code_block_bitplanes(2, 1, 8, 100), 1);
    }
}
