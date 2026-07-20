//! Sub-region extraction: copy only the tiles within a lon/lat bounding box
//! into a new PMTiles v3 archive.
//!
//! # Overview
//!
//! [`extract_subregion`] opens an existing archive, iterates over all zoom
//! levels in the requested range, converts the lon/lat bbox to a tile grid
//! range via [`bbox_to_tile_range`], fetches each tile that lies within that
//! range, and writes the result into a fresh [`PmTilesBuilder`] archive.
//!
//! # Coordinate conventions
//!
//! All angles are in decimal degrees, WGS-84.
//! - `min_lon`/`max_lon`: −180 to +180 (positive East).
//! - `min_lat`/`max_lat`: approx −85.05 to +85.05 (positive North).
//!
//! Antimeridian-crossing bboxes (where `min_lon > max_lon`) are rejected
//! with [`PmTilesError::InvalidBounds`]; callers must split the bbox into
//! two before calling this function.

use crate::error::PmTilesError;
use crate::hilbert::zxy_to_tile_id;
use crate::pmtiles::PmTilesReader;
use crate::webmerc::lonlat_to_tile;
use crate::writer::PmTilesBuilder;

// ---------------------------------------------------------------------------
// ExtractOptions
// ---------------------------------------------------------------------------

/// Options controlling which tiles are copied during sub-region extraction.
///
/// The zoom-level fields narrow the range; when `None`, the source archive's
/// own `min_zoom`/`max_zoom` header values are used.
///
/// # Example
/// ```
/// use oxigeo_pmtiles::extract::ExtractOptions;
///
/// let opts = ExtractOptions {
///     min_zoom: Some(3),
///     max_zoom: Some(10),
///     preserve_metadata: true,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Override the minimum zoom level.  When `None` the archive's `min_zoom`
    /// header field is used.
    pub min_zoom: Option<u8>,
    /// Override the maximum zoom level.  When `None` the archive's `max_zoom`
    /// header field is used.
    pub max_zoom: Option<u8>,
    /// When `true`, the output archive's geographic bounds header fields are
    /// populated with the intersection of the source archive's bounds and the
    /// requested extraction bbox, and the zoom range is copied from the source.
    /// When `false`, the builder defaults are used (world extent).
    pub preserve_metadata: bool,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            min_zoom: None,
            max_zoom: None,
            preserve_metadata: true,
        }
    }
}

// ---------------------------------------------------------------------------
// bbox_to_tile_range
// ---------------------------------------------------------------------------

