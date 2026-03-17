//! Height profile extraction and ground filtering for point clouds.
//!
//! [`HeightProfile`] extracts a cross-sectional elevation profile along an
//! arbitrary 2-D transect through an [`Octree`].  [`GroundFilter`] provides
//! ASPRS-classification-based and slope-based ground / non-ground separation.

use crate::octree::Octree;
use crate::point::{BoundingBox3D, Point3D};

// ---------------------------------------------------------------------------
// HeightProfile
// ---------------------------------------------------------------------------

/// A single bin along the transect.
#[derive(Debug, Clone)]
pub struct ProfileSegment {
    /// Distance from the start of the transect to the bin centre (metres).
    pub distance_along_line: f64,
    /// All points whose 2-D projection falls inside this bin's corridor.
    pub points: Vec<Point3D>,
    /// Maximum Z value in the bin (`f64::NEG_INFINITY` when `points` is empty).
    pub max_z: f64,
    /// Minimum Z value in the bin (`f64::INFINITY` when `points` is empty).
    pub min_z: f64,
    /// Mean Z value in the bin (0.0 when `points` is empty).
    pub mean_z: f64,
    /// Number of points in the bin.
    pub point_count: usize,
}

impl ProfileSegment {
    fn from_points(distance: f64, pts: Vec<Point3D>) -> Self {
        if pts.is_empty() {
            return Self {
                distance_along_line: distance,
                points: pts,
                max_z: f64::NEG_INFINITY,
                min_z: f64::INFINITY,
                mean_z: 0.0,
                point_count: 0,
            };
        }
        let mut max_z = f64::NEG_INFINITY;
        let mut min_z = f64::INFINITY;
        let mut sum_z = 0.0_f64;
        for p in &pts {
            if p.z > max_z {
                max_z = p.z;
            }
            if p.z < min_z {
                min_z = p.z;
            }
            sum_z += p.z;
        }
        let count = pts.len();
        Self {
            distance_along_line: distance,
            max_z,
            min_z,
            mean_z: sum_z / count as f64,
            point_count: count,
            points: pts,
        }
    }
}

/// A height profile extracted along a 2-D transect through a point cloud.
pub struct HeightProfile {
    /// Ordered segments from the start to the end of the transect.
    pub segments: Vec<ProfileSegment>,
    /// Width of each bin along the transect (metres).
    pub bin_width: f64,
    /// Global minimum Z across all non-empty segments.
    pub height_min: f64,
    /// Global maximum Z across all non-empty segments.
    pub height_max: f64,
}

impl HeightProfile {
    /// Extract a height profile along a 2-D transect.
    ///
    /// # Parameters
    /// * `octree` – source point cloud.
    /// * `(x1, y1)` – start of the transect (2-D).
    /// * `(x2, y2)` – end of the transect (2-D).
    /// * `corridor_width` – half-width of the corridor on each side of the
    ///   centre line (metres).
    /// * `bin_count` – number of bins to divide the transect into.
    pub fn along_line(
        octree: &Octree,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        corridor_width: f64,
        bin_count: u32,
    ) -> Self {
        let bin_count = bin_count.max(1) as usize;

        let dx = x2 - x1;
        let dy = y2 - y1;
        let total_length = (dx * dx + dy * dy).sqrt();

        // Unit vector along and perpendicular to the transect.
        let (ux, uy) = if total_length > 0.0 {
            (dx / total_length, dy / total_length)
        } else {
            (1.0, 0.0)
        };
        // Perpendicular (rotated 90°).
        let (px, py) = (-uy, ux);

        let bin_width = if total_length > 0.0 {
            total_length / bin_count as f64
        } else {
            1.0
        };

        // Bounding box for the corridor — used to pre-filter candidates.
        let cw = corridor_width.abs();
        let all_corners = [
            (x1 + px * cw, y1 + py * cw),
            (x1 - px * cw, y1 - py * cw),
            (x2 + px * cw, y2 + py * cw),
            (x2 - px * cw, y2 - py * cw),
        ];
        let cmin_x = all_corners
            .iter()
            .map(|c| c.0)
            .fold(f64::INFINITY, f64::min);
        let cmin_y = all_corners
            .iter()
            .map(|c| c.1)
            .fold(f64::INFINITY, f64::min);
        let cmax_x = all_corners
            .iter()
            .map(|c| c.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let cmax_y = all_corners
            .iter()
            .map(|c| c.1)
            .fold(f64::NEG_INFINITY, f64::max);

        // Use the octree root bounds for Z extents.
        let root_min_z = octree
            .stats()
            .bounds
            .as_ref()
            .map(|b| b.min_z)
            .unwrap_or(f64::NEG_INFINITY);
        let root_max_z = octree
            .stats()
            .bounds
            .as_ref()
            .map(|b| b.max_z)
            .unwrap_or(f64::INFINITY);

        let candidates = if let Some(query_bbox) =
            BoundingBox3D::new(cmin_x, cmin_y, root_min_z, cmax_x, cmax_y, root_max_z)
        {
            octree.query_bbox(&query_bbox)
        } else {
            Vec::new()
        };

        // Bucket each candidate into its bin.
        let mut bins: Vec<Vec<Point3D>> = (0..bin_count).map(|_| Vec::new()).collect();

        for &pt in &candidates {
            // Project onto the transect axis.
            let rel_x = pt.x - x1;
            let rel_y = pt.y - y1;
            let along = rel_x * ux + rel_y * uy;
            let perp = rel_x * px + rel_y * py;

            // Check corridor width.
            if perp.abs() > cw {
                continue;
            }
            // Check within transect length.
            if total_length > 0.0 && (along < 0.0 || along > total_length) {
                continue;
            }

            let bin_idx = if total_length > 0.0 {
                let idx = (along / bin_width).floor() as usize;
                idx.min(bin_count - 1)
            } else {
                0
            };
            bins[bin_idx].push(pt.clone());
        }

        let segments: Vec<ProfileSegment> = bins
            .into_iter()
            .enumerate()
            .map(|(i, pts)| {
                let dist = (i as f64 + 0.5) * bin_width;
                ProfileSegment::from_points(dist, pts)
            })
            .collect();

        let height_min = segments
            .iter()
            .filter(|s| s.point_count > 0)
            .map(|s| s.min_z)
            .fold(f64::INFINITY, f64::min);
        let height_max = segments
            .iter()
            .filter(|s| s.point_count > 0)
            .map(|s| s.max_z)
            .fold(f64::NEG_INFINITY, f64::max);

        Self {
            segments,
            bin_width,
            height_min,
            height_max,
        }
    }

    /// Total length of the transect (sum of all bin widths).
    #[inline]
    pub fn total_length(&self) -> f64 {
        self.bin_width * self.segments.len() as f64
    }

    /// The highest (maximum Z) individual point across all segments.
    ///
    /// Returns `None` if no points were collected.
    pub fn highest_point(&self) -> Option<&Point3D> {
        self.segments
            .iter()
            .flat_map(|s| s.points.iter())
            .max_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal))
    }
}

