//! Fresnel zone clearance analysis.
//!
//! Computes the clearance between terrain and the Fresnel zone ellipsoid
//! along a TX-RX radio path, including optional Earth-curvature correction.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Speed of light in metres per second.
const SPEED_OF_LIGHT: f64 = 299_792_458.0;

/// A single clearance sample along the Fresnel path.
#[derive(Debug, Clone)]
pub struct ClearanceSample {
    /// Distance from TX along the path (map units).
    pub distance: f64,
    /// LOS elevation at this point (interpolated).
    pub los_elevation: f64,
    /// Terrain elevation at this point.
    pub terrain_elevation: f64,
    /// Fresnel zone radius at this point.
    pub fresnel_radius: f64,
    /// Clearance = (los_elevation - terrain_elevation) - fresnel_radius. Positive = clear.
    pub clearance: f64,
}

/// Result of Fresnel clearance analysis.
#[derive(Debug, Clone)]
pub struct FresnelResult {
    /// Per-sample clearance profile.
    pub samples: Vec<ClearanceSample>,
    /// Worst (minimum) clearance ratio: clearance / fresnel_radius at that point.
    /// Ratio >= 0.6 is typically considered "acceptable" first-zone clearance.
    pub worst_clearance_ratio: f64,
    /// True if any sample has clearance < 0 (terrain obstructs the Fresnel zone).
    pub is_blocked: bool,
}

/// Compute the nth Fresnel zone radius at a point along a TX-RX path.
///
/// # Arguments
/// * `freq_hz` - Frequency in Hz (e.g., 2.4e9 for 2.4 GHz Wi-Fi)
/// * `d1` - Distance from TX to the point (meters)
/// * `d2` - Distance from the point to RX (meters)
/// * `n` - Zone number (1 = first Fresnel zone, the most common)
///
/// # Returns
/// Fresnel zone radius in the same units as d1/d2.
///
/// # Examples
///
/// ```
/// use oxigdal_terrain::visibility::fresnel::fresnel_zone_radius;
/// let r = fresnel_zone_radius(2.4e9, 1000.0, 1000.0, 1);
/// assert!((r - 7.9).abs() < 0.5);
/// ```
pub fn fresnel_zone_radius(freq_hz: f64, d1: f64, d2: f64, n: u32) -> f64 {
    // r_n = sqrt(n * lambda * d1 * d2 / (d1 + d2))
    // lambda = c / freq_hz
    let lambda = SPEED_OF_LIGHT / freq_hz;
    let numerator = (n as f64) * lambda * d1 * d2;
    let denominator = d1 + d2;
    if denominator <= 0.0 {
        return 0.0;
    }
    (numerator / denominator).sqrt()
}

/// Bilinear interpolation of DEM at a real-valued (row, col) position.
///
/// Returns `None` if the position is outside the DEM bounds or is nodata.
fn bilinear_interp<T>(dem: &Array2<T>, row: f64, col: f64, nodata: Option<T>) -> Option<f64>
where
    T: Float + Into<f64> + Copy,
{
    let (nrows, ncols) = dem.dim();
    let r0 = row.floor() as isize;
    let c0 = col.floor() as isize;
    let r1 = r0 + 1;
    let c1 = c0 + 1;

    // All four corners must be in bounds.
    if r0 < 0 || c0 < 0 || r1 >= nrows as isize || c1 >= ncols as isize {
        // Fall back to nearest neighbour if near edge.
        let r = row.round() as isize;
        let c = col.round() as isize;
        if r < 0 || c < 0 || r >= nrows as isize || c >= ncols as isize {
            return None;
        }
        let v = dem[[r as usize, c as usize]];
        if let Some(nd) = nodata
            && (v - nd).abs() < T::epsilon()
        {
            return None;
        }
        return Some(v.into());
    }

    let tr = row - r0 as f64; // fractional row
    let tc = col - c0 as f64; // fractional col

    let v00 = dem[[r0 as usize, c0 as usize]];
    let v01 = dem[[r0 as usize, c1 as usize]];
    let v10 = dem[[r1 as usize, c0 as usize]];
    let v11 = dem[[r1 as usize, c1 as usize]];

    // Nodata check: if any corner is nodata, fall back to the nearest-neighbour value.
    if let Some(nd) = nodata {
        for v in [v00, v01, v10, v11] {
            if (v - nd).abs() < T::epsilon() {
                // Nearest neighbour fallback.
                let r = row.round() as usize;
                let c = col.round() as usize;
                let nv = dem[[r, c]];
                return Some(nv.into());
            }
        }
    }

    let v00: f64 = v00.into();
    let v01: f64 = v01.into();
    let v10: f64 = v10.into();
    let v11: f64 = v11.into();

    let interp = v00 * (1.0 - tr) * (1.0 - tc)
        + v01 * (1.0 - tr) * tc
        + v10 * tr * (1.0 - tc)
        + v11 * tr * tc;

    Some(interp)
}