/// Convert a WGS-84 bounding box to the inclusive tile grid range
/// `(min_x, min_y, max_x, max_y)` at zoom level `z`.
///
/// The mapping from lat/lon to tile (x, y) follows the standard Web Mercator
/// slippy-map convention used by OpenStreetMap and PMTiles:
///
/// - `x` increases West → East (like longitude).
/// - `y` increases North → **South** (opposite to latitude).
///
/// Because of this inversion the tile that contains `max_lat` (the northern
/// edge) has a **lower** y-index than the tile containing `min_lat` (the
/// southern edge).  This function normalises the result so that
/// `min_y <= max_y`, which is required for a correct iteration:
///
/// ```text
/// min_y = tile y for max_lat  (northern-most row, numerically smallest)
/// max_y = tile y for min_lat  (southern-most row, numerically largest)
/// ```
///
/// Tile coordinates are clamped to `[0, 2^z − 1]` before being returned.
///
/// # Errors
///
/// Returns [`PmTilesError::InvalidBounds`] when:
/// - `min_lon > max_lon` (antimeridian-crossing bbox — split it first).
/// - `min_lat > max_lat` (degenerate / inverted latitude range).
/// - Any coordinate is non-finite (NaN or infinity).
///
/// # Examples
///
/// ```
/// use oxigeo_pmtiles::extract::bbox_to_tile_range;
///
/// // Full world at z=0 → single tile (0, 0, 0, 0)
/// let (min_x, min_y, max_x, max_y) = bbox_to_tile_range(0, -180.0, -85.05, 180.0, 85.05).unwrap();
/// assert_eq!((min_x, min_y, max_x, max_y), (0, 0, 0, 0));
/// ```
pub fn bbox_to_tile_range(
    z: u8,
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
) -> Result<(u32, u32, u32, u32), PmTilesError> {
    // Guard against non-finite inputs.
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return Err(PmTilesError::InvalidBounds(
            "Bounding box coordinates must be finite".into(),
        ));
    }

    // Reject antimeridian-crossing bboxes.
    if min_lon > max_lon {
        return Err(PmTilesError::InvalidBounds(format!(
            "min_lon ({min_lon}) > max_lon ({max_lon}): antimeridian-crossing bboxes are not \
             supported — split the bbox before calling bbox_to_tile_range"
        )));
    }

    // Reject inverted latitude range.
    if min_lat > max_lat {
        return Err(PmTilesError::InvalidBounds(format!(
            "min_lat ({min_lat}) > max_lat ({max_lat}): latitude range is inverted"
        )));
    }

    // At z=0 there is exactly one tile covering the entire world.
    if z == 0 {
        return Ok((0, 0, 0, 0));
    }

    // Convert the two corners of the bbox to tile coordinates.
    // lonlat_to_tile clamps internally, so lat values beyond ±85.05 are safe.
    //
    // NW corner (min_lon, max_lat) → tile with smallest (x, y) indices.
    let (x_min, y_for_max_lat) = lonlat_to_tile(z, min_lon, max_lat);
    // SE corner (max_lon, min_lat) → tile with largest (x, y) indices.
    let (x_max, y_for_min_lat) = lonlat_to_tile(z, max_lon, min_lat);

    // y_for_max_lat is the row of the northernmost edge (numerically smallest).
    // y_for_min_lat is the row of the southernmost edge (numerically largest).
    // We normalise so min_y ≤ max_y.
    let min_y = y_for_max_lat.min(y_for_min_lat);
    let max_y = y_for_max_lat.max(y_for_min_lat);
    let min_x = x_min.min(x_max);
    let max_x = x_min.max(x_max);

    Ok((min_x, min_y, max_x, max_y))
}

// ---------------------------------------------------------------------------
// extract_subregion
// ---------------------------------------------------------------------------