// ---------------------------------------------------------------------------
// GroundFilter
// ---------------------------------------------------------------------------

/// Parameters for ground / non-ground classification.
pub struct GroundFilter {
    /// Maximum terrain slope (rise / run ratio) between adjacent grid cells.
    /// Points whose height exceeds `max_slope * cell_size` above the local
    /// minimum are considered non-ground.  Default: `0.3`.
    pub max_slope: f64,
    /// Grid cell size used to estimate the local minimum height (metres).
    /// Default: `1.0`.
    pub cell_size: f64,
    /// ASPRS LAS classification code that identifies ground points.
    /// Default: `2` (Ground).
    pub classification_code: u8,
}

impl Default for GroundFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl GroundFilter {
    /// Create a filter with default parameters.
    pub fn new() -> Self {
        Self {
            max_slope: 0.3,
            cell_size: 1.0,
            classification_code: 2,
        }
    }

    /// Separate a point slice into ground and non-ground vectors using the
    /// ASPRS classification field.
    ///
    /// Points with `classification == 2` (ground) go into the first returned
    /// `Vec`; all others go into the second.
    pub fn by_classification(points: &[Point3D]) -> (Vec<Point3D>, Vec<Point3D>) {
        let mut ground = Vec::new();
        let mut non_ground = Vec::new();
        for p in points {
            if p.classification == 2 {
                ground.push(p.clone());
            } else {
                non_ground.push(p.clone());
            }
        }
        (ground, non_ground)
    }

    /// Apply a slope-based height filter.
    ///
    /// For each point, the XY plane is divided into grid cells of size
    /// `self.cell_size`.  The minimum Z within each cell is used as the local
    /// ground estimate.  A point is flagged as ground (`true`) when its Z is
    /// within `self.max_slope * self.cell_size` of the cell minimum.
    ///
    /// Returns a `Vec<bool>` of the same length as `points`; `true` means
    /// the point is classified as ground.
    pub fn apply(&self, points: &[Point3D]) -> Vec<bool> {
        if points.is_empty() {
            return Vec::new();
        }
        if self.cell_size <= 0.0 {
            return vec![false; points.len()];
        }

        // Build grid of minimum Z per cell.
        let mut cell_min: std::collections::HashMap<(i64, i64), f64> =
            std::collections::HashMap::new();

        for p in points {
            let ix = (p.x / self.cell_size).floor() as i64;
            let iy = (p.y / self.cell_size).floor() as i64;
            let entry = cell_min.entry((ix, iy)).or_insert(f64::INFINITY);
            if p.z < *entry {
                *entry = p.z;
            }
        }

        let threshold = self.max_slope * self.cell_size;

        points
            .iter()
            .map(|p| {
                let ix = (p.x / self.cell_size).floor() as i64;
                let iy = (p.y / self.cell_size).floor() as i64;
                let min_z = cell_min.get(&(ix, iy)).copied().unwrap_or(f64::INFINITY);
                p.z - min_z <= threshold
            })
            .collect()
    }
}
