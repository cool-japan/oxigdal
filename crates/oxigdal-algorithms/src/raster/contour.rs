//! Contour line generation using the marching squares algorithm
//!
//! Extracts isolines (contour lines) from a 2D elevation grid at specified
//! intervals. The marching squares algorithm classifies each 2x2 cell by
//! comparing corner values to the contour level, then interpolates edge
//! crossings and chains them into polylines.

use crate::error::{AlgorithmError, Result};
use std::collections::BTreeMap;

/// A point on a contour line
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourPoint {
    pub x: f64,
    pub y: f64,
}

impl ContourPoint {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A single contour line at a specific elevation level
#[derive(Debug, Clone)]
pub struct ContourLine {
    pub level: f64,
    pub points: Vec<ContourPoint>,
    pub is_closed: bool,
}

/// Configuration for contour generation
#[derive(Debug, Clone)]
pub struct ContourConfig {
    /// Contour interval (must be > 0)
    pub interval: f64,
    /// Base level (default 0.0)
    pub base: f64,
    /// Nodata value to skip
    pub nodata: Option<f64>,
}

impl ContourConfig {
    /// Create a new contour configuration with the given interval.
    ///
    /// # Errors
    /// Returns an error if `interval` is not positive.
    pub fn new(interval: f64) -> Result<Self> {
        if interval <= 0.0 || !interval.is_finite() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "interval",
                message: format!("interval must be positive, got {interval}"),
            });
        }
        Ok(Self {
            interval,
            base: 0.0,
            nodata: None,
        })
    }

    #[must_use]
    pub fn with_base(mut self, base: f64) -> Self {
        self.base = base;
        self
    }

    #[must_use]
    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Compute the contour levels that span [min_val, max_val].
    fn compute_levels(&self, min_val: f64, max_val: f64) -> Vec<f64> {
        if min_val > max_val || !min_val.is_finite() || !max_val.is_finite() {
            return vec![];
        }

        let start = ((min_val - self.base) / self.interval).ceil();
        let end = ((max_val - self.base) / self.interval).floor();

        let start_i = start as i64;
        let end_i = end as i64;

        let mut levels = Vec::new();
        for i in start_i..=end_i {
            let level = self.base + (i as f64) * self.interval;
            if level > min_val && level < max_val {
                levels.push(level);
            }
        }
        levels
    }
}

/// A segment produced by marching squares for one cell.
#[derive(Debug, Clone, Copy)]
struct Segment {
    p0: ContourPoint,
    p1: ContourPoint,
}

/// Linearly interpolate between two values to find where `level` falls.
#[inline]
fn lerp_frac(v0: f64, v1: f64, level: f64) -> f64 {
    let denom = v1 - v0;
    if denom.abs() < 1e-15 {
        0.5
    } else {
        (level - v0) / denom
    }
}

