//! Polyline terrain profile extraction via bilinear interpolation.
//!
//! Given a DEM and a sequence of (x, y) control points in map coordinates,
//! this module densifies the polyline at a user-specified step distance and
//! samples the DEM elevation at each point using bilinear interpolation.
//!
//! # Coordinate convention
//!
//! The DEM is assumed to be in a **north-up** (row-major, top-left origin)
//! coordinate system:
//!
//! - `origin_x`, `origin_y` are the map coordinates of the **top-left corner**
//!   of cell `dem[[0, 0]]`.
//! - `cell_size` is the square pixel spacing.
//! - Column index increases eastward (positive x).
//! - Row index increases southward (negative y direction), i.e.
//!   `col_f = (x - origin_x) / cell_size` and
//!   `row_f = (origin_y - y)  / cell_size`.
//!
//! # Bilinear interpolation
//!
//! Four neighbouring cells are used for each sample.  If any of the four
//! cells is nodata, or if the fractional index falls outside `[0, rows-1) ×
//! [0, cols-1)`, the sample point receives `elevation = None`.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single sample point on a terrain profile.
#[derive(Debug, Clone)]
pub struct ProfilePoint {
    /// Cumulative distance along the profile from the start (map units).
    pub distance: f64,
    /// Map x coordinate.
    pub x: f64,
    /// Map y coordinate.
    pub y: f64,
    /// Interpolated elevation at this point.  `None` if nodata or out-of-bounds.
    pub elevation: Option<f64>,
}

/// A terrain profile sampled along a polyline.
#[derive(Debug, Clone)]
pub struct TerrainProfile {
    /// All sampled points, ordered from the start to the end of the polyline.
    pub points: Vec<ProfilePoint>,
}

impl TerrainProfile {
    /// Total length of the profile (cumulative distance of the last point).
    ///
    /// Returns `0.0` when `points` is empty.
    pub fn length(&self) -> f64 {
        self.points.last().map_or(0.0, |p| p.distance)
    }

    /// Minimum elevation among valid (non-nodata) points.
    ///
    /// Returns `None` when no valid points exist.
    pub fn min_elevation(&self) -> Option<f64> {
        self.points
            .iter()
            .filter_map(|p| p.elevation)
            .reduce(f64::min)
    }

    /// Maximum elevation among valid (non-nodata) points.
    ///
    /// Returns `None` when no valid points exist.
    pub fn max_elevation(&self) -> Option<f64> {
        self.points
            .iter()
            .filter_map(|p| p.elevation)
            .reduce(f64::max)
    }

    /// Total elevation gain (sum of all uphill steps, in map units).
    ///
    /// Consecutive nodata transitions are skipped.
    pub fn total_gain(&self) -> f64 {
        let mut gain = 0.0;
        let mut prev: Option<f64> = None;
        for p in &self.points {
            if let Some(cur) = p.elevation {
                if let Some(prv) = prev {
                    if cur > prv {
                        gain += cur - prv;
                    }
                }
                prev = Some(cur);
            }
        }
        gain
    }

