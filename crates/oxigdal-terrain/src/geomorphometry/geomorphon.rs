//! Geomorphon landform classification (Jasiewicz & Stepinski 2013).
//!
//! Classifies each DEM cell into one of 10 landform types by analysing the
//! line-of-sight profile along 8 rays and encoding the result as a ternary
//! pattern (each direction is +1, −1, or 0).
//!
//! Classes (1-indexed per the original paper):
//! 1 = flat, 2 = peak, 3 = ridge, 4 = shoulder, 5 = spur,
//! 6 = slope, 7 = hollow, 8 = footslope, 9 = valley, 10 = pit.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

// ---------------------------------------------------------------------------
// Direction table — (delta_row, delta_col) for N, NE, E, SE, S, SW, W, NW
// ---------------------------------------------------------------------------
const DIRS: [(isize, isize); 8] = [
    (-1, 0),  // N
    (-1, 1),  // NE
    (0, 1),   // E
    (1, 1),   // SE
    (1, 0),   // S
    (1, -1),  // SW
    (0, -1),  // W
    (-1, -1), // NW
];

/// `true` for diagonal directions (NE, SE, SW, NW) — they have horizontal
/// distance `cell_size * √2` per step rather than `cell_size * 1`.
const IS_DIAGONAL: [bool; 8] = [false, true, false, true, false, true, false, true];

// ---------------------------------------------------------------------------
// Core class lookup
// ---------------------------------------------------------------------------

/// Map (n_plus, n_minus) → geomorphon class (1..=10).
///
/// In Jasiewicz & Stepinski (2013) the ternary pattern encodes the visible
/// horizon angle looking *outward* from the centre cell:
/// - `+1` (L) = horizon is **above** the centre (terrain rises → you look UP)
/// - `−1` (W) = horizon is **below** the centre (terrain falls → you look DOWN)
///
/// Therefore:
/// - A **pit** has all directions looking UP  → all `+` → `(n_plus=8, n_minus=0)`
/// - A **peak** has all directions looking DOWN → all `−` → `(n_plus=0, n_minus=8)`
/// - A **ridge** runs along a high axis: left/right look DOWN, front/back look up
///   or flat → dominated by `-` directions → `(0, 5..=7)` range
/// - A **valley** is the opposite → dominated by `+` directions
#[inline]
fn geomorphon_class(n_plus: u8, n_minus: u8) -> u8 {
    match (n_plus, n_minus) {
        // Pure extreme forms
        (0, 8) => 2,  // peak  (all terrain falls outward → all nadir)
        (8, 0) => 10, // pit   (all terrain rises outward → all zenith)
        (0, 0) => 1,  // flat

        // Dominantly negative (terrain falls), no positives → ridge family
        (0, 5..=7) => 3, // ridge
        (0, 3..=4) => 4, // shoulder
        (0, 1..=2) => 5, // spur

        // Dominantly positive (terrain rises), no negatives → valley family
        (5..=7, 0) => 9, // valley
        (3..=4, 0) => 8, // footslope
        (1..=2, 0) => 7, // hollow

        // Mixed: classify by signed difference
        (np, nm) if nm > np + 2 => 5, // spur  (more nadir → local high)
        (np, nm) if np > nm + 2 => 7, // hollow (more zenith → local low)
        (np, nm) if nm > np => 4,     // shoulder
        (np, nm) if np > nm => 8,     // footslope
        _ => 6,                       // slope (balanced)
    }
}

// ---------------------------------------------------------------------------
// Per-direction analysis helper
// ---------------------------------------------------------------------------