/// Extract the tiles within `(min_lon, min_lat, max_lon, max_lat)` from an
/// existing PMTiles v3 archive and return the result as a new archive.
///
/// # Algorithm
///
/// 1. Parse the source archive header.
/// 2. Compute the effective zoom range: `[opts.min_zoom ∥ header.min_zoom,
///    opts.max_zoom ∥ header.max_zoom]`.
/// 3. For every zoom level in the effective range:
///    a. Compute the tile grid range that intersects the bbox via [`bbox_to_tile_range`].
///    b. For every `(x, y)` in the grid, fetch the tile with `reader.get_tile(z, x, y)`
///    and add it to the builder when present.
/// 4. When `preserve_metadata` is `true`, set the output bounds to the
///    intersection of the source archive bounds and the requested bbox, and
///    copy the min/max zoom from the source.
/// 5. Build and return the archive bytes.
///
/// Tile data is copied verbatim (raw compressed bytes, matching the source
/// archive's `tile_compression`).  Deduplication and run-length compression
/// are applied by the builder on `build()`.
///
/// # Errors
///
/// - [`PmTilesError::InvalidBounds`] — antimeridian-crossing or degenerate
///   bbox.
/// - [`PmTilesError::InvalidFormat`] / [`PmTilesError::InvalidArchive`] —
///   corrupt source archive.
/// - Any other error propagated from [`PmTilesReader`] or [`PmTilesBuilder`].
///
/// # Examples
///
/// ```
/// use oxigeo_pmtiles::{PmTilesBuilder, PmTilesReader, TileType};
/// use oxigeo_pmtiles::extract::{ExtractOptions, extract_subregion};
///
/// // Build a tiny source archive with one tile at z=1 covering NW quadrant.
/// let mut src_builder = PmTilesBuilder::new(TileType::Png, 0, 1);
/// src_builder.add_tile(1, 0, 0, b"nw-tile").unwrap();
/// let source = src_builder.build().unwrap();
///
/// // Extract: only the NW quadrant (-180..0, 0..85).
/// let opts = ExtractOptions { min_zoom: Some(1), max_zoom: Some(1), preserve_metadata: true };
/// let extracted = extract_subregion(&source, -180.0, 0.0, 0.0, 85.0, &opts).unwrap();
///
/// // The extracted archive is a valid PMTiles file.
/// let reader = PmTilesReader::from_bytes(extracted).unwrap();
/// assert!(reader.get_tile(1, 0, 0).unwrap().is_some());
/// ```
pub fn extract_subregion(
    archive: &[u8],
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
    opts: &ExtractOptions,
) -> Result<Vec<u8>, PmTilesError> {
    // Guard inputs before opening the archive.
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return Err(PmTilesError::InvalidBounds(
            "Bounding box coordinates must be finite".into(),
        ));
    }
    if min_lon > max_lon {
        return Err(PmTilesError::InvalidBounds(format!(
            "min_lon ({min_lon}) > max_lon ({max_lon}): antimeridian-crossing bboxes are not \
             supported"
        )));
    }
    if min_lat > max_lat {
        return Err(PmTilesError::InvalidBounds(format!(
            "min_lat ({min_lat}) > max_lat ({max_lat}): latitude range is inverted"
        )));
    }

    // Open the source archive.
    let reader = PmTilesReader::from_bytes(archive.to_vec())?;
    let hdr = &reader.header;

    // Effective zoom range: opts overrides, otherwise defer to the source header.
    let effective_min_zoom = opts.min_zoom.unwrap_or(hdr.min_zoom);
    let effective_max_zoom = opts.max_zoom.unwrap_or(hdr.max_zoom);

    // The output zoom range may differ from the source when opts restrict it.
    let out_min_zoom = effective_min_zoom;
    let out_max_zoom = effective_max_zoom;

    // Create a builder that covers the effective zoom range.
    let mut builder = PmTilesBuilder::new(hdr.tile_type.clone(), out_min_zoom, out_max_zoom);

    // Iterate over each zoom level and each tile in the grid range.
    for z in effective_min_zoom..=effective_max_zoom {
        let (min_x, min_y, max_x, max_y) =
            bbox_to_tile_range(z, min_lon, min_lat, max_lon, max_lat)?;

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                if let Some(data) = reader.get_tile(z, x, y)? {
                    let tile_id = zxy_to_tile_id(z, x, y)?;
                    builder.add_tile_by_id(tile_id, &data)?;
                }
            }
        }
    }

    // Optionally propagate geographic metadata from the source header.
    if opts.preserve_metadata {
        // Intersect the source archive bounds with the requested bbox.
        let src_min_lon = hdr.min_lon_e7 as f64 / 1e7;
        let src_min_lat = hdr.min_lat_e7 as f64 / 1e7;
        let src_max_lon = hdr.max_lon_e7 as f64 / 1e7;
        let src_max_lat = hdr.max_lat_e7 as f64 / 1e7;

        // Clamped intersection: take the inner (tighter) of the two bboxes.
        let clamped_min_lon = src_min_lon.max(min_lon);
        let clamped_min_lat = src_min_lat.max(min_lat);
        let clamped_max_lon = src_max_lon.min(max_lon);
        let clamped_max_lat = src_max_lat.min(max_lat);

        // Only set bounds when the intersection is non-degenerate.
        if clamped_min_lon <= clamped_max_lon && clamped_min_lat <= clamped_max_lat {
            builder.set_bounds(
                clamped_min_lon,
                clamped_min_lat,
                clamped_max_lon,
                clamped_max_lat,
            );
        }

        // Set centre to the midpoint of the clamped bbox at min_zoom.
        if clamped_min_lon <= clamped_max_lon && clamped_min_lat <= clamped_max_lat {
            let center_lon = (clamped_min_lon + clamped_max_lon) / 2.0;
            let center_lat = (clamped_min_lat + clamped_max_lat) / 2.0;
            builder.set_center(center_lon, center_lat, out_min_zoom);
        }
    }

    builder.build()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{PmTilesHeader, TileType};
    use crate::writer::PmTilesBuilder;

    // -----------------------------------------------------------------------
    // Helper
    // -----------------------------------------------------------------------

    fn build_test_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
        let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
        let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
        let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
        for &(z, x, y, data) in tiles {
            builder.add_tile(z, x, y, data).expect("add_tile");
        }
        builder.build().expect("build")
    }

    // -----------------------------------------------------------------------
    // bbox_to_tile_range unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_bbox_to_tile_range_full_world_z0() {
        let (min_x, min_y, max_x, max_y) =
            bbox_to_tile_range(0, -180.0, -85.05, 180.0, 85.05).expect("ok");
        // z=0 is always a single tile.
        assert_eq!((min_x, min_y, max_x, max_y), (0, 0, 0, 0));
    }

    #[test]
    fn test_bbox_to_tile_range_full_world_z1() {
        // At z=1 there are 4 tiles (2×2).  The full world bbox should yield all 4.
        let (min_x, min_y, max_x, max_y) =
            bbox_to_tile_range(1, -180.0, -85.05, 180.0, 85.05).expect("ok");
        assert_eq!(min_x, 0);
        assert_eq!(min_y, 0);
        assert_eq!(max_x, 1);
        assert_eq!(max_y, 1);
    }

    #[test]
    fn test_bbox_to_tile_range_nw_quadrant_z1() {
        // NW tile at z=1 is (x=0, y=0), covering lon [-180, 0) and lat [0, ~85).
        // To stay strictly inside tile x=0 we use max_lon = -0.001 to avoid
        // the right boundary (lon=0 maps to tile x=1 at z=1).
        let (min_x, min_y, max_x, max_y) =
            bbox_to_tile_range(1, -180.0, 0.001, -0.001, 85.0).expect("ok");
        assert_eq!(min_x, 0, "x min");
        assert_eq!(max_x, 0, "x max");
        assert_eq!(min_y, 0, "y min");
        assert_eq!(max_y, 0, "y max");
    }

    #[test]
    fn test_bbox_to_tile_range_antimeridian_error() {
        // min_lon > max_lon is forbidden.
        let result = bbox_to_tile_range(5, 170.0, -10.0, -170.0, 10.0);
        assert!(result.is_err());
        // Verify the error message mentions antimeridian.
        if let Err(e) = result {
            let msg = format!("{e}");
            assert!(msg.contains("antimeridian"), "err msg: {msg}");
        }
    }

    #[test]
    fn test_bbox_to_tile_range_inverted_lat_error() {
        let err = bbox_to_tile_range(5, 0.0, 50.0, 10.0, 10.0);
        assert!(err.is_err());
    }

    #[test]
    fn test_bbox_to_tile_range_single_tile_z2() {
        // z=2 tile grid is 4×4. Tile (z=2, x=2, y=1) covers the NE quadrant
        // of the northern hemisphere. We obtain its bbox and convert back.
        use crate::webmerc::tile_bounds_lonlat;
        let (tmin_lon, tmin_lat, tmax_lon, tmax_lat) = tile_bounds_lonlat(2, 2, 1);

        // A point strictly inside the tile should map back to exactly that tile.
        let mid_lon = (tmin_lon + tmax_lon) / 2.0;
        let mid_lat = (tmin_lat + tmax_lat) / 2.0;
        // Build a tiny bbox around the midpoint.
        let epsilon = 0.001;
        let (min_x, min_y, max_x, max_y) = bbox_to_tile_range(
            2,
            mid_lon - epsilon,
            mid_lat - epsilon,
            mid_lon + epsilon,
            mid_lat + epsilon,
        )
        .expect("ok");
        // Must be a single tile.
        assert_eq!(min_x, max_x);
        assert_eq!(min_y, max_y);
        // And it must be tile (2, 2, 1).
        assert_eq!(min_x, 2, "expected x=2, got {min_x}");
        assert_eq!(min_y, 1, "expected y=1, got {min_y}");
    }

    // -----------------------------------------------------------------------
    // extract_subregion integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_empty_bbox_no_tiles() {
        // Build an archive with one tile at z=2 in the NE quadrant (x=2, y=0
        // at z=2 is in the eastern hemisphere north of the equator).
        let src = build_test_archive(&[(2, 2, 0, b"ne-tile")]);

        // Extract from the NW quadrant (lon < 0, lat > 0) — no overlap with x=2.
        // We use max_lon = -0.001 to stay strictly west of the prime meridian.
        let opts = ExtractOptions {
            min_zoom: Some(2),
            max_zoom: Some(2),
            preserve_metadata: false,
        };
        let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");

        // Output must be a valid PMTiles archive with zero tiles.
        let hdr = PmTilesHeader::parse(&out).expect("parse");
        assert_eq!(hdr.addressed_tiles, 0, "no tiles should be in the output");
    }

    #[test]
    fn test_extract_single_tile_round_trip() {
        // Archive with exactly one tile at z=1, x=0, y=0 (NW quadrant).
        let src = build_test_archive(&[(1, 0, 0, b"nw-tile-data")]);

        // Extract with bbox strictly inside the NW tile (lon < 0, lat > 0).
        // max_lon = -0.001 ensures we stay in tile x=0 (lon=0 maps to x=1).
        let opts = ExtractOptions {
            min_zoom: Some(1),
            max_zoom: Some(1),
            preserve_metadata: true,
        };
        let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");

        let reader = PmTilesReader::from_bytes(out).expect("reader");
        let tile_bytes = reader
            .get_tile(1, 0, 0)
            .expect("get_tile ok")
            .expect("tile must exist");
        assert_eq!(tile_bytes, b"nw-tile-data");
        // The NE quadrant tile (1, 1, 0) must NOT be in the output.
        assert!(reader.get_tile(1, 1, 0).expect("ok").is_none());
    }

    #[test]
    fn test_extract_multi_zoom_preserves_all_tiles_in_bbox() {
        // z=2 tile x-ranges (90° each at 4 tiles wide):
        //   x=0: [-180,-90)  x=1: [-90,0)  x=2: [0,90)  x=3: [90,180)
        // To exclude the eastern hemisphere, use max_lon = -0.001 (stays in x=1).
        // Tiles in the western half (x=0, x=1) are inside bbox; x=2,3 are outside.
        let src = build_test_archive(&[
            (0, 0, 0, b"z0-world"), // z=0: always included
            (1, 0, 0, b"z1-nw"),    // z=1 western tile — inside bbox
            (2, 0, 0, b"z2-x0-y0"), // z=2 western tile — inside
            (2, 1, 0, b"z2-x1-y0"), // z=2 also western  — inside (lon [-90,0))
            (2, 2, 0, b"z2-east"),  // z=2 eastern tile   — outside (lon [0,90))
        ]);

        let opts = ExtractOptions {
            min_zoom: None,
            max_zoom: None,
            preserve_metadata: true,
        };
        // Bbox: western hemisphere, north of equator.  max_lon=-0.001 keeps us
        // strictly west of the prime meridian so x=2 is excluded.
        let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");
        let reader = PmTilesReader::from_bytes(out).expect("reader");

        // z=0 is always the whole world → must be present.
        assert!(
            reader.get_tile(0, 0, 0).expect("ok").is_some(),
            "z0 tile must exist"
        );
        // z=1 western tile.
        assert!(
            reader.get_tile(1, 0, 0).expect("ok").is_some(),
            "z1 NW tile must exist"
        );
        // z=2 western tiles.
        assert!(
            reader.get_tile(2, 0, 0).expect("ok").is_some(),
            "z2 (0,0) must exist"
        );
        assert!(
            reader.get_tile(2, 1, 0).expect("ok").is_some(),
            "z2 (1,0) must exist"
        );
        // z=2 eastern tile — outside bbox — must be absent.
        assert!(
            reader.get_tile(2, 2, 0).expect("ok").is_none(),
            "z2 eastern tile must be absent"
        );
    }

    #[test]
    fn test_extract_drops_tiles_outside_bbox() {
        // Four z=1 tiles, one in each quadrant.
        // At z=1, x=0 covers lon [-180, 0) and x=1 covers lon [0, 180).
        // y=0 covers lat [0, ~85) and y=1 covers lat (~-85, 0).
        let src = build_test_archive(&[
            (1, 0, 0, b"NW"), // NW: lon [-180,0), lat (0,85]
            (1, 1, 0, b"NE"), // NE: lon [0,180],  lat (0,85]
            (1, 0, 1, b"SW"), // SW: lon [-180,0), lat [-85,0)
            (1, 1, 1, b"SE"), // SE: lon [0,180],  lat [-85,0)
        ]);

        // Extract only the NW tile using max_lon=-0.001 to stay strictly
        // west of lon=0 (which would map to x=1).
        let opts = ExtractOptions {
            min_zoom: Some(1),
            max_zoom: Some(1),
            preserve_metadata: false,
        };
        let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");
        let reader = PmTilesReader::from_bytes(out).expect("reader");

        assert!(
            reader.get_tile(1, 0, 0).expect("ok").is_some(),
            "NW must be present"
        );
        assert!(
            reader.get_tile(1, 1, 0).expect("ok").is_none(),
            "NE must be absent"
        );
        assert!(
            reader.get_tile(1, 0, 1).expect("ok").is_none(),
            "SW must be absent"
        );
        assert!(
            reader.get_tile(1, 1, 1).expect("ok").is_none(),
            "SE must be absent"
        );
    }

    #[test]
    fn test_extract_preserve_metadata_copies_min_max_zoom() {
        // Source covers z=0..=3.
        let src = build_test_archive(&[
            (0, 0, 0, b"z0"),
            (1, 0, 0, b"z1"),
            (2, 0, 0, b"z2"),
            (3, 0, 0, b"z3"),
        ]);

        // Extract with a narrower zoom range and preserve_metadata = true.
        let opts = ExtractOptions {
            min_zoom: Some(1),
            max_zoom: Some(2),
            preserve_metadata: true,
        };
        let out = extract_subregion(&src, -180.0, -85.0, 180.0, 85.0, &opts).expect("ok");

        let hdr = PmTilesHeader::parse(&out).expect("parse");
        // The builder was initialised with the effective zoom range (1..=2).
        assert_eq!(hdr.min_zoom, 1, "min_zoom should be 1");
        assert_eq!(hdr.max_zoom, 2, "max_zoom should be 2");
        // z=0 and z=3 tiles must not appear in output.
        let reader = PmTilesReader::from_bytes(out).expect("reader");
        assert!(
            reader.get_tile(0, 0, 0).expect("ok").is_none(),
            "z0 must be absent"
        );
        assert!(
            reader.get_tile(3, 0, 0).expect("ok").is_none(),
            "z3 must be absent"
        );
        assert!(
            reader.get_tile(1, 0, 0).expect("ok").is_some(),
            "z1 must be present"
        );
        assert!(
            reader.get_tile(2, 0, 0).expect("ok").is_some(),
            "z2 must be present"
        );
    }

    #[test]
    fn test_extract_invalid_bounds_antimeridian() {
        let src = build_test_archive(&[(0, 0, 0, b"z0")]);
        let opts = ExtractOptions::default();
        // min_lon > max_lon → antimeridian crossing → error
        let result = extract_subregion(&src, 170.0, -10.0, -170.0, 10.0, &opts);
        assert!(result.is_err());
    }
}
