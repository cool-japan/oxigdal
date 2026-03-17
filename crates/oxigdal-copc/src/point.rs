//! LAS/LAZ point record types and 3D bounding box.
//!
//! Implements [`Point3D`] following the ASPRS LAS 1.4 specification (R15, November 2019)
//! and [`BoundingBox3D`] for spatial queries.

/// A single LAS/LAZ point record.
///
/// Covers the core fields present in all LAS point data format IDs (0–10).
/// Optional fields (`gps_time`, `red`, `green`, `blue`) are `None` for format
/// IDs that do not carry them.
#[derive(Debug, Clone, PartialEq)]
pub struct Point3D {
    /// X coordinate (scaled and offset per LAS header).
    pub x: f64,
    /// Y coordinate (scaled and offset per LAS header).
    pub y: f64,
    /// Z coordinate (scaled and offset per LAS header).
    pub z: f64,
    /// Laser pulse return intensity (0–65535).
    pub intensity: u16,
    /// Return number within the pulse (1-based, ≤ `number_of_returns`).
    pub return_number: u8,
    /// Total number of returns for this pulse.
    pub number_of_returns: u8,
    /// ASPRS classification code (see [`Point3D::classification_name`]).
    pub classification: u8,
    /// Scan angle rank in degrees (−90 to +90, rounded to integer for formats 0-5).
    pub scan_angle_rank: i8,
    /// User-defined data byte.
    pub user_data: u8,
    /// Point source ID (flight line ID for airborne surveys).
    pub point_source_id: u16,
    /// GPS time of the point (present in formats 1, 3, 5, 6–10).
    pub gps_time: Option<f64>,
    /// Red channel colour value (present in formats 2, 3, 5, 7, 8).
    pub red: Option<u16>,
    /// Green channel colour value (present in formats 2, 3, 5, 7, 8).
    pub green: Option<u16>,
    /// Blue channel colour value (present in formats 2, 3, 5, 7, 8).
    pub blue: Option<u16>,
}

