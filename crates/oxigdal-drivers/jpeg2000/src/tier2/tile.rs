//! Tier-2 tile decoding: drive the progression order over the real code-block
//! layout, parse each packet, and route the sliced code-block bytes to Tier-1.
//!
//! This is the integration that replaces the former naive "even-division" byte
//! split.  For every packet emitted by [`ProgressionIterator`] (in the COD
//! progression order) it uses [`parse_precinct_packet`] to recover the exact
//! per-code-block contribution byte ranges, decodes each code block with the
//! EBCOT Tier-1 decoder, places the coefficients into the correct subband, and
//! finally synthesises the spatial tile-component samples with the inverse 5/3
//! wavelet transform (interleaved multi-level lifting).
//!
//! # Supported inputs
//!
//! - Progression orders **LRCP** and **RLCP** (single tile-part).
//! - **Single quality layer** (`num_layers == 1`).
//! - Maximum-size precincts (one precinct per resolution level), i.e. the
//!   default `Scod` with custom precincts disabled (enforced at COD parse).
//! - Reversible 5/3 wavelet.
//!
//! Multi-layer streams and the RPCL/PCRL/CPRL progression orders return a typed
//! [`Jpeg2000Error::UnsupportedFeature`] rather than mis-slicing silently.

use crate::codestream::{ProgressionOrder, Quantization};
use crate::error::{Jpeg2000Error, Result};
use crate::tier1::CodeBlockDecoder;
use crate::tier2::layout::{
    TileComponentLayout, build_tile_component_layout, code_block_bitplanes,
};
use crate::tier2::packet::parse_precinct_packet;
use crate::tier2::progression::ProgressionIterator;
use crate::wavelet::Reversible53;

/// Per-component decode inputs.
#[derive(Debug, Clone, Copy)]
pub struct TileComponentInput {
    /// Tile-component width in samples.
    pub comp_w: usize,
    /// Tile-component height in samples.
    pub comp_h: usize,
    /// Component bit depth (used for the Tier-1 bit-plane budget).
    pub precision: u8,
}

/// Parameters controlling a tile decode.
pub struct TileDecodeParams<'a> {
    /// One entry per component.
    pub components: &'a [TileComponentInput],
    /// Number of wavelet decomposition levels.
    pub num_levels: u32,
    /// Nominal code-block width in samples.
    pub cbw: usize,
    /// Nominal code-block height in samples.
    pub cbh: usize,
    /// Progression order from the COD marker.
    pub progression: ProgressionOrder,
    /// Number of quality layers from the COD marker.
    pub num_layers: u16,
    /// Number of guard bits from the QCD marker.
    pub guard_bits: u8,
    /// Optional quantization (for subband exponents / bit-plane counts).
    pub quantization: Option<&'a Quantization>,
    /// Whether SOP markers precede each packet.
    pub has_sop: bool,
    /// Whether EPH markers terminate each packet header.
    pub has_eph: bool,
}

/// Decode all components of a single tile from its packet-data byte range.
///
/// Returns one vector of spatial samples (`comp_w * comp_h`) per component,
/// after the inverse wavelet transform but before any colour transform.
pub fn decode_tile_components(
    tile_data: &[u8],
    params: &TileDecodeParams<'_>,
) -> Result<Vec<Vec<i32>>> {
    let num_components = params.components.len();

    // --- Fail loud on structurally unsupported inputs (no silent mis-slice) ---
    if params.num_layers > 1 {
        return Err(Jpeg2000Error::UnsupportedFeature(format!(
            "multi-layer Tier-2 decoding not supported ({} layers); \
             re-encode with a single quality layer",
            params.num_layers
        )));
    }
    match params.progression {
        ProgressionOrder::Lrcp | ProgressionOrder::Rlcp => {}
        other => {
            return Err(Jpeg2000Error::UnsupportedFeature(format!(
                "progression order {:?} not supported by the Tier-2 decoder \
                 (only LRCP and RLCP)",
                other
            )));
        }
    }

    // Build per-component layouts and zeroed subband coefficient storage.
    let layouts: Vec<TileComponentLayout> = params
        .components
        .iter()
        .map(|c| {
            build_tile_component_layout(
                c.comp_w,
                c.comp_h,
                params.num_levels,
                params.cbw,
                params.cbh,
            )
        })
        .collect();

    // storage[component][resolution][subband] = coefficients (subband raster).
    let mut storage: Vec<Vec<Vec<Vec<i32>>>> = layouts
        .iter()
        .map(|layout| {
            layout
                .resolutions
                .iter()
                .map(|res| {
                    res.subbands
                        .iter()
                        .map(|sb| vec![0i32; sb.width * sb.height])
                        .collect()
                })
                .collect()
        })
        .collect();

    // With no packet data (e.g. empty SOD), every coefficient stays zero.
    if !tile_data.is_empty() {
        decode_packets(tile_data, params, &layouts, &mut storage)?;
    }

    // Synthesise spatial samples for each component.
    let mut components = Vec::with_capacity(num_components);
    for (c, layout) in layouts.iter().enumerate() {
        components.push(synthesize_component(layout, &storage[c])?);
    }
    Ok(components)
}