/// Generate segments for a single cell using marching squares.
///
/// Corners ordered:
///   tl(0) --- tr(1)
///    |          |
///   bl(2) --- br(3)
///
/// Bit assignment: tl=bit3, tr=bit2, bl=bit1, br=bit0
fn cell_segments(
    col: usize,
    row: usize,
    tl: f64,
    tr: f64,
    bl: f64,
    br: f64,
    level: f64,
) -> Vec<Segment> {
    let case = ((tl >= level) as u8) << 3
        | ((tr >= level) as u8) << 2
        | ((bl >= level) as u8) << 1
        | (br >= level) as u8;

    // Edge midpoints via linear interpolation
    // top edge: tl -> tr
    let top = || {
        let t = lerp_frac(tl, tr, level);
        ContourPoint::new(col as f64 + t, row as f64)
    };
    // bottom edge: bl -> br
    let bottom = || {
        let t = lerp_frac(bl, br, level);
        ContourPoint::new(col as f64 + t, row as f64 + 1.0)
    };
    // left edge: tl -> bl
    let left = || {
        let t = lerp_frac(tl, bl, level);
        ContourPoint::new(col as f64, row as f64 + t)
    };
    // right edge: tr -> br
    let right = || {
        let t = lerp_frac(tr, br, level);
        ContourPoint::new(col as f64 + 1.0, row as f64 + t)
    };

    match case {
        0 | 15 => vec![], // all below or all above
        1 => vec![Segment {
            p0: bottom(),
            p1: right(),
        }],
        2 => vec![Segment {
            p0: left(),
            p1: bottom(),
        }],
        3 => vec![Segment {
            p0: left(),
            p1: right(),
        }],
        4 => vec![Segment {
            p0: top(),
            p1: right(),
        }],
        5 => {
            // Saddle point: disambiguate using center value
            let center = (tl + tr + bl + br) * 0.25;
            if center >= level {
                vec![
                    Segment {
                        p0: top(),
                        p1: right(),
                    },
                    Segment {
                        p0: left(),
                        p1: bottom(),
                    },
                ]
            } else {
                vec![
                    Segment {
                        p0: top(),
                        p1: left(),
                    },
                    Segment {
                        p0: bottom(),
                        p1: right(),
                    },
                ]
            }
        }
        6 => vec![Segment {
            p0: top(),
            p1: bottom(),
        }],
        7 => vec![Segment {
            p0: top(),
            p1: left(),
        }],
        8 => vec![Segment {
            p0: top(),
            p1: left(),
        }],
        9 => vec![Segment {
            p0: top(),
            p1: bottom(),
        }],
        10 => {
            // Saddle point: disambiguate using center value
            let center = (tl + tr + bl + br) * 0.25;
            if center >= level {
                vec![
                    Segment {
                        p0: top(),
                        p1: left(),
                    },
                    Segment {
                        p0: bottom(),
                        p1: right(),
                    },
                ]
            } else {
                vec![
                    Segment {
                        p0: top(),
                        p1: right(),
                    },
                    Segment {
                        p0: left(),
                        p1: bottom(),
                    },
                ]
            }
        }
        11 => vec![Segment {
            p0: top(),
            p1: right(),
        }],
        12 => vec![Segment {
            p0: left(),
            p1: right(),
        }],
        13 => vec![Segment {
            p0: left(),
            p1: bottom(),
        }],
        14 => vec![Segment {
            p0: bottom(),
            p1: right(),
        }],
        _ => vec![], // unreachable for 4-bit
    }
}

/// Check if a value is considered nodata.
#[inline]
fn is_nodata(val: f64, nodata: Option<f64>) -> bool {
    if !val.is_finite() {
        return true;
    }
    if let Some(nd) = nodata {
        (val - nd).abs() < 1e-10
    } else {
        false
    }
}

