//! GRIB2 Section 3: Grid Definition Section

use crate::error::{GribError, Result};
use crate::grid::{
    GaussianGrid, GridDefinition, LambertConformalGrid, LatLonGrid, MercatorGrid,
    PolarStereographicGrid, RotatedLatLonGrid, ScanMode,
};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// GRIB2 Section 3: Grid Definition Section
///
/// Describes the grid geometry and projection used for the data.
#[derive(Debug, Clone)]
pub struct GridDefinitionSection {
    /// The grid definition (projection and dimensions)
    pub grid: GridDefinition,
    /// Total number of grid points
    pub num_points: usize,
}

impl GridDefinitionSection {
    /// Parses the grid definition section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        let _source = cursor.read_u8()?;
        let num_points = cursor.read_u32::<BigEndian>()? as usize;
        let _num_octets = cursor.read_u8()?;
        let _interp = cursor.read_u8()?;
        let template_number = cursor.read_u16::<BigEndian>()?;

        let grid = match template_number {
            0 => {
                let _radius = read_earth_shape(&mut cursor)?;
                GridDefinition::LatLon(read_latlon_body(&mut cursor)?)
            }
            1 => {
                // Rotated lat/lon: a regular lat/lon body followed by the
                // rotated south pole and the angle of rotation.
                let _radius = read_earth_shape(&mut cursor)?;
                let base = read_latlon_body(&mut cursor)?;
                let lat_south_pole = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                let lon_south_pole = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                // Angle of rotation is stored as a 32-bit IEEE float (WMO GDT 3.1).
                let angle = cursor.read_f32::<BigEndian>()? as f64;
                GridDefinition::RotatedLatLon(RotatedLatLonGrid {
                    base,
                    lat_south_pole,
                    lon_south_pole,
                    angle,
                })
            }
            10 => {
                // Mercator.
                let radius = read_earth_shape(&mut cursor)?;
                let ni = cursor.read_u32::<BigEndian>()?;
                let nj = cursor.read_u32::<BigEndian>()?;
                let la1 = read_lat(&mut cursor)?;
                let lo1 = read_lat(&mut cursor)?;
                let _res_flags = cursor.read_u8()?;
                let latin = read_lat(&mut cursor)?;
                let la2 = read_lat(&mut cursor)?;
                let lo2 = read_lat(&mut cursor)?;
                let scan_flags = cursor.read_u8()?;
                let _orientation = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                let di = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                let dj = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                GridDefinition::Mercator(MercatorGrid {
                    ni,
                    nj,
                    la1,
                    lo1,
                    la2,
                    lo2,
                    latin,
                    di,
                    dj,
                    earth_radius_m: radius,
                    scan_mode: ScanMode::from_flags(scan_flags),
                })
            }
            20 => {
                // Polar stereographic.
                let radius = read_earth_shape(&mut cursor)?;
                let nx = cursor.read_u32::<BigEndian>()?;
                let ny = cursor.read_u32::<BigEndian>()?;
                let la1 = read_lat(&mut cursor)?;
                let lo1 = read_lat(&mut cursor)?;
                let _res_flags = cursor.read_u8()?;
                let lad = read_lat(&mut cursor)?;
                let lov = read_lat(&mut cursor)?;
                let dx = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                let dy = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                let projection_center = cursor.read_u8()?;
                let scan_flags = cursor.read_u8()?;
                GridDefinition::PolarStereographic(PolarStereographicGrid {
                    nx,
                    ny,
                    la1,
                    lo1,
                    lov,
                    dx,
                    dy,
                    lad,
                    projection_center,
                    earth_radius_m: radius,
                    scan_mode: ScanMode::from_flags(scan_flags),
                })
            }
            30 => {
                // Lambert conformal conic.
                let radius = read_earth_shape(&mut cursor)?;
                let nx = cursor.read_u32::<BigEndian>()?;
                let ny = cursor.read_u32::<BigEndian>()?;
                let la1 = read_lat(&mut cursor)?;
                let lo1 = read_lat(&mut cursor)?;
                let _res_flags = cursor.read_u8()?;
                let _lad = read_lat(&mut cursor)?;
                let lov = read_lat(&mut cursor)?;
                let dx = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                let dy = cursor.read_u32::<BigEndian>()? as f64 / 1000.0;
                let _projection_center = cursor.read_u8()?;
                let scan_flags = cursor.read_u8()?;
                let latin1 = read_lat(&mut cursor)?;
                let latin2 = read_lat(&mut cursor)?;
                let lat_south_pole = read_lat(&mut cursor)?;
                let lon_south_pole = read_lat(&mut cursor)?;
                GridDefinition::LambertConformal(LambertConformalGrid {
                    nx,
                    ny,
                    la1,
                    lo1,
                    lov,
                    dx,
                    dy,
                    latin1,
                    latin2,
                    lat_south_pole,
                    lon_south_pole,
                    earth_radius_m: radius,
                    scan_mode: ScanMode::from_flags(scan_flags),
                })
            }
            40 => {
                // Gaussian lat/lon.
                let _radius = read_earth_shape(&mut cursor)?;
                let ni = cursor.read_u32::<BigEndian>()?;
                let nj = cursor.read_u32::<BigEndian>()?;
                let _basic_angle = cursor.read_u32::<BigEndian>()?;
                let _subdivisions = cursor.read_u32::<BigEndian>()?;
                let la1 = read_lat(&mut cursor)?;
                let lo1 = read_lat(&mut cursor)?;
                let _res_flags = cursor.read_u8()?;
                let la2 = read_lat(&mut cursor)?;
                let lo2 = read_lat(&mut cursor)?;
                let di = cursor.read_u32::<BigEndian>()? as f64 / 1_000_000.0;
                let n = cursor.read_u32::<BigEndian>()?;
                let scan_flags = cursor.read_u8()?;
                GridDefinition::Gaussian(GaussianGrid {
                    ni,
                    nj,
                    la1,
                    lo1,
                    la2,
                    lo2,
                    di,
                    n,
                    scan_mode: ScanMode::from_flags(scan_flags),
                })
            }
            _ => return Err(GribError::UnsupportedGridTemplate(template_number)),
        };