/// Drive the progression iterator, parse packets and run Tier-1 on each
/// included code block.
///
/// Packet-level failures are concealed (coefficients left zeroed, with a
/// warning) and stop further packet parsing, because packet boundaries after a
/// malformed header are unknowable. This is error concealment of undecodable
/// bytes, never a silent mis-slice of valid data.
fn decode_packets(
    tile_data: &[u8],
    params: &TileDecodeParams<'_>,
    layouts: &[TileComponentLayout],
    storage: &mut [Vec<Vec<Vec<i32>>>],
) -> Result<()> {
    let num_components = layouts.len();
    let num_resolutions = params.num_levels as usize + 1;
    // Maximum precinct size => exactly one precinct per resolution level.
    let num_precincts = vec![1u32; num_resolutions];

    let iterator = ProgressionIterator::new(
        params.progression,
        1, // single layer (validated by caller)
        num_resolutions as u8,
        num_components as u16,
        &num_precincts,
    );

    let mut pos = 0usize;

    for address in iterator {
        let c = address.component as usize;
        let r = address.resolution as usize;
        if c >= num_components || r >= layouts[c].num_resolutions() {
            continue;
        }

        if pos >= tile_data.len() {
            // Ran out of packet bytes early: remaining coefficients stay zero.
            break;
        }

        // Optional SOP (start-of-packet) marker: 0xFF91 Lsop(2) Nsop(2) = 6 bytes.
        if params.has_sop
            && pos + 2 <= tile_data.len()
            && tile_data[pos] == 0xFF
            && tile_data[pos + 1] == 0x91
        {
            pos = (pos + 6).min(tile_data.len());
        }

        let res_layout = &layouts[c].resolutions[r];
        let grids: Vec<(u32, u32)> = res_layout
            .subbands
            .iter()
            .map(|sb| (sb.cblk_nx as u32, sb.cblk_ny as u32))
            .collect();

        let packet_slice = &tile_data[pos..];
        let (consumed, contributions) =
            match parse_precinct_packet(packet_slice, &grids, address.layer, params.has_eph) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        "JPEG2000 Tier-2 packet parse failed at component={} resolution={} \
                         (offset {}): {}; concealing remaining packets with zeros",
                        c,
                        r,
                        pos,
                        e
                    );
                    break;
                }
            };

        let precision = params.components[c].precision;
        for (sbi, sub_contrib) in contributions.iter().enumerate() {
            let sb = &res_layout.subbands[sbi];
            let exponent = params
                .quantization
                .map(|q| q.subband_exponent(sb.exponent_index))
                .unwrap_or(0);

            for by in 0..sb.cblk_ny {
                for bx in 0..sb.cblk_nx {
                    let idx = by * sb.cblk_nx + bx;
                    let Some(contrib) = sub_contrib.get(idx) else {
                        continue;
                    };
                    if !contrib.included || contrib.data_len == 0 {
                        continue;
                    }
                    let (cw, ch) = sb.cblk_dims(bx, by, params.cbw, params.cbh);
                    if cw == 0 || ch == 0 {
                        continue;
                    }

                    let start = contrib.data_offset;
                    let end = start + contrib.data_len;
                    if end > packet_slice.len() {
                        // Should not happen (parser bounds-checks), but never slice OOB.
                        continue;
                    }
                    let block_bytes = &packet_slice[start..end];

                    let num_bitplanes =
                        code_block_bitplanes(params.guard_bits, exponent, precision, contrib.zbp);
                    let decoder =
                        CodeBlockDecoder::with_subband(cw, ch, num_bitplanes, sb.subband_type);
                    let coeffs = match decoder.decode(block_bytes) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                "JPEG2000 Tier-1 decode failed (component={} resolution={} \
                                 subband={:?} block=({},{})): {}; substituting zeros",
                                c,
                                r,
                                sb.subband_type,
                                bx,
                                by,
                                e
                            );
                            vec![0i32; cw * ch]
                        }
                    };

                    // Place the decoded code block into its subband buffer.
                    let band = &mut storage[c][r][sbi];
                    let band_w = sb.width;
                    let base_x = bx * params.cbw;
                    let base_y = by * params.cbh;
                    for ly in 0..ch {
                        for lx in 0..cw {
                            let src = ly * cw + lx;
                            let x = base_x + lx;
                            let y = base_y + ly;
                            let dst = y * band_w + x;
                            if src < coeffs.len() && dst < band.len() {
                                band[dst] = coeffs[src];
                            }
                        }
                    }
                }
            }
        }

        pos += consumed;
    }

    Ok(())
}