    /// Total elevation loss (sum of all downhill steps, returned as a positive value).
    ///
    /// Consecutive nodata transitions are skipped.
    pub fn total_loss(&self) -> f64 {
        let mut loss = 0.0;
        let mut prev: Option<f64> = None;
        for p in &self.points {
            if let Some(cur) = p.elevation {
                if let Some(prv) = prev {
                    if cur < prv {
                        loss += prv - cur;
                    }
                }
                prev = Some(cur);
            }
        }
        loss
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract a terrain profile along a polyline by bilinear interpolation.
///
/// The polyline is densified at intervals of `step` map units.  The first
/// point of the profile is always the first vertex of the polyline; the last
/// vertex is always included even when the final segment length is not an
/// exact multiple of `step`.
///
/// # Arguments
/// * `dem` — 2-D elevation grid (rows × cols).
/// * `polyline` — Ordered list of `(x, y)` control points in map coordinates.
///   Must contain at least 2 points.
/// * `origin_x` — Map x coordinate of the top-left corner of `dem[[0, 0]]`.
/// * `origin_y` — Map y coordinate of the top-left corner of `dem[[0, 0]]`.
/// * `cell_size` — Square pixel spacing (must be > 0).
/// * `step` — Sampling interval along the polyline in map units (must be > 0).
/// * `nodata` — Optional nodata sentinel.
///
/// # Returns
/// [`TerrainProfile`] with all sampled points.  Points that fall outside
/// the DEM extent, or where any corner of the bilinear stencil is nodata,
/// carry `elevation = None`.
///
/// # Errors
/// * [`TerrainError::InvalidCellSize`] when `cell_size <= 0`.
/// * [`TerrainError::ComputationError`] when `step <= 0` or the polyline has
///   fewer than 2 vertices.
pub fn extract_profile<T>(
    dem: &Array2<T>,
    polyline: &[(f64, f64)],
    origin_x: f64,
    origin_y: f64,
    cell_size: f64,
    step: f64,
    nodata: Option<T>,
) -> Result<TerrainProfile>
where
    T: Float + Into<f64> + Copy,
{
    // --- Validation ----------------------------------------------------------
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    if step <= 0.0 {
        return Err(TerrainError::ComputationError {
            message: format!("step must be > 0, got {step}"),
        });
    }
    if polyline.len() < 2 {
        return Err(TerrainError::ComputationError {
            message: format!(
                "polyline must have at least 2 control points, got {}",
                polyline.len()
            ),
        });
    }

    let grid_ctx = GridContext {
        origin_x,
        origin_y,
        cell_size,
    };

    // --- Sample accumulation ------------------------------------------------
    let mut points: Vec<ProfilePoint> = Vec::new();
    let mut cumulative_dist: f64 = 0.0;
    // Distance remaining since last explicit sample.
    let mut dist_since_sample: f64 = 0.0;

    // Always emit the first vertex.
    let (x0, y0) = polyline[0];
    points.push(ProfilePoint {
        distance: 0.0,
        x: x0,
        y: y0,
        elevation: bilinear_interp(dem, &grid_ctx, x0, y0, nodata),
    });

    for seg_idx in 0..polyline.len() - 1 {
        let (sx, sy) = polyline[seg_idx];
        let (ex, ey) = polyline[seg_idx + 1];

        let dx = ex - sx;
        let dy = ey - sy;
        let seg_len = dx.hypot(dy);

        if seg_len == 0.0 {
            continue; // zero-length segment: skip
        }

        let ux = dx / seg_len; // unit vector x
        let uy = dy / seg_len; // unit vector y

        // How far into this segment have we "used up" on `dist_since_sample`?
        // We start from 0 within the segment and walk at intervals of `step`.
        let mut t = step - dist_since_sample; // distance to the first sample within this segment

        // Emit samples at t, t+step, t+2*step ... while t < seg_len.
        while t < seg_len {
            let x = sx + ux * t;
            let y = sy + uy * t;
            cumulative_dist += step;
            points.push(ProfilePoint {
                distance: cumulative_dist,
                x,
                y,
                elevation: bilinear_interp(dem, &grid_ctx, x, y, nodata),
            });
            t += step;
        }

        // How much of this segment is left after the last sample?
        dist_since_sample = seg_len - (t - step);
        cumulative_dist += dist_since_sample;
        // (do not add a sample here — the segment end is added below only if
        //  it is also the polyline end, to avoid duplicating vertices)

        // Always include the final endpoint of the last segment.
        if seg_idx == polyline.len() - 2 {
            points.push(ProfilePoint {
                distance: cumulative_dist,
                x: ex,
                y: ey,
                elevation: bilinear_interp(dem, &grid_ctx, ex, ey, nodata),
            });
        } else {
            // Intermediate vertices: reset dist_since_sample so next segment
            // starts fresh.  The vertex itself is *not* added as an explicit
            // sample point; the densification continues seamlessly.
            // (This matches the GIS convention of smooth densification.)
        }
    }

    Ok(TerrainProfile { points })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Georeferencing context for bilinear interpolation.
///
/// Bundles the three grid parameters so that `bilinear_interp` stays within
/// the 7-argument Clippy limit.
struct GridContext {
    origin_x: f64,
    origin_y: f64,
    cell_size: f64,
}

/// Bilinear interpolation at map coordinates `(mx, my)`.
///
/// Returns `None` when the point is outside the DEM or any of the four
/// corner cells is nodata.
fn bilinear_interp<T>(
    dem: &Array2<T>,
    ctx: &GridContext,
    mx: f64,
    my: f64,
    nodata: Option<T>,
) -> Option<f64>
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = dem.dim();
    let col_f = (mx - ctx.origin_x) / ctx.cell_size;
    let row_f = (ctx.origin_y - my) / ctx.cell_size;

    if col_f < 0.0 || row_f < 0.0 {
        return None;
    }

    let c0 = col_f.floor() as usize;
    let r0 = row_f.floor() as usize;

    // We need a 2×2 window: r0..=r0+1, c0..=c0+1
    if c0 + 1 >= cols || r0 + 1 >= rows {
        return None;
    }

    let dc = col_f - c0 as f64;
    let dr = row_f - r0 as f64;

    let v00 = dem[[r0, c0]];
    let v01 = dem[[r0, c0 + 1]];
    let v10 = dem[[r0 + 1, c0]];
    let v11 = dem[[r0 + 1, c0 + 1]];

    // Nodata check: if any corner is nodata, return None.
    if let Some(nd) = nodata {
        for &v in &[v00, v01, v10, v11] {
            if is_nodata(v, nd) {
                return None;
            }
        }
    } else {
        for &v in &[v00, v01, v10, v11] {
            if v.is_nan() {
                return None;
            }
        }
    }

    let z00: f64 = v00.into();
    let z01: f64 = v01.into();
    let z10: f64 = v10.into();
    let z11: f64 = v11.into();

    // Bilinear formula: weights (1-dc,dc) × (1-dr,dr)
    let z = (1.0 - dc) * (1.0 - dr) * z00
        + dc * (1.0 - dr) * z01
        + (1.0 - dc) * dr * z10
        + dc * dr * z11;

    Some(z)
}

fn is_nodata<T: Float>(value: T, nodata: T) -> bool {
    if value.is_nan() && nodata.is_nan() {
        true
    } else {
        (value - nodata).abs() < T::epsilon()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 5×5 DEM with elevation = col index (0..4) — increases W→E.
    fn linear_we_dem() -> Array2<f64> {
        let mut dem = Array2::zeros((5, 5));
        for r in 0..5_usize {
            for c in 0..5_usize {
                dem[[r, c]] = c as f64;
            }
        }
        dem
    }

    #[test]
    fn test_profile_horizontal_line() {
        // Profile runs W→E along the middle row.
        // origin=(0,4), cell_size=1.  y=2 → row=4-2=2 (north-up convention).
        let dem = linear_we_dem();
        let polyline: Vec<(f64, f64)> = vec![(0.5, 2.5), (4.5, 2.5)];
        let profile = extract_profile(&dem, &polyline, 0.0, 5.0, 1.0, 1.0, None::<f64>)
            .expect("extract_profile horizontal");

        // All sampled elevations should be non-decreasing.
        let elevations: Vec<f64> = profile.points.iter().filter_map(|p| p.elevation).collect();
        assert!(
            elevations.len() >= 2,
            "expected >= 2 valid points, got {}",
            elevations.len()
        );
        for w in elevations.windows(2) {
            assert!(
                w[1] >= w[0] - 1.0e-12,
                "elevation should be non-decreasing: {} > {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn test_profile_stats() {
        // Manually craft a profile with known gain/loss.
        let points = vec![
            ProfilePoint {
                distance: 0.0,
                x: 0.0,
                y: 0.0,
                elevation: Some(10.0),
            },
            ProfilePoint {
                distance: 1.0,
                x: 1.0,
                y: 0.0,
                elevation: Some(20.0),
            }, // +10
            ProfilePoint {
                distance: 2.0,
                x: 2.0,
                y: 0.0,
                elevation: Some(15.0),
            }, // -5
            ProfilePoint {
                distance: 3.0,
                x: 3.0,
                y: 0.0,
                elevation: Some(25.0),
            }, // +10
            ProfilePoint {
                distance: 4.0,
                x: 4.0,
                y: 0.0,
                elevation: Some(5.0),
            }, // -20
        ];
        let tp = TerrainProfile { points };

        assert!(
            (tp.total_gain() - 20.0).abs() < 1.0e-12,
            "gain={}",
            tp.total_gain()
        );
        assert!(
            (tp.total_loss() - 25.0).abs() < 1.0e-12,
            "loss={}",
            tp.total_loss()
        );
        assert_eq!(tp.min_elevation(), Some(5.0));
        assert_eq!(tp.max_elevation(), Some(25.0));
        assert!((tp.length() - 4.0).abs() < 1.0e-12);
    }

    #[test]
    fn test_profile_diagonal() {
        // 5×5 DEM, step=1.  Diagonal from (0,0) to (4,4) in map coords.
        // With cell_size=1, origin=(0,5), the polyline in world coords is
        // (0.5,4.5)→(4.5,0.5): length = sqrt(4^2+4^2) ≈ 5.66.
        let dem: Array2<f64> = Array2::from_elem((5, 5), 0.0);
        let polyline: Vec<(f64, f64)> = vec![(0.5, 4.5), (4.5, 0.5)];
        let profile = extract_profile(&dem, &polyline, 0.0, 5.0, 1.0, 1.0, None::<f64>)
            .expect("extract_profile diagonal");

        // Should have at least 2 points (start + end) and no panic.
        assert!(
            profile.points.len() >= 2,
            "expected >= 2 points, got {}",
            profile.points.len()
        );

        // All cumulative distances must be non-decreasing.
        for w in profile.points.windows(2) {
            assert!(
                w[1].distance >= w[0].distance - 1.0e-12,
                "distances must be non-decreasing"
            );
        }
    }

    #[test]
    fn test_profile_bilinear() {
        // 2×2 DEM with corners: z00=0, z01=4, z10=8, z11=12.
        // At the centre of the DEM (row=0.5, col=0.5), bilinear gives
        // (0.5*0.5)*0 + (0.5*0.5)*4 + (0.5*0.5)*8 + (0.5*0.5)*12 = 6.
        let mut dem: Array2<f64> = Array2::zeros((2, 2));
        dem[[0, 0]] = 0.0;
        dem[[0, 1]] = 4.0;
        dem[[1, 0]] = 8.0;
        dem[[1, 1]] = 12.0;

        // origin=(0,2), cell_size=1.  The centre point is at world (0.5, 1.5).
        // row_f = (2 - 1.5)/1 = 0.5, col_f = (0.5 - 0)/1 = 0.5
        let polyline: Vec<(f64, f64)> = vec![(0.5, 1.5), (0.5, 1.5)];
        // step=0.1 so that we don't hit the duplicate-segment edge case
        let profile = extract_profile(&dem, &polyline, 0.0, 2.0, 1.0, 0.1, None::<f64>);
        // polyline with identical start/end has length 0 → handled gracefully.
        // We test the helper directly instead.
        let ctx = GridContext {
            origin_x: 0.0,
            origin_y: 2.0,
            cell_size: 1.0,
        };
        let elev = bilinear_interp(&dem, &ctx, 0.5, 1.5, None::<f64>);
        assert!(
            elev.is_some(),
            "bilinear_interp returned None for in-bounds point"
        );
        let z = elev.expect("some elevation");
        assert!(
            (z - 6.0).abs() < 1.0e-12,
            "expected bilinear z=6.0, got {z}"
        );
        // profile result can be Ok or Err (zero-length segment) — just check no panic.
        let _ = profile;
    }

    #[test]
    fn test_profile_invalid_step() {
        let dem = Array2::<f64>::from_elem((5, 5), 0.0);
        let polyline: Vec<(f64, f64)> = vec![(0.0, 0.0), (4.0, 4.0)];
        let err = extract_profile(&dem, &polyline, 0.0, 5.0, 1.0, 0.0, None::<f64>)
            .expect_err("step=0 should error");
        assert!(
            matches!(err, TerrainError::ComputationError { .. }),
            "expected ComputationError, got {err:?}"
        );
    }

    #[test]
    fn test_profile_too_short_polyline() {
        let dem = Array2::<f64>::from_elem((5, 5), 0.0);
        let polyline: Vec<(f64, f64)> = vec![(0.0, 0.0)];
        let err = extract_profile(&dem, &polyline, 0.0, 5.0, 1.0, 1.0, None::<f64>)
            .expect_err("single-point polyline should error");
        assert!(
            matches!(err, TerrainError::ComputationError { .. }),
            "expected ComputationError, got {err:?}"
        );
    }
}