        Ok(Self { grid, num_points })
    }
}

/// Reads the regular lat/lon body common to GDT 3.0 and GDT 3.1 (the fields
/// after the shape-of-earth block, WMO GDT 3.0 octets 31-72).
fn read_latlon_body(cursor: &mut Cursor<&[u8]>) -> Result<LatLonGrid> {
    let ni = cursor.read_u32::<BigEndian>()?;
    let nj = cursor.read_u32::<BigEndian>()?;
    let _basic_angle = cursor.read_u32::<BigEndian>()?;
    let _subdivisions = cursor.read_u32::<BigEndian>()?;
    let la1 = read_lat(cursor)?;
    let lo1 = read_lat(cursor)?;
    let _resolution = cursor.read_u8()?;
    let la2 = read_lat(cursor)?;
    let lo2 = read_lat(cursor)?;
    // Di/Dj are `unsigned[4]` (plain unsigned, not sign-and-magnitude).
    let di = cursor.read_u32::<BigEndian>()? as f64 / 1_000_000.0;
    let dj = cursor.read_u32::<BigEndian>()? as f64 / 1_000_000.0;
    let scan_flags = cursor.read_u8()?;
    Ok(LatLonGrid {
        ni,
        nj,
        la1,
        lo1,
        la2,
        lo2,
        di,
        dj,
        scan_mode: ScanMode::from_flags(scan_flags),
    })
}

/// Reads a WMO `signed[4]` latitude/longitude field and scales it to degrees
/// (the stored value is in units of 10^-6 degrees, sign-and-magnitude).
fn read_lat(cursor: &mut Cursor<&[u8]>) -> Result<f64> {
    Ok(read_i32_sign_magnitude(cursor)? as f64 / 1_000_000.0)
}