impl Point3D {
    /// Create a new point at `(x, y, z)` with all other fields zeroed / `None`.
    #[inline]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            intensity: 0,
            return_number: 1,
            number_of_returns: 1,
            classification: 0,
            scan_angle_rank: 0,
            user_data: 0,
            point_source_id: 0,
            gps_time: None,
            red: None,
            green: None,
            blue: None,
        }
    }

    /// Builder: set the intensity value.
    #[inline]
    pub fn with_intensity(mut self, intensity: u16) -> Self {
        self.intensity = intensity;
        self
    }

    /// Builder: set the ASPRS classification code.
    #[inline]
    pub fn with_classification(mut self, class: u8) -> Self {
        self.classification = class;
        self
    }

    /// Builder: set red / green / blue colour values.
    #[inline]
    pub fn with_color(mut self, r: u16, g: u16, b: u16) -> Self {
        self.red = Some(r);
        self.green = Some(g);
        self.blue = Some(b);
        self
    }

    /// Builder: set the GPS timestamp.
    #[inline]
    pub fn with_gps_time(mut self, t: f64) -> Self {
        self.gps_time = Some(t);
        self
    }

    /// 3-D Euclidean distance to another point.
    #[inline]
    pub fn distance_to(&self, other: &Point3D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// 2-D horizontal distance (ignores Z) to another point.
    #[inline]
    pub fn distance_2d(&self, other: &Point3D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Human-readable name for the ASPRS LAS 1.4 classification code.
    ///
    /// Returns the standard name for codes 0–18 and `"Reserved/Unknown"` for
    /// everything else.
    pub fn classification_name(&self) -> &'static str {
        match self.classification {
            0 => "Created, Never Classified",
            1 => "Unclassified",
            2 => "Ground",
            3 => "Low Vegetation",
            4 => "Medium Vegetation",
            5 => "High Vegetation",
            6 => "Building",
            7 => "Low Point (Noise)",
            8 => "Reserved",
            9 => "Water",
            10 => "Rail",
            11 => "Road Surface",
            12 => "Reserved",
            13 => "Wire - Guard (Shield)",
            14 => "Wire - Conductor (Phase)",
            15 => "Transmission Tower",
            16 => "Wire-Structure Connector (Insulator)",
            17 => "Bridge Deck",
            18 => "High Noise",
            _ => "Reserved/Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// BoundingBox3D
// ---------------------------------------------------------------------------

/// An axis-aligned 3-D bounding box.
///
/// Invariant: `min_x ≤ max_x`, `min_y ≤ max_y`, `min_z ≤ max_z`.
/// This invariant is enforced by [`BoundingBox3D::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBox3D {
    /// Minimum X coordinate.
    pub min_x: f64,
    /// Minimum Y coordinate.
    pub min_y: f64,
    /// Minimum Z coordinate.
    pub min_z: f64,
    /// Maximum X coordinate.
    pub max_x: f64,
    /// Maximum Y coordinate.
    pub max_y: f64,
    /// Maximum Z coordinate.
    pub max_z: f64,
}

impl BoundingBox3D {
    /// Construct a new bounding box.
    ///
    /// Returns `None` if any `min > max` invariant is violated.
    pub fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Option<Self> {
        if min_x > max_x || min_y > max_y || min_z > max_z {
            return None;
        }
        Some(Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        })
    }

    /// Build the tightest bounding box that contains every point in `points`.
    ///
    /// Returns `None` when `points` is empty.
    pub fn from_points(points: &[Point3D]) -> Option<Self> {
        let first = points.first()?;
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut min_z = first.z;
        let mut max_x = first.x;
        let mut max_y = first.y;
        let mut max_z = first.z;

        for p in points.iter().skip(1) {
            if p.x < min_x {
                min_x = p.x;
            }
            if p.y < min_y {
                min_y = p.y;
            }
            if p.z < min_z {
                min_z = p.z;
            }
            if p.x > max_x {
                max_x = p.x;
            }
            if p.y > max_y {
                max_y = p.y;
            }
            if p.z > max_z {
                max_z = p.z;
            }
        }

        Some(Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        })
    }

    /// Return `true` when `p` is strictly inside or on the boundary.
    #[inline]
    pub fn contains(&self, p: &Point3D) -> bool {
        p.x >= self.min_x
            && p.x <= self.max_x
            && p.y >= self.min_y
            && p.y <= self.max_y
            && p.z >= self.min_z
            && p.z <= self.max_z
    }

    /// Return `true` when the XY footprints of `self` and `other` overlap (or
    /// touch).
    #[inline]
    pub fn intersects_2d(&self, other: &BoundingBox3D) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Return `true` when `self` and `other` share any volume (or face).
    #[inline]
    pub fn intersects_3d(&self, other: &BoundingBox3D) -> bool {
        self.intersects_2d(other) && self.min_z <= other.max_z && self.max_z >= other.min_z
    }

    /// Centre point of the box as `(cx, cy, cz)`.
    #[inline]
    pub fn center(&self) -> (f64, f64, f64) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
            (self.min_z + self.max_z) * 0.5,
        )
    }

    /// Length of the space diagonal (3-D).
    #[inline]
    pub fn diagonal(&self) -> f64 {
        let dx = self.max_x - self.min_x;
        let dy = self.max_y - self.min_y;
        let dz = self.max_z - self.min_z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Return a box expanded symmetrically in every direction by `delta`.
    ///
    /// If `delta` is negative the box may collapse; the result will still
    /// satisfy the `min ≤ max` invariant because `f64` arithmetic naturally
    /// produces equal values when expansion < 0 produces min > max (the
    /// caller should validate the result if that matters).
    #[inline]
    pub fn expand_by(&self, delta: f64) -> Self {
        Self {
            min_x: self.min_x - delta,
            min_y: self.min_y - delta,
            min_z: self.min_z - delta,
            max_x: self.max_x + delta,
            max_y: self.max_y + delta,
            max_z: self.max_z + delta,
        }
    }

    /// Volume of the box.
    #[inline]
    pub fn volume(&self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y) * (self.max_z - self.min_z)
    }

    /// Subdivide the box into eight equal octant children.
    ///
    /// Children are ordered by `(x_high, y_high, z_high)` bits:
    /// index 0 = `(lo, lo, lo)`, index 7 = `(hi, hi, hi)`.
    pub fn split_octants(&self) -> [BoundingBox3D; 8] {
        let (cx, cy, cz) = self.center();
        [
            // 0: x-lo, y-lo, z-lo
            BoundingBox3D {
                min_x: self.min_x,
                min_y: self.min_y,
                min_z: self.min_z,
                max_x: cx,
                max_y: cy,
                max_z: cz,
            },
            // 1: x-lo, y-lo, z-hi
            BoundingBox3D {
                min_x: self.min_x,
                min_y: self.min_y,
                min_z: cz,
                max_x: cx,
                max_y: cy,
                max_z: self.max_z,
            },
            // 2: x-lo, y-hi, z-lo
            BoundingBox3D {
                min_x: self.min_x,
                min_y: cy,
                min_z: self.min_z,
                max_x: cx,
                max_y: self.max_y,
                max_z: cz,
            },
            // 3: x-lo, y-hi, z-hi
            BoundingBox3D {
                min_x: self.min_x,
                min_y: cy,
                min_z: cz,
                max_x: cx,
                max_y: self.max_y,
                max_z: self.max_z,
            },
            // 4: x-hi, y-lo, z-lo
            BoundingBox3D {
                min_x: cx,
                min_y: self.min_y,
                min_z: self.min_z,
                max_x: self.max_x,
                max_y: cy,
                max_z: cz,
            },
            // 5: x-hi, y-lo, z-hi
            BoundingBox3D {
                min_x: cx,
                min_y: self.min_y,
                min_z: cz,
                max_x: self.max_x,
                max_y: cy,
                max_z: self.max_z,
            },
            // 6: x-hi, y-hi, z-lo
            BoundingBox3D {
                min_x: cx,
                min_y: cy,
                min_z: self.min_z,
                max_x: self.max_x,
                max_y: self.max_y,
                max_z: cz,
            },
            // 7: x-hi, y-hi, z-hi
            BoundingBox3D {
                min_x: cx,
                min_y: cy,
                min_z: cz,
                max_x: self.max_x,
                max_y: self.max_y,
                max_z: self.max_z,
            },
        ]
    }
}