/// Chain segments into polylines by matching endpoints.
fn chain_segments(segments: &[Segment]) -> Vec<(Vec<ContourPoint>, bool)> {
    if segments.is_empty() {
        return vec![];
    }

    // We use a simple approach: maintain a list of chains, try to attach each
    // segment to an existing chain by matching endpoints.
    let eps = 1e-9;

    let points_eq = |a: &ContourPoint, b: &ContourPoint| -> bool {
        (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps
    };

    let mut chains: Vec<Vec<ContourPoint>> = Vec::new();

    for seg in segments {
        let mut attached = false;

        for chain in chains.iter_mut() {
            let first = chain[0];
            let last = chain[chain.len() - 1];

            if points_eq(&last, &seg.p0) {
                chain.push(seg.p1);
                attached = true;
                break;
            } else if points_eq(&last, &seg.p1) {
                chain.push(seg.p0);
                attached = true;
                break;
            } else if points_eq(&first, &seg.p1) {
                chain.insert(0, seg.p0);
                attached = true;
                break;
            } else if points_eq(&first, &seg.p0) {
                chain.insert(0, seg.p1);
                attached = true;
                break;
            }
        }

        if !attached {
            chains.push(vec![seg.p0, seg.p1]);
        }
    }

    // Merge chains that can be connected
    let mut merged = true;
    while merged {
        merged = false;
        let mut i = 0;
        while i < chains.len() {
            let mut j = i + 1;
            while j < chains.len() {
                let i_first = chains[i][0];
                let i_last = chains[i][chains[i].len() - 1];
                let j_first = chains[j][0];
                let j_last = chains[j][chains[j].len() - 1];

                if points_eq(&i_last, &j_first) {
                    let mut taken = chains.remove(j);
                    taken.remove(0); // avoid duplicate point
                    chains[i].append(&mut taken);
                    merged = true;
                    continue; // don't increment j, re-check
                } else if points_eq(&i_first, &j_last) {
                    let mut taken = chains.remove(j);
                    taken.pop(); // avoid duplicate point
                    taken.append(&mut chains[i]);
                    chains[i] = taken;
                    merged = true;
                    continue;
                } else if points_eq(&i_last, &j_last) {
                    let mut taken = chains.remove(j);
                    taken.reverse();
                    taken.remove(0);
                    chains[i].append(&mut taken);
                    merged = true;
                    continue;
                } else if points_eq(&i_first, &j_first) {
                    let mut taken = chains.remove(j);
                    taken.reverse();
                    taken.pop();
                    taken.append(&mut chains[i]);
                    chains[i] = taken;
                    merged = true;
                    continue;
                }
                j += 1;
            }
            i += 1;
        }
    }

    chains
        .into_iter()
        .map(|pts: Vec<ContourPoint>| {
            let closed = pts.len() >= 3 && points_eq(&pts[0], &pts[pts.len() - 1]);
            (pts, closed)
        })
        .collect()
}

/// Generate contour lines from a 2D grid using marching squares.
///
/// # Arguments
/// * `data` - Row-major 2D grid of elevation values (width * height)
/// * `width` - Grid width
/// * `height` - Grid height
/// * `config` - Contour generation configuration
///
/// # Errors
/// Returns an error if `data.len() != width * height`.
///
/// # Returns
/// A `Vec<ContourLine>` sorted by level.
pub fn generate_contours(
    data: &[f64],
    width: usize,
    height: usize,
    config: &ContourConfig,
) -> Result<Vec<ContourLine>> {
    // Empty grids produce no contours
    if width == 0 || height == 0 {
        return Ok(vec![]);
    }

    if data.len() != width * height {
        return Err(AlgorithmError::InvalidDimensions {
            message: "data length must equal width * height",
            actual: data.len(),
            expected: width * height,
        });
    }

    // Need at least a 2x2 grid for marching squares
    if width < 2 || height < 2 {
        return Ok(vec![]);
    }

    // Find min/max, skipping nodata
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for &v in data {
        if is_nodata(v, config.nodata) {
            continue;
        }
        if v < min_val {
            min_val = v;
        }
        if v > max_val {
            max_val = v;
        }
    }

    if !min_val.is_finite() || !max_val.is_finite() {
        return Ok(vec![]);
    }

    let levels = config.compute_levels(min_val, max_val);
    if levels.is_empty() {
        return Ok(vec![]);
    }

    // Group results by level for sorted output
    let mut result: BTreeMap<i64, Vec<Segment>> = BTreeMap::new();

    // For each level, run marching squares over every cell
    for &level in &levels {
        // Use a stable integer key for BTreeMap ordering
        let key = (level * 1e10) as i64;
        let segments = result.entry(key).or_default();

        for row in 0..height - 1 {
            for col in 0..width - 1 {
                let tl = data[row * width + col];
                let tr = data[row * width + col + 1];
                let bl = data[(row + 1) * width + col];
                let br = data[(row + 1) * width + col + 1];

                // Skip cell if any corner is nodata
                if is_nodata(tl, config.nodata)
                    || is_nodata(tr, config.nodata)
                    || is_nodata(bl, config.nodata)
                    || is_nodata(br, config.nodata)
                {
                    continue;
                }

                let cell_segs = cell_segments(col, row, tl, tr, bl, br, level);
                segments.extend_from_slice(&cell_segs);
            }
        }
    }

    // Chain segments into polylines per level and collect results
    let mut contour_lines: Vec<ContourLine> = Vec::new();

    for level in levels.iter() {
        let key = (*level * 1e10) as i64;
        if let Some(segments) = result.get(&key) {
            let chains: Vec<(Vec<ContourPoint>, bool)> = chain_segments(segments);
            for (points, is_closed) in chains {
                if points.len() >= 2 {
                    contour_lines.push(ContourLine {
                        level: *level,
                        points,
                        is_closed,
                    });
                }
            }
        }
    }

    Ok(contour_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contour_config_valid() {
        let config = ContourConfig::new(10.0);
        assert!(config.is_ok());
        let config = config.expect("should be ok");
        assert!((config.interval - 10.0).abs() < f64::EPSILON);
        assert!((config.base - 0.0).abs() < f64::EPSILON);
        assert!(config.nodata.is_none());
    }

    #[test]
    fn test_contour_config_zero_interval_error() {
        let result = ContourConfig::new(0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_contour_config_negative_interval_error() {
        let result = ContourConfig::new(-5.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_levels() {
        let config = ContourConfig::new(10.0).expect("valid config");
        let levels = config.compute_levels(5.0, 35.0);
        assert_eq!(levels, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn test_contour_flat_grid() {
        let data = vec![100.0; 16];
        let config = ContourConfig::new(10.0).expect("valid config");
        let contours = generate_contours(&data, 4, 4, &config).expect("should succeed");
        assert!(contours.is_empty());
    }

    #[test]
    fn test_contour_simple_slope() {
        // 4x4 grid with linear ramp from 0 to 30
        // Row 0: 0  10  20  30
        // Row 1: 0  10  20  30
        // Row 2: 0  10  20  30
        // Row 3: 0  10  20  30
        let data = vec![
            0.0, 10.0, 20.0, 30.0, 0.0, 10.0, 20.0, 30.0, 0.0, 10.0, 20.0, 30.0, 0.0, 10.0, 20.0,
            30.0,
        ];
        let config = ContourConfig::new(15.0).expect("valid config");
        let contours = generate_contours(&data, 4, 4, &config).expect("should succeed");

        // Should have contour at level 15.0 (between 0-30, strictly inside)
        assert!(!contours.is_empty());
        let levels: Vec<f64> = contours.iter().map(|c| c.level).collect();
        assert!(levels.contains(&15.0));

        // Each contour line should have at least 2 points
        for c in &contours {
            assert!(c.points.len() >= 2);
        }
    }

    #[test]
    fn test_contour_single_level() {
        // 3x3 grid:
        //  0   5  10
        //  0   5  10
        //  0   5  10
        let data = vec![0.0, 5.0, 10.0, 0.0, 5.0, 10.0, 0.0, 5.0, 10.0];
        let config = ContourConfig::new(3.0).expect("valid config");
        let contours = generate_contours(&data, 3, 3, &config).expect("should succeed");

        // Should produce contours at 3, 6, 9
        let levels: Vec<f64> = contours.iter().map(|c| c.level).collect();
        assert!(levels.contains(&3.0));
        assert!(levels.contains(&6.0));
        assert!(levels.contains(&9.0));
    }

    #[test]
    fn test_contour_nodata_handling() {
        // 3x3 grid with nodata in center
        let nodata = -9999.0;
        let data = vec![0.0, 5.0, 10.0, 0.0, nodata, 10.0, 0.0, 5.0, 10.0];
        let config = ContourConfig::new(4.0)
            .expect("valid config")
            .with_nodata(nodata);
        let contours = generate_contours(&data, 3, 3, &config).expect("should succeed");

        // Contours should still be generated, but cells touching nodata are skipped.
        // The center cell is nodata, so the 4 cells that touch it are all skipped.
        // Only corners that don't touch center could produce segments, but in a 3x3
        // grid all 4 cells touch the center — so no contours produced.
        // Actually, let's verify: cells are (0,0),(1,0),(0,1),(1,1).
        // Cell (0,0): corners are data[0],data[1],data[3],data[4]=nodata → skip
        // Cell (1,0): corners are data[1],data[2],data[4]=nodata,data[5] → skip
        // Cell (0,1): corners are data[3],data[4]=nodata,data[6],data[7] → skip
        // Cell (1,1): corners are data[4]=nodata,data[5],data[7],data[8] → skip
        // All cells skipped because they all touch center nodata.
        assert!(contours.is_empty());
    }

    #[test]
    fn test_contour_closed_loop() {
        // A "hill" grid: high value in center, low values on edges
        // 5x5 grid
        let data = vec![
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 0.0, 0.0, 5.0, 20.0, 5.0, 0.0, 0.0, 5.0,
            5.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let config = ContourConfig::new(10.0).expect("valid config");
        let contours = generate_contours(&data, 5, 5, &config).expect("should succeed");

        // Should have a contour at level 10.0
        assert!(!contours.is_empty());
        let at_10: Vec<&ContourLine> = contours
            .iter()
            .filter(|c| (c.level - 10.0).abs() < 1e-10)
            .collect();
        assert!(!at_10.is_empty());

        // The contour around the central peak should be closed
        let has_closed = at_10.iter().any(|c| c.is_closed);
        assert!(has_closed, "expected a closed contour around the hill");
    }

    #[test]
    fn test_contour_saddle_point() {
        // Create a configuration that produces saddle points (cases 5/10).
        // Diagonal pattern: high in TL and BR, low in TR and BL (or vice versa).
        // 3x3 grid:
        //  10   0   10
        //   0  10    0
        //  10   0   10
        let data = vec![10.0, 0.0, 10.0, 0.0, 10.0, 0.0, 10.0, 0.0, 10.0];
        let config = ContourConfig::new(4.0).expect("valid config");
        let contours = generate_contours(&data, 3, 3, &config).expect("should succeed");

        // Should produce contours (the saddle cells are disambiguated)
        assert!(!contours.is_empty());

        // All contour lines should have valid points
        for c in &contours {
            assert!(c.points.len() >= 2);
            for p in &c.points {
                assert!(p.x.is_finite() && p.y.is_finite());
            }
        }
    }

    #[test]
    fn test_contour_empty_grid() {
        let config = ContourConfig::new(10.0).expect("valid config");

        // Width 0
        let contours = generate_contours(&[], 0, 0, &config).expect("should succeed");
        assert!(contours.is_empty());

        // Height 0
        let contours = generate_contours(&[], 0, 5, &config).expect("should succeed");
        assert!(contours.is_empty());

        // Width 0, height > 0
        let contours = generate_contours(&[], 5, 0, &config).expect("should succeed");
        assert!(contours.is_empty());
    }

    #[test]
    fn test_contour_wrong_data_size() {
        let data = vec![1.0, 2.0, 3.0]; // 3 elements
        let config = ContourConfig::new(10.0).expect("valid config");

        // 2x2 expects 4 elements
        let result = generate_contours(&data, 2, 2, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_levels_with_base() {
        let config = ContourConfig::new(10.0)
            .expect("valid config")
            .with_base(5.0);
        let levels = config.compute_levels(0.0, 30.0);
        // base=5, interval=10, so levels: 5, 15, 25
        // But must be strictly inside (0, 30), so 5, 15, 25 all qualify
        assert!(levels.contains(&15.0));
        assert!(levels.contains(&25.0));
    }

    #[test]
    fn test_contour_config_nan_interval() {
        let result = ContourConfig::new(f64::NAN);
        assert!(result.is_err());
    }

    #[test]
    fn test_contour_config_inf_interval() {
        let result = ContourConfig::new(f64::INFINITY);
        assert!(result.is_err());
    }
}