/// Reads the shape-of-earth block (WMO GDT octets 15-30) and returns the
/// spherical Earth radius in metres used by the projection math.
///
/// Codes 0/6/8 select the WMO standard spherical radii; code 1 reads a
/// user-specified spherical radius from its scale factor / scaled value; the
/// oblate-spheroid codes (2/3/4/5/7) fall back to the WMO default mean radius
/// (6 371 229 m), since the projection math in this crate is spherical.
fn read_earth_shape(cursor: &mut Cursor<&[u8]>) -> Result<f64> {
    const DEFAULT_RADIUS_M: f64 = 6_371_229.0;

    let shape = cursor.read_u8()?;
    let sf_radius = cursor.read_u8()?;
    let scaled_radius = cursor.read_u32::<BigEndian>()?;
    let _sf_major = cursor.read_u8()?;
    let _scaled_major = cursor.read_u32::<BigEndian>()?;
    let _sf_minor = cursor.read_u8()?;
    let _scaled_minor = cursor.read_u32::<BigEndian>()?;

    let radius = match shape {
        0 => 6_367_470.0,
        1 => {
            if scaled_radius == 0 || scaled_radius == u32::MAX {
                DEFAULT_RADIUS_M
            } else {
                let factor = if sf_radius == 0xFF {
                    0
                } else {
                    sf_radius as i32
                };
                scaled_radius as f64 / 10f64.powi(factor)
            }
        }
        6 => 6_371_229.0,
        8 => 6_371_200.0,
        _ => DEFAULT_RADIUS_M,
    };
    Ok(radius)
}

