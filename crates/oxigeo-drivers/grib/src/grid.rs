//! Grid definitions and coordinate transformations for GRIB formats.
//!
//! This module provides grid definition types and coordinate transformations for various
//! grid types including regular lat/lon, Lambert conformal, Mercator, polar stereographic,
//! and other common GRIB grids.

use crate::error::{GribError, Result};
use serde::{Deserialize, Serialize};

/// Grid definition for GRIB data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GridDefinition {
    /// Regular latitude/longitude grid (equidistant cylindrical projection)
    LatLon(LatLonGrid),
    /// Rotated latitude/longitude grid
    RotatedLatLon(RotatedLatLonGrid),
    /// Lambert Conformal Conic projection
    LambertConformal(LambertConformalGrid),
    /// Mercator projection
    Mercator(MercatorGrid),
    /// Polar Stereographic projection
    PolarStereographic(PolarStereographicGrid),
    /// Gaussian latitude/longitude grid
    Gaussian(GaussianGrid),
    /// Space view perspective or orthographic
    SpaceView(SpaceViewGrid),
}

/// Regular latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatLonGrid {
    /// Number of points along a parallel (longitude)
    pub ni: u32,
    /// Number of points along a meridian (latitude)
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// i direction increment (degrees)
    pub di: f64,
    /// j direction increment (degrees)
    pub dj: f64,
    /// Scanning mode flags
    pub scan_mode: ScanMode,
}

impl LatLonGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }

    /// Get latitude for grid point index
    pub fn latitude(&self, j: u32) -> Result<f64> {
        if j >= self.nj {
            return Err(GribError::OutOfRange(format!(
                "j index {} out of range [0, {})",
                j, self.nj
            )));
        }

        let lat = if self.scan_mode.j_positive {
            self.la1 + (j as f64) * self.dj
        } else {
            self.la1 - (j as f64) * self.dj
        };

        Ok(lat)
    }

    /// Get longitude for grid point index
    pub fn longitude(&self, i: u32) -> Result<f64> {
        if i >= self.ni {
            return Err(GribError::OutOfRange(format!(
                "i index {} out of range [0, {})",
                i, self.ni
            )));
        }

        let lon = if self.scan_mode.i_positive {
            self.lo1 + (i as f64) * self.di
        } else {
            self.lo1 - (i as f64) * self.di
        };

        // Normalize to [-180, 180]
        let mut lon = lon;
        while lon > 180.0 {
            lon -= 360.0;
        }
        while lon < -180.0 {
            lon += 360.0;
        }

        Ok(lon)
    }

    /// Get (lat, lon) for grid point (i, j)
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        Ok((self.latitude(j)?, self.longitude(i)?))
    }
}

/// Rotated latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotatedLatLonGrid {
    /// Base regular lat/lon grid
    pub base: LatLonGrid,
    /// Latitude of southern pole of rotation (degrees)
    pub lat_south_pole: f64,
    /// Longitude of southern pole of rotation (degrees)
    pub lon_south_pole: f64,
    /// Angle of rotation (degrees)
    pub angle: f64,
}

/// Lambert Conformal Conic projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LambertConformalGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Orientation of the grid (longitude of meridian parallel to Y-axis)
    pub lov: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// First latitude from pole at which the secant cone cuts the sphere (degrees)
    pub latin1: f64,
    /// Second latitude from pole at which the secant cone cuts the sphere (degrees)
    pub latin2: f64,
    /// Latitude of southern pole (degrees)
    pub lat_south_pole: f64,
    /// Longitude of southern pole (degrees)
    pub lon_south_pole: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl LambertConformalGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }
}

/// Mercator projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MercatorGrid {
    /// Number of points along X-axis
    pub ni: u32,
    /// Number of points along Y-axis
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// Latitude at which the Mercator projection intersects the Earth
    pub latin: f64,
    /// X-direction grid length (m)
    pub di: f64,
    /// Y-direction grid length (m)
    pub dj: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl MercatorGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }
}

/// Polar Stereographic projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolarStereographicGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Orientation of the grid (longitude where dx and dy are specified)
    pub lov: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// Projection center flag (0 = North Pole, 1 = South Pole)
    pub projection_center: u8,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl PolarStereographicGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }

    /// Check if projection is centered on North Pole
    pub fn is_north_pole(&self) -> bool {
        self.projection_center == 0
    }
}

/// Gaussian latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussianGrid {
    /// Number of points along a parallel
    pub ni: u32,
    /// Number of points along a meridian
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// i direction increment (degrees)
    pub di: f64,
    /// Number of latitude circles between pole and equator
    pub n: u32,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl GaussianGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }
}