/// Parameters for a single-ray direction classification.
///
/// Bundles the cell-level and algorithm-level settings so that
/// `classify_direction` stays within the 7-argument Clippy limit.
struct RayParams {
    /// Row index of the centre cell.
    center_r: usize,
    /// Column index of the centre cell.
    center_c: usize,
    /// Elevation of the centre cell (pre-cast to f64).
    elev_center: f64,
    /// Horizontal cell size (metres or map units).
    cell_size: f64,
    /// (delta_row, delta_col) unit step for this direction.
    dir: (isize, isize),
    /// True for diagonal directions (NE/SE/SW/NW) — horiz dist per step = √2·cell_size.
    diagonal: bool,
    /// Maximum number of steps away from centre.
    search_radius: usize,
    /// Number of steps nearest the centre to skip.
    skip_radius: usize,
    /// Flatness threshold in radians.
    flat_thresh: f64,
}

/// Classify a single ray direction as `+1i8`, `−1i8`, or `0i8`.
///
/// Walks the DEM from `skip_radius+1` to `search_radius` steps in the given
/// direction, tracking the maximum zenith and minimum nadir elevation angles.
/// Returns `+1` if terrain rises steeply, `−1` if it falls steeply, or `0`
/// (flat) when neither threshold is exceeded.
fn classify_direction<T>(dem: &Array2<T>, p: &RayParams) -> i8
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = dem.dim();
    let horiz_per_step = if p.diagonal {
        p.cell_size * std::f64::consts::SQRT_2
    } else {
        p.cell_size
    };

    let (dir_row, dir_col) = p.dir;
    let start = if p.skip_radius == 0 {
        1
    } else {
        p.skip_radius + 1
    };

    let mut max_zenith: f64 = f64::NEG_INFINITY; // most elevated angle (up)
    let mut min_zenith: f64 = f64::INFINITY; // most depressed angle (down)

    for step in start..=p.search_radius {
        let t = step as f64;
        let sr = p.center_r as isize + (dir_row as f64 * t).round() as isize;
        let sc = p.center_c as isize + (dir_col as f64 * t).round() as isize;

        // Bounds check
        if sr < 0 || sc < 0 {
            break;
        }
        let sr = sr as usize;
        let sc = sc as usize;
        if sr >= rows || sc >= cols {
            break;
        }

        let elev_sample: f64 = dem[[sr, sc]].into();
        let horiz = horiz_per_step * t;
        let angle = (elev_sample - p.elev_center).atan2(horiz);

        if angle > max_zenith {
            max_zenith = angle;
        }
        if angle < min_zenith {
            min_zenith = angle;
        }
    }

    // If no valid sample was found (e.g. skip_radius >= search_radius, edge cell)
    // treat the direction as flat.
    if max_zenith == f64::NEG_INFINITY {
        return 0i8;
    }

    if max_zenith > p.flat_thresh {
        1i8
    } else if min_zenith < -p.flat_thresh {
        -1i8
    } else {
        0i8
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Classify DEM cells into geomorphon landform classes.
///
/// Each output cell is an integer in **1–10** (Jasiewicz & Stepinski 2013):
/// 1 = flat, 2 = peak, 3 = ridge, 4 = shoulder, 5 = spur,
/// 6 = slope, 7 = hollow, 8 = footslope, 9 = valley, 10 = pit.
///
/// Cells within `search_radius` of the border and cells containing `nodata`
/// are assigned **0** (unclassified).
///
/// # Parameters
/// * `dem`           – input elevation grid
/// * `cell_size`     – horizontal cell size in the DEM's linear unit (e.g. metres)
/// * `search_radius` – look-ahead distance in cells (must be ≥ 1 and < min(rows,cols)/2)
/// * `skip_radius`   – number of cells nearest the centre to skip (0 = none)
/// * `flatness_deg`  – angular threshold (degrees) below which a direction is flat
/// * `nodata`        – optional nodata sentinel value
///
/// # Errors
/// Returns [`TerrainError::InvalidRadius`] if `search_radius` is 0 or too large,
/// or [`TerrainError::InvalidCellSize`] if `cell_size` ≤ 0.
pub fn geomorphons<T>(
    dem: &Array2<T>,
    cell_size: f64,
    search_radius: usize,
    skip_radius: usize,
    flatness_deg: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    // --- parameter validation -----------------------------------------------
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }

    let (rows, cols) = dem.dim();
    let min_dim = rows.min(cols);

    if search_radius == 0 || search_radius >= min_dim / 2 {
        return Err(TerrainError::InvalidRadius {
            radius: search_radius as f64,
        });
    }

    let flat_thresh = flatness_deg.to_radians();
    let mut output = Array2::zeros((rows, cols));

    // --- per-cell classification ---------------------------------------------
    for r in 0..rows {
        for c in 0..cols {
            // Border guard: skip cells too close to the edge
            if r < search_radius
                || r + search_radius >= rows
                || c < search_radius
                || c + search_radius >= cols
            {
                // output stays 0 (unclassified)
                continue;
            }

            let elev_center: T = dem[[r, c]];

            // nodata check
            if let Some(nd) = nodata {
                if (elev_center - nd).abs() < T::epsilon() {
                    continue;
                }
            }

            let elev_f64: f64 = elev_center.into();

            let mut n_plus: u8 = 0;
            let mut n_minus: u8 = 0;

            for d in 0..8usize {
                let params = RayParams {
                    center_r: r,
                    center_c: c,
                    elev_center: elev_f64,
                    cell_size,
                    dir: DIRS[d],
                    diagonal: IS_DIAGONAL[d],
                    search_radius,
                    skip_radius,
                    flat_thresh,
                };
                match classify_direction(dem, &params) {
                    1 => n_plus += 1,
                    -1 => n_minus += 1,
                    _ => {}
                }
            }

            output[[r, c]] = geomorphon_class(n_plus, n_minus);
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array2;

    /// Build a 9×9 DEM filled with a constant value.
    fn flat_dem(val: f64) -> Array2<f64> {
        Array2::from_elem((9, 9), val)
    }

    // -----------------------------------------------------------------------
    // 1. Peak: central spike — all 8 directions look DOWN from centre
    // -----------------------------------------------------------------------
    #[test]
    fn test_peak() {
        let mut dem = Array2::from_elem((9, 9), 50.0_f64);
        dem[[4, 4]] = 100.0;
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("peak classification failed");
        assert_eq!(
            out[[4, 4]],
            2,
            "central spike should be classified as peak (2), got {}",
            out[[4, 4]]
        );
    }

    // -----------------------------------------------------------------------
    // 2. Pit: central depression — all 8 directions look UP from centre
    // -----------------------------------------------------------------------
    #[test]
    fn test_pit() {
        let mut dem = Array2::from_elem((9, 9), 50.0_f64);
        dem[[4, 4]] = 0.0;
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("pit classification failed");
        assert_eq!(
            out[[4, 4]],
            10,
            "central depression should be classified as pit (10), got {}",
            out[[4, 4]]
        );
    }

    // -----------------------------------------------------------------------
    // 3. Flat: uniform elevation — all interior cells should be flat
    // -----------------------------------------------------------------------
    #[test]
    fn test_flat() {
        let dem = flat_dem(42.0);
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("flat classification failed");
        // Interior cells (excluding border guard = search_radius=2 from each edge)
        for r in 2..7 {
            for c in 2..7 {
                assert_eq!(
                    out[[r, c]],
                    1,
                    "uniform DEM cell ({r},{c}) should be flat (1), got {}",
                    out[[r, c]]
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // 4. Ridge: N-S running ridge — high centre column, low flanks
    // -----------------------------------------------------------------------
    #[test]
    fn test_ridge() {
        // 9×9: centre column (col=4) is high (100), rest is low (50)
        let mut dem = Array2::from_elem((9, 9), 50.0_f64);
        for r in 0..9 {
            dem[[r, 4]] = 100.0;
        }
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("ridge classification failed");
        // Centre cell of the ridge spine
        let cls = out[[4, 4]];
        // Expect ridge (3) or shoulder (4) — ridge spine with steep sides
        assert!(
            cls == 3 || cls == 4,
            "N-S ridge centre should be ridge(3) or shoulder(4), got {cls}"
        );
    }

    // -----------------------------------------------------------------------
    // 5. Valley: N-S running valley — low centre column, high flanks
    // -----------------------------------------------------------------------
    #[test]
    fn test_valley() {
        // 9×9: centre column (col=4) is low (0), rest is high (50)
        let mut dem = Array2::from_elem((9, 9), 50.0_f64);
        for r in 0..9 {
            dem[[r, 4]] = 0.0;
        }
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("valley classification failed");
        let cls = out[[4, 4]];
        // Expect valley (9) or footslope (8)
        assert!(
            cls == 9 || cls == 8,
            "N-S valley centre should be valley(9) or footslope(8), got {cls}"
        );
    }

    // -----------------------------------------------------------------------
    // 6. Slope: tilted plane — interior cells should be classified as slope
    // -----------------------------------------------------------------------
    #[test]
    fn test_slope() {
        // Gentle N→S ramp: row 0 = 10.0, row 8 = 18.0 (1 unit per row)
        let dem = Array2::from_shape_fn((9, 9), |(r, _c)| r as f64 + 10.0);
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("slope classification failed");
        // Slope cells: E/W directions are flat; N/S directions are asymmetric
        // The centre cell (4,4) should be slope (6)
        let cls = out[[4, 4]];
        assert_eq!(cls, 6, "tilted-plane centre should be slope (6), got {cls}");
    }

    // -----------------------------------------------------------------------
    // 7. Invalid radius: search_radius = 0 → Err
    // -----------------------------------------------------------------------
    #[test]
    fn test_invalid_radius() {
        let dem = flat_dem(10.0);
        let result = geomorphons(&dem, 1.0, 0, 0, 1.0, None);
        assert!(result.is_err(), "search_radius=0 should return an error");
    }

    // -----------------------------------------------------------------------
    // 8. Nodata: cell with nodata value → output = 0
    // -----------------------------------------------------------------------
    #[test]
    fn test_nodata() {
        let nodata_val = -9999.0_f64;
        let mut dem = Array2::from_elem((9, 9), 50.0_f64);
        dem[[4, 4]] = nodata_val;
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, Some(nodata_val)).expect("nodata test failed");
        assert_eq!(
            out[[4, 4]],
            0,
            "nodata cell should be unclassified (0), got {}",
            out[[4, 4]]
        );
    }

    // -----------------------------------------------------------------------
    // 9. Border cells: unclassified (0) within search_radius of edge
    // -----------------------------------------------------------------------
    #[test]
    fn test_border_unclassified() {
        let dem = flat_dem(10.0);
        let out = geomorphons(&dem, 1.0, 2, 0, 1.0, None).expect("border test failed");
        // Cells in the first / last 2 rows and columns should be 0
        for c in 0..9 {
            assert_eq!(out[[0, c]], 0, "row 0 should be 0");
            assert_eq!(out[[1, c]], 0, "row 1 should be 0");
            assert_eq!(out[[7, c]], 0, "row 7 should be 0");
            assert_eq!(out[[8, c]], 0, "row 8 should be 0");
        }
    }

    // -----------------------------------------------------------------------
    // 10. Invalid cell size
    // -----------------------------------------------------------------------
    #[test]
    fn test_invalid_cell_size() {
        let dem = flat_dem(10.0);
        let result = geomorphons(&dem, -1.0, 2, 0, 1.0, None);
        assert!(result.is_err(), "negative cell_size should return an error");
        let result_zero = geomorphons(&dem, 0.0, 2, 0, 1.0, None);
        assert!(
            result_zero.is_err(),
            "zero cell_size should return an error"
        );
    }
}