/// Reassemble a tile-component's spatial samples from its subband coefficients
/// using the interleaved multi-level inverse 5/3 wavelet transform.
fn synthesize_component(
    layout: &TileComponentLayout,
    subbands: &[Vec<Vec<i32>>],
) -> Result<Vec<i32>> {
    // Resolution 0 is a single LL subband.
    let mut cur = subbands
        .first()
        .and_then(|res0| res0.first())
        .cloned()
        .unwrap_or_default();

    let full = layout.width * layout.height;

    if layout.num_levels == 0 {
        // No transform: LL is already the tile-component (comp_w x comp_h).
        cur.resize(full, 0);
        return Ok(cur);
    }

    // `r` indexes three correlated structures (this resolution, the previous
    // resolution for LL dims, and this resolution's subband coefficients), so a
    // plain range loop is the clearest form here.
    #[allow(clippy::needless_range_loop)]
    for r in 1..layout.resolutions.len() {
        let res = &layout.resolutions[r];
        let dims = (res.width, res.height);
        let ll_dims = (
            layout.resolutions[r - 1].width,
            layout.resolutions[r - 1].height,
        );
        let mut grid = vec![0i32; dims.0 * dims.1];

        // LL (previous synthesis result) -> even x, even y.
        interleave(&mut grid, dims, &cur, ll_dims, (0, 0));
        // HL (subband 0) -> odd x, even y.
        if let Some(hl) = res.subbands.first() {
            interleave(
                &mut grid,
                dims,
                &subbands[r][0],
                (hl.width, hl.height),
                (1, 0),
            );
        }
        // LH (subband 1) -> even x, odd y.
        if let Some(lh) = res.subbands.get(1) {
            interleave(
                &mut grid,
                dims,
                &subbands[r][1],
                (lh.width, lh.height),
                (0, 1),
            );
        }
        // HH (subband 2) -> odd x, odd y.
        if let Some(hh) = res.subbands.get(2) {
            interleave(
                &mut grid,
                dims,
                &subbands[r][2],
                (hh.width, hh.height),
                (1, 1),
            );
        }

        Reversible53::inverse_2d(&mut grid, dims.0, dims.1)?;
        cur = grid;
    }

    cur.resize(full, 0);
    Ok(cur)
}