/// Space view perspective or orthographic grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceViewGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of sub-satellite point (degrees)
    pub lap: f64,
    /// Longitude of sub-satellite point (degrees)
    pub lop: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// Altitude of camera from Earth's center (m)
    pub altitude: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl SpaceViewGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }
}

/// Scanning mode flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanMode {
    /// Points scan in +i direction (true) or -i direction (false)
    pub i_positive: bool,
    /// Points scan in +j direction (true) or -j direction (false)
    pub j_positive: bool,
    /// Adjacent points in i direction are consecutive (true) or j direction (false)
    pub consecutive_i: bool,
}

impl ScanMode {
    /// Parse scanning mode from GRIB flags byte
    pub fn from_flags(flags: u8) -> Self {
        Self {
            i_positive: (flags & 0b1000_0000) == 0,
            j_positive: (flags & 0b0100_0000) != 0,
            consecutive_i: (flags & 0b0010_0000) == 0,
        }
    }

    /// Convert to GRIB flags byte
    pub fn to_flags(&self) -> u8 {
        let mut flags = 0u8;
        if !self.i_positive {
            flags |= 0b1000_0000;
        }
        if self.j_positive {
            flags |= 0b0100_0000;
        }
        if !self.consecutive_i {
            flags |= 0b0010_0000;
        }
        flags
    }
}

impl Default for ScanMode {
    fn default() -> Self {
        Self {
            i_positive: true,
            j_positive: false,
            consecutive_i: true,
        }
    }
}

impl GridDefinition {
    /// Get the total number of grid points
    pub fn num_points(&self) -> usize {
        match self {
            Self::LatLon(g) => g.num_points(),
            Self::RotatedLatLon(g) => g.base.num_points(),
            Self::LambertConformal(g) => g.num_points(),
            Self::Mercator(g) => g.num_points(),
            Self::PolarStereographic(g) => g.num_points(),
            Self::Gaussian(g) => g.num_points(),
            Self::SpaceView(g) => g.num_points(),
        }
    }

    /// Get grid dimensions (ni/nx, nj/ny)
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::LatLon(g) => (g.ni, g.nj),
            Self::RotatedLatLon(g) => (g.base.ni, g.base.nj),
            Self::LambertConformal(g) => (g.nx, g.ny),
            Self::Mercator(g) => (g.ni, g.nj),
            Self::PolarStereographic(g) => (g.nx, g.ny),
            Self::Gaussian(g) => (g.ni, g.nj),
            Self::SpaceView(g) => (g.nx, g.ny),
        }
    }

    /// Get grid type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::LatLon(_) => "Regular Lat/Lon",
            Self::RotatedLatLon(_) => "Rotated Lat/Lon",
            Self::LambertConformal(_) => "Lambert Conformal",
            Self::Mercator(_) => "Mercator",
            Self::PolarStereographic(_) => "Polar Stereographic",
            Self::Gaussian(_) => "Gaussian",
            Self::SpaceView(_) => "Space View",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latlon_grid() {
        let grid = LatLonGrid {
            ni: 360,
            nj: 181,
            la1: 90.0,
            lo1: 0.0,
            la2: -90.0,
            lo2: 359.0,
            di: 1.0,
            dj: 1.0,
            scan_mode: ScanMode {
                i_positive: true,
                j_positive: false,
                consecutive_i: true,
            },
        };

        assert_eq!(grid.num_points(), 360 * 181);

        let lat = grid.latitude(0).expect("latitude failed");
        assert!((lat - 90.0).abs() < 1e-6);

        let lon = grid.longitude(0).expect("longitude failed");
        assert!((lon - 0.0).abs() < 1e-6);

        let (lat, lon) = grid.coordinates(180, 90).expect("coordinates failed");
        assert!((lat - 0.0).abs() < 1.1); // ~0 degrees latitude
        assert!((lon - 180.0).abs() < 1.1); // ~180 degrees longitude
    }

    #[test]
    fn test_scan_mode() {
        let mode = ScanMode::from_flags(0b0100_0000);
        assert!(mode.i_positive);
        assert!(mode.j_positive);
        assert!(mode.consecutive_i);

        let flags = mode.to_flags();
        assert_eq!(flags, 0b0100_0000);
    }

    #[test]
    fn test_grid_dimensions() {
        let grid = GridDefinition::LatLon(LatLonGrid {
            ni: 720,
            nj: 361,
            la1: 90.0,
            lo1: 0.0,
            la2: -90.0,
            lo2: 359.5,
            di: 0.5,
            dj: 0.5,
            scan_mode: ScanMode::default(),
        });

        assert_eq!(grid.dimensions(), (720, 361));
        assert_eq!(grid.num_points(), 720 * 361);
        assert_eq!(grid.type_name(), "Regular Lat/Lon");
    }
}