/// Reads a big-endian 32-bit WMO sign-and-magnitude integer: the most
/// significant bit is a sign flag, and the remaining 31 bits hold the
/// magnitude. This is the encoding used by GRIB2 `signed[4]` fields (e.g.
/// La1/Lo1/La2/Lo2 in the grid templates), per WMO Manual on Codes
/// Regulation 92.1.5 -- it is NOT two's complement.
fn read_i32_sign_magnitude(cursor: &mut Cursor<&[u8]>) -> Result<i32> {
    let raw = cursor.read_u32::<BigEndian>()?;
    let magnitude = (raw & 0x7FFF_FFFF) as i32;
    Ok(if raw & 0x8000_0000 != 0 {
        -magnitude
    } else {
        magnitude
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_read_i32_sign_magnitude_positive() {
        let data = 45_000_000u32.to_be_bytes();
        let mut cursor = Cursor::new(&data[..]);
        let value = read_i32_sign_magnitude(&mut cursor).expect("read failed");
        assert_eq!(value, 45_000_000);
    }

    #[test]
    fn test_read_i32_sign_magnitude_negative() {
        let raw: u32 = 0x85_5D_4A_80;
        let data = raw.to_be_bytes();
        let mut cursor = Cursor::new(&data[..]);
        let value = read_i32_sign_magnitude(&mut cursor).expect("read failed");
        assert_eq!(value, -90_000_000);
    }

    /// Pushes a shape-of-earth block (code 6, spherical) to `data`.
    fn push_shape_spherical(data: &mut Vec<u8>) {
        data.push(6); // shape: spherical, radius 6371229
        data.push(0); // scale factor radius
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled radius
        data.push(0); // scale factor major
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled major
        data.push(0); // scale factor minor
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled minor
    }

    fn gds_header(num_points: u32, template: u16) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(0); // source of grid definition
        data.extend_from_slice(&num_points.to_be_bytes());
        data.push(0); // num octets for optional list
        data.push(0); // interpretation
        data.extend_from_slice(&template.to_be_bytes());
        data
    }

    #[test]
    fn test_from_bytes_gfs_style_grid_negative_la2() {
        let mut data = gds_header(65160, 0);
        push_shape_spherical(&mut data);
        data.extend_from_slice(&360u32.to_be_bytes()); // Ni
        data.extend_from_slice(&181u32.to_be_bytes()); // Nj
        data.extend_from_slice(&0u32.to_be_bytes()); // basic angle
        data.extend_from_slice(&0u32.to_be_bytes()); // subdivisions
        data.extend_from_slice(&90_000_000u32.to_be_bytes()); // La1 = 90.0
        data.extend_from_slice(&0u32.to_be_bytes()); // Lo1 = 0.0
        data.push(0); // resolution flags
        data.extend_from_slice(&(0x8000_0000u32 | 90_000_000).to_be_bytes()); // La2 = -90.0
        data.extend_from_slice(&359_000_000u32.to_be_bytes()); // Lo2 = 359.0
        data.extend_from_slice(&1_000_000u32.to_be_bytes()); // Di = 1.0
        data.extend_from_slice(&1_000_000u32.to_be_bytes()); // Dj = 1.0
        data.push(0); // scan flags

        let gds = GridDefinitionSection::from_bytes(&data).expect("failed to parse GDT 3.0");
        assert_eq!(gds.num_points, 65160);
        match gds.grid {
            GridDefinition::LatLon(grid) => {
                assert!((grid.la1 - 90.0).abs() < 1e-6);
                assert!((grid.la2 - (-90.0)).abs() < 1e-6);
                assert!((grid.lo2 - 359.0).abs() < 1e-6);
            }
            other => panic!("expected LatLon grid, got {other:?}"),
        }
    }

    /// GDT 3.30 Lambert conformal: the first grid point must round-trip back
    /// to (La1, Lo1) through the projection math.
    #[test]
    fn test_lambert_conformal_first_point_roundtrip() {
        let mut data = gds_header(4, 30);
        push_shape_spherical(&mut data);
        data.extend_from_slice(&2u32.to_be_bytes()); // Nx
        data.extend_from_slice(&2u32.to_be_bytes()); // Ny
        data.extend_from_slice(&25_000_000u32.to_be_bytes()); // La1 = 25.0
        data.extend_from_slice(&(0x8000_0000u32 | 95_000_000).to_be_bytes()); // Lo1 = -95.0
        data.push(0); // resolution flags
        data.extend_from_slice(&25_000_000u32.to_be_bytes()); // LaD = 25.0
        data.extend_from_slice(&(0x8000_0000u32 | 95_000_000).to_be_bytes()); // LoV = -95.0
        data.extend_from_slice(&3_000_000u32.to_be_bytes()); // Dx = 3000 m
        data.extend_from_slice(&3_000_000u32.to_be_bytes()); // Dy = 3000 m
        data.push(0); // projection center flag
        data.push(0b0100_0000); // scan flags: +i, +j
        data.extend_from_slice(&25_000_000u32.to_be_bytes()); // Latin1 = 25.0
        data.extend_from_slice(&25_000_000u32.to_be_bytes()); // Latin2 = 25.0
        data.extend_from_slice(&(0x8000_0000u32 | 90_000_000).to_be_bytes()); // lat SP = -90
        data.extend_from_slice(&0u32.to_be_bytes()); // lon SP = 0

        let gds = GridDefinitionSection::from_bytes(&data).expect("failed to parse GDT 3.30");
        match gds.grid {
            GridDefinition::LambertConformal(grid) => {
                let (lat, lon) = grid.coordinates(0, 0).expect("coordinates failed");
                assert!((lat - 25.0).abs() < 1e-4, "lat={lat}");
                assert!((lon - (-95.0)).abs() < 1e-4, "lon={lon}");
            }
            other => panic!("expected Lambert conformal grid, got {other:?}"),
        }
    }

    /// GDT 3.20 polar stereographic: the first grid point must round-trip back
    /// to (La1, Lo1).
    #[test]
    fn test_polar_stereographic_first_point_roundtrip() {
        let mut data = gds_header(4, 20);
        push_shape_spherical(&mut data);
        data.extend_from_slice(&2u32.to_be_bytes()); // Nx
        data.extend_from_slice(&2u32.to_be_bytes()); // Ny
        data.extend_from_slice(&60_000_000u32.to_be_bytes()); // La1 = 60.0
        data.extend_from_slice(&(0x8000_0000u32 | 150_000_000).to_be_bytes()); // Lo1 = -150.0
        data.push(0); // resolution flags
        data.extend_from_slice(&60_000_000u32.to_be_bytes()); // LaD = 60.0
        data.extend_from_slice(&(0x8000_0000u32 | 150_000_000).to_be_bytes()); // LoV = -150.0
        data.extend_from_slice(&5_000_000u32.to_be_bytes()); // Dx = 5000 m
        data.extend_from_slice(&5_000_000u32.to_be_bytes()); // Dy = 5000 m
        data.push(0); // projection center flag: north pole
        data.push(0b0100_0000); // scan flags: +i, +j

        let gds = GridDefinitionSection::from_bytes(&data).expect("failed to parse GDT 3.20");
        match gds.grid {
            GridDefinition::PolarStereographic(grid) => {
                assert!(grid.is_north_pole());
                let (lat, lon) = grid.coordinates(0, 0).expect("coordinates failed");
                assert!((lat - 60.0).abs() < 1e-4, "lat={lat}");
                assert!((lon - (-150.0)).abs() < 1e-4, "lon={lon}");
            }
            other => panic!("expected polar stereographic grid, got {other:?}"),
        }
    }

    /// GDT 3.10 Mercator: the first grid point must round-trip back to
    /// (La1, Lo1).
    #[test]
    fn test_mercator_first_point_roundtrip() {
        let mut data = gds_header(4, 10);
        push_shape_spherical(&mut data);
        data.extend_from_slice(&2u32.to_be_bytes()); // Ni
        data.extend_from_slice(&2u32.to_be_bytes()); // Nj
        data.extend_from_slice(&(0x8000_0000u32 | 10_000_000).to_be_bytes()); // La1 = -10.0
        data.extend_from_slice(&100_000_000u32.to_be_bytes()); // Lo1 = 100.0
        data.push(0); // resolution flags
        data.extend_from_slice(&0u32.to_be_bytes()); // LaD = 0.0
        data.extend_from_slice(&10_000_000u32.to_be_bytes()); // La2 = 10.0
        data.extend_from_slice(&110_000_000u32.to_be_bytes()); // Lo2 = 110.0
        data.push(0b0100_0000); // scan flags: +i, +j
        data.extend_from_slice(&0u32.to_be_bytes()); // orientation
        data.extend_from_slice(&5_000_000u32.to_be_bytes()); // Di = 5000 m
        data.extend_from_slice(&5_000_000u32.to_be_bytes()); // Dj = 5000 m

        let gds = GridDefinitionSection::from_bytes(&data).expect("failed to parse GDT 3.10");
        match gds.grid {
            GridDefinition::Mercator(grid) => {
                let (lat, lon) = grid.coordinates(0, 0).expect("coordinates failed");
                assert!((lat - (-10.0)).abs() < 1e-4, "lat={lat}");
                assert!((lon - 100.0).abs() < 1e-4, "lon={lon}");
            }
            other => panic!("expected Mercator grid, got {other:?}"),
        }
    }

    /// GDT 3.40 Gaussian: parses and yields the correct number of points; the
    /// first row latitude must be near the northern edge.
    #[test]
    fn test_gaussian_grid_parse() {
        let mut data = gds_header(8, 40);
        push_shape_spherical(&mut data);
        data.extend_from_slice(&4u32.to_be_bytes()); // Ni
        data.extend_from_slice(&2u32.to_be_bytes()); // Nj (2N, N=1)
        data.extend_from_slice(&0u32.to_be_bytes()); // basic angle
        data.extend_from_slice(&0u32.to_be_bytes()); // subdivisions
        data.extend_from_slice(&30_000_000u32.to_be_bytes()); // La1
        data.extend_from_slice(&0u32.to_be_bytes()); // Lo1
        data.push(0); // resolution flags
        data.extend_from_slice(&(0x8000_0000u32 | 30_000_000).to_be_bytes()); // La2
        data.extend_from_slice(&270_000_000u32.to_be_bytes()); // Lo2
        data.extend_from_slice(&90_000_000u32.to_be_bytes()); // Di = 90.0
        data.extend_from_slice(&1u32.to_be_bytes()); // N
        data.push(0); // scan flags: -i? actually 0 => +i,-j

        let gds = GridDefinitionSection::from_bytes(&data).expect("failed to parse GDT 3.40");
        match gds.grid {
            GridDefinition::Gaussian(grid) => {
                assert_eq!(grid.num_points(), 8);
                let lat0 = grid.latitude(0).expect("latitude failed");
                // Northern row latitude is positive for a 2-row Gaussian grid.
                assert!(lat0 > 0.0, "lat0={lat0}");
                let lat1 = grid.latitude(1).expect("latitude failed");
                assert!((lat0 + lat1).abs() < 1e-6, "gaussian lats symmetric");
            }
            other => panic!("expected Gaussian grid, got {other:?}"),
        }
    }
}