/// Scatter a subband (`band_dims`) into `grid` (`grid_dims`) at parity offset
/// `(xoff, yoff)` — the inverse of the standard deinterleave step.
fn interleave(
    grid: &mut [i32],
    grid_dims: (usize, usize),
    band: &[i32],
    band_dims: (usize, usize),
    offset: (usize, usize),
) {
    let (gw, gh) = grid_dims;
    let (bw, bh) = band_dims;
    let (xoff, yoff) = offset;
    for j in 0..bh {
        for i in 0..bw {
            let src = j * bw + i;
            if src >= band.len() {
                continue;
            }
            let x = 2 * i + xoff;
            let y = 2 * j + yoff;
            if x < gw && y < gh {
                grid[y * gw + x] = band[src];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Standard multi-level interleaved 5/3 analysis (forward), producing
    /// `[resolution][subband]` coefficient arrays that mirror what Tier-1 would
    /// recover.  Used to validate that [`synthesize_component`] inverts it.
    fn analyze(image: &[i32], layout: &TileComponentLayout) -> Vec<Vec<Vec<i32>>> {
        let n = layout.num_levels as usize;
        let mut storage: Vec<Vec<Vec<i32>>> = layout
            .resolutions
            .iter()
            .map(|res| {
                res.subbands
                    .iter()
                    .map(|sb| vec![0i32; sb.width * sb.height])
                    .collect()
            })
            .collect();

        if n == 0 {
            storage[0][0] = image.to_vec();
            return storage;
        }

        let mut cur = image.to_vec();

        for r in (1..=n).rev() {
            let res = &layout.resolutions[r];
            let rw = res.width;
            let rh = res.height;
            let mut grid = cur.clone();
            Reversible53::forward_2d(&mut grid, rw, rh).expect("forward");

            let ll_w = layout.resolutions[r - 1].width;
            let ll_h = layout.resolutions[r - 1].height;
            let mut ll = vec![0i32; ll_w * ll_h];
            let gdims = (rw, rh);
            deinterleave(&grid, gdims, &mut ll, (ll_w, ll_h), (0, 0));

            let hl = &res.subbands[0];
            let (hlw, hlh) = (hl.width, hl.height);
            deinterleave(&grid, gdims, &mut storage[r][0], (hlw, hlh), (1, 0));
            let lh = &res.subbands[1];
            let (lhw, lhh) = (lh.width, lh.height);
            deinterleave(&grid, gdims, &mut storage[r][1], (lhw, lhh), (0, 1));
            let hh = &res.subbands[2];
            let (hhw, hhh) = (hh.width, hh.height);
            deinterleave(&grid, gdims, &mut storage[r][2], (hhw, hhh), (1, 1));

            cur = ll;
        }

        storage[0][0] = cur;
        storage
    }

    fn deinterleave(
        grid: &[i32],
        grid_dims: (usize, usize),
        band: &mut [i32],
        band_dims: (usize, usize),
        offset: (usize, usize),
    ) {
        let (gw, gh) = grid_dims;
        let (bw, bh) = band_dims;
        let (xoff, yoff) = offset;
        for j in 0..bh {
            for i in 0..bw {
                let x = 2 * i + xoff;
                let y = 2 * j + yoff;
                if x < gw && y < gh {
                    band[j * bw + i] = grid[y * gw + x];
                }
            }
        }
    }

    #[test]
    fn test_synthesis_round_trip_one_level() {
        let (w, h) = (8usize, 8usize);
        let image: Vec<i32> = (0..(w * h) as i32).map(|v| v * 3 - 50).collect();
        let layout = build_tile_component_layout(w, h, 1, 64, 64);
        let subbands = analyze(&image, &layout);
        let out = synthesize_component(&layout, &subbands).expect("synth");
        assert_eq!(out, image, "one-level 5/3 synthesis must invert analysis");
    }

    #[test]
    fn test_synthesis_round_trip_two_levels_non_square() {
        let (w, h) = (10usize, 6usize);
        let image: Vec<i32> = (0..(w * h) as i32).map(|v| (v * v) % 251 - 40).collect();
        let layout = build_tile_component_layout(w, h, 2, 64, 64);
        let subbands = analyze(&image, &layout);
        let out = synthesize_component(&layout, &subbands).expect("synth");
        assert_eq!(out, image, "two-level 5/3 synthesis must invert analysis");
    }

    #[test]
    fn test_synthesis_round_trip_three_levels() {
        let (w, h) = (16usize, 16usize);
        let image: Vec<i32> = (0..(w * h) as i32).map(|v| (v * 7) % 200).collect();
        let layout = build_tile_component_layout(w, h, 3, 32, 32);
        let subbands = analyze(&image, &layout);
        let out = synthesize_component(&layout, &subbands).expect("synth");
        assert_eq!(out, image, "three-level 5/3 synthesis must invert analysis");
    }

    #[test]
    fn test_multi_layer_rejected() {
        let comps = [TileComponentInput {
            comp_w: 4,
            comp_h: 4,
            precision: 8,
        }];
        let params = TileDecodeParams {
            components: &comps,
            num_levels: 0,
            cbw: 16,
            cbh: 16,
            progression: ProgressionOrder::Lrcp,
            num_layers: 3,
            guard_bits: 2,
            quantization: None,
            has_sop: false,
            has_eph: false,
        };
        let err = decode_tile_components(&[0x00], &params);
        assert!(matches!(err, Err(Jpeg2000Error::UnsupportedFeature(_))));
    }

    #[test]
    fn test_unsupported_progression_rejected() {
        let comps = [TileComponentInput {
            comp_w: 4,
            comp_h: 4,
            precision: 8,
        }];
        let params = TileDecodeParams {
            components: &comps,
            num_levels: 0,
            cbw: 16,
            cbh: 16,
            progression: ProgressionOrder::Cprl,
            num_layers: 1,
            guard_bits: 2,
            quantization: None,
            has_sop: false,
            has_eph: false,
        };
        let err = decode_tile_components(&[0x00], &params);
        assert!(matches!(err, Err(Jpeg2000Error::UnsupportedFeature(_))));
    }

    #[test]
    fn test_empty_tile_data_all_zero() {
        let comps = [TileComponentInput {
            comp_w: 4,
            comp_h: 4,
            precision: 8,
        }];
        let params = TileDecodeParams {
            components: &comps,
            num_levels: 0,
            cbw: 16,
            cbh: 16,
            progression: ProgressionOrder::Lrcp,
            num_layers: 1,
            guard_bits: 2,
            quantization: None,
            has_sop: false,
            has_eph: false,
        };
        let out = decode_tile_components(&[], &params).expect("empty");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], vec![0i32; 16]);
    }
}