/// Configuration for a Fresnel zone clearance analysis.
///
/// Bundles the algorithm and antenna parameters so that [`fresnel_clearance`]
/// stays within the 7-argument Clippy limit.
pub struct FresnelParams<T: Float> {
    /// Transmitter pixel position (row, col).
    pub tx: (usize, usize),
    /// Transmitter height above terrain (metres).
    pub tx_height: f64,
    /// Receiver pixel position (row, col).
    pub rx: (usize, usize),
    /// Receiver height above terrain (metres).
    pub rx_height: f64,
    /// Signal frequency in Hz.
    pub freq_hz: f64,
    /// Cell size in map units (metres).
    pub cell_size: f64,
    /// Fresnel zone number to analyse (1 = first zone).
    pub zone: u32,
    /// Optional Earth-curvature correction radius in metres.
    /// `None` disables correction; use `6_371_000.0` for the standard Earth.
    pub earth_radius: Option<f64>,
    /// Optional nodata sentinel value.
    pub nodata: Option<T>,
}

/// Analyze Fresnel zone clearance along a line-of-sight path over a DEM.
///
/// # Arguments
/// * `dem`    - Elevation raster.
/// * `params` - Algorithm and antenna parameters (see [`FresnelParams`]).
///
/// # Errors
/// Returns [`TerrainError::InvalidObserverPosition`] if TX or RX is out of bounds.
/// Returns [`TerrainError::ComputationError`] if `freq_hz <= 0` or `cell_size <= 0`.
///
/// # Examples
///
/// ```
/// use oxigdal_terrain::visibility::fresnel::{fresnel_clearance, FresnelParams};
/// use scirs2_core::ndarray::Array2;
/// let dem = Array2::from_elem((20, 20), 100.0_f64);
/// let params = FresnelParams {
///     tx: (1, 1), tx_height: 20.0, rx: (18, 18), rx_height: 20.0,
///     freq_hz: 2.4e9, cell_size: 10.0, zone: 1,
///     earth_radius: None, nodata: None,
/// };
/// let result = fresnel_clearance(&dem, params).unwrap();
/// assert!(!result.is_blocked);
/// ```
pub fn fresnel_clearance<T>(dem: &Array2<T>, params: FresnelParams<T>) -> Result<FresnelResult>
where
    T: Float + Into<f64> + Copy,
{
    let FresnelParams {
        tx,
        tx_height,
        rx,
        rx_height,
        freq_hz,
        cell_size,
        zone,
        earth_radius,
        nodata,
    } = params;
    let (nrows, ncols) = dem.dim();

    // Validate inputs.
    if tx.0 >= nrows || tx.1 >= ncols {
        return Err(TerrainError::InvalidObserverPosition { x: tx.1, y: tx.0 });
    }
    if rx.0 >= nrows || rx.1 >= ncols {
        return Err(TerrainError::InvalidObserverPosition { x: rx.1, y: rx.0 });
    }
    if freq_hz <= 0.0 {
        return Err(TerrainError::ComputationError {
            message: format!("freq_hz must be positive, got {}", freq_hz),
        });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::ComputationError {
            message: format!("cell_size must be positive, got {}", cell_size),
        });
    }

    // TX and RX absolute elevations (terrain + antenna height).
    let tx_terrain: f64 = dem[[tx.0, tx.1]].into();
    let rx_terrain: f64 = dem[[rx.0, rx.1]].into();
    let tx_elev = tx_terrain + tx_height;
    let rx_elev = rx_terrain + rx_height;

    // DDA step count based on Chebyshev distance (pixel steps).
    let delta_row = rx.0 as isize - tx.0 as isize;
    let delta_col = rx.1 as isize - tx.1 as isize;
    let steps = delta_row.abs().max(delta_col.abs());

    // Horizontal total distance in map units.
    let total_dist = ((delta_row as f64).powi(2) + (delta_col as f64).powi(2)).sqrt() * cell_size;

    // Handle the degenerate case where TX == RX.
    if steps == 0 {
        let sample = ClearanceSample {
            distance: 0.0,
            los_elevation: tx_elev,
            terrain_elevation: tx_terrain,
            fresnel_radius: 0.0,
            clearance: tx_height,
        };
        return Ok(FresnelResult {
            samples: vec![sample],
            worst_clearance_ratio: if tx_height > 0.0 { f64::INFINITY } else { 0.0 },
            is_blocked: false,
        });
    }

    let mut samples = Vec::with_capacity(steps as usize + 1);

    for i in 0..=steps {
        let t = i as f64 / steps as f64;

        // Real-valued position in DEM pixels.
        let row = tx.0 as f64 + delta_row as f64 * t;
        let col = tx.1 as f64 + delta_col as f64 * t;

        // Distances from TX and RX (metres).
        let d_tx = t * total_dist;
        let d_rx = (1.0 - t) * total_dist;

        // LOS elevation: linear interpolation along the TX-RX straight line.
        let los_elevation = tx_elev + (rx_elev - tx_elev) * t;

        // Terrain elevation via bilinear interpolation.
        let raw_terrain = bilinear_interp(dem, row, col, nodata).unwrap_or(tx_terrain);

        // Earth-curvature correction (adds to effective terrain height seen by the LOS).
        let h_curve = earth_radius
            .map(|r| {
                if r > 0.0 {
                    d_tx * d_rx / (2.0 * r)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        let terrain_elevation = raw_terrain + h_curve;

        // Fresnel zone radius (0 at endpoints, maximum at midpoint).
        let fresnel_radius = if d_tx <= 0.0 || d_rx <= 0.0 {
            0.0
        } else {
            fresnel_zone_radius(freq_hz, d_tx, d_rx, zone)
        };

        let clearance = (los_elevation - terrain_elevation) - fresnel_radius;

        samples.push(ClearanceSample {
            distance: d_tx,
            los_elevation,
            terrain_elevation,
            fresnel_radius,
            clearance,
        });
    }

    // Compute summary statistics.
    let worst_clearance_ratio = samples
        .iter()
        .map(|s| s.clearance / s.fresnel_radius.max(1e-10))
        .fold(f64::INFINITY, f64::min);

    let is_blocked = samples.iter().any(|s| s.clearance < 0.0);

    Ok(FresnelResult {
        samples,
        worst_clearance_ratio,
        is_blocked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// Helper: flat DEM of given shape and constant elevation.
    fn flat_dem(rows: usize, cols: usize, elev: f64) -> Array2<f64> {
        Array2::from_elem((rows, cols), elev)
    }

    #[test]
    fn test_fresnel_radius_known() {
        // 2.4 GHz, d1 = d2 = 1000 m, zone 1.
        // lambda = 299_792_458 / 2.4e9 ≈ 0.12491 m
        // r = sqrt(1 * 0.12491 * 1000 * 1000 / 2000) = sqrt(62.455) ≈ 7.903 m
        let r = fresnel_zone_radius(2.4e9, 1000.0, 1000.0, 1);
        assert_relative_eq!(r, 7.9, epsilon = 0.5);
    }

    #[test]
    fn test_fresnel_clear_path() {
        // Flat 30×30 DEM at elevation 100 m.  TX and RX each 20 m above terrain.
        // LOS is 120 m, Fresnel zone well above terrain → all samples clear.
        let dem = flat_dem(30, 30, 100.0);
        let result = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (1, 1),
                tx_height: 20.0,
                rx: (28, 28),
                rx_height: 20.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: None,
                nodata: None::<f64>,
            },
        )
        .expect("fresnel_clearance should succeed");

        assert!(!result.is_blocked, "path should be clear");
        for s in &result.samples {
            assert!(
                s.clearance >= 0.0,
                "every sample should have non-negative clearance, got {}",
                s.clearance
            );
        }
    }

    #[test]
    fn test_fresnel_obstructed() {
        // Flat 30×30 DEM at elevation 0 m but centre cell elevated to 50 m.
        // TX and RX at elevation 2 m → LOS at midpoint is ~2 m, obstacle is 50 m → blocked.
        let mut dem = flat_dem(30, 30, 0.0);
        dem[[15, 15]] = 50.0;

        let result = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (1, 1),
                tx_height: 2.0,
                rx: (28, 28),
                rx_height: 2.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: None,
                nodata: None::<f64>,
            },
        )
        .expect("fresnel_clearance should succeed");

        assert!(result.is_blocked, "path should be blocked by 50 m obstacle");
    }

    #[test]
    fn test_fresnel_earth_curvature() {
        // Same clear-path scenario as test_fresnel_clear_path but with Earth-curvature
        // correction enabled.  The path should still be clear (20 m antennas dominate).
        let dem = flat_dem(30, 30, 100.0);

        let no_curve = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (1, 1),
                tx_height: 20.0,
                rx: (28, 28),
                rx_height: 20.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: None,
                nodata: None::<f64>,
            },
        )
        .expect("without curvature");

        let with_curve = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (1, 1),
                tx_height: 20.0,
                rx: (28, 28),
                rx_height: 20.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: Some(6_371_000.0),
                nodata: None::<f64>,
            },
        )
        .expect("with curvature");

        // Should not be blocked in either case.
        assert!(!no_curve.is_blocked, "no-curve path should be clear");
        assert!(!with_curve.is_blocked, "curved-earth path should be clear");

        // Earth curvature adds to effective terrain height, so clearance should be
        // lower (or equal at endpoints) with curvature enabled.
        let nc_mid = &no_curve.samples[no_curve.samples.len() / 2];
        let wc_mid = &with_curve.samples[with_curve.samples.len() / 2];
        assert!(
            wc_mid.clearance <= nc_mid.clearance + 1e-9,
            "curvature correction should reduce clearance at midpoint"
        );
    }

    #[test]
    fn test_fresnel_invalid_position() {
        let dem = flat_dem(10, 10, 0.0);

        // TX out of bounds (row 10 on a 10×10 DEM).
        let err = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (10, 5),
                tx_height: 1.0,
                rx: (8, 8),
                rx_height: 1.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: None,
                nodata: None::<f64>,
            },
        );
        assert!(err.is_err(), "out-of-bounds TX should return an error");

        // RX out of bounds (col 15 on a 10×10 DEM).
        let err2 = fresnel_clearance(
            &dem,
            FresnelParams {
                tx: (2, 2),
                tx_height: 1.0,
                rx: (5, 15),
                rx_height: 1.0,
                freq_hz: 2.4e9,
                cell_size: 10.0,
                zone: 1,
                earth_radius: None,
                nodata: None::<f64>,
            },
        );
        assert!(err2.is_err(), "out-of-bounds RX should return an error");
    }
}
