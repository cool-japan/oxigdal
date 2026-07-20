//! GRIB2 Section 3: Grid Definition Section

use crate::error::{GribError, Result};
use crate::grid::{GridDefinition, LatLonGrid, ScanMode};
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
                // Regular lat/lon
                let _shape = cursor.read_u8()?;
                let _scale_factor_radius = cursor.read_u8()?;
                let _scaled_radius = cursor.read_u32::<BigEndian>()?;
                let _scale_factor_major = cursor.read_u8()?;
                let _scaled_major = cursor.read_u32::<BigEndian>()?;
                let _scale_factor_minor = cursor.read_u8()?;
                let _scaled_minor = cursor.read_u32::<BigEndian>()?;
                let ni = cursor.read_u32::<BigEndian>()?;
                let nj = cursor.read_u32::<BigEndian>()?;
                let _basic_angle = cursor.read_u32::<BigEndian>()?;
                let _subdivisions = cursor.read_u32::<BigEndian>()?;
                // La1/Lo1/La2/Lo2 are `signed[4]` per the WMO Manual on
                // Codes (Regulation 92.1.5) and eccodes'
                // grib2/templates/template.3.grid.def: sign-and-magnitude,
                // not two's complement.
                let la1 = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                let lo1 = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                let _resolution = cursor.read_u8()?;
                let la2 = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                let lo2 = read_i32_sign_magnitude(&mut cursor)? as f64 / 1_000_000.0;
                // Di/Dj are `unsigned[4]` per the same eccodes template
                // (template.3.latlon.def: `unsigned[4] iDirectionIncrement`
                // / `jDirectionIncrement`) -- plain unsigned, not
                // sign-and-magnitude, unlike the lat/lon fields above.
                let di = cursor.read_u32::<BigEndian>()? as f64 / 1_000_000.0;
                let dj = cursor.read_u32::<BigEndian>()? as f64 / 1_000_000.0;
                let scan_flags = cursor.read_u8()?;

                GridDefinition::LatLon(LatLonGrid {
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
            _ => return Err(GribError::UnsupportedGridTemplate(template_number)),
        };

        Ok(Self { grid, num_points })
    }
}

/// Reads a big-endian 32-bit WMO sign-and-magnitude integer: the most
/// significant bit is a sign flag, and the remaining 31 bits hold the
/// magnitude. This is the encoding used by GRIB2 `signed[4]` fields (e.g.
/// La1/Lo1/La2/Lo2 in Grid Definition Template 3.0), per WMO Manual on
/// Codes Regulation 92.1.5 -- it is NOT two's complement.
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
        // raw = 0x85_5D_4A_80: sign bit set, magnitude 0x05_5D_4A_80 =
        // 90,000,000. Under two's complement this raw pattern would decode
        // to a wildly different (huge negative) value.
        let raw: u32 = 0x85_5D_4A_80;
        let data = raw.to_be_bytes();
        let mut cursor = Cursor::new(&data[..]);
        let value = read_i32_sign_magnitude(&mut cursor).expect("read failed");
        assert_eq!(value, -90_000_000);
    }

    /// Full `from_bytes()` round-trip on a synthetic GDT 3.0 byte buffer
    /// modeling a GFS-style global grid: La1=90.0 (north pole, positive),
    /// La2=-90.0 (south pole, sign bit set). Confirms the whole parse path
    /// (not just the low-level helper) produces the correct `LatLonGrid`.
    #[test]
    fn test_from_bytes_gfs_style_grid_negative_la2() {
        let mut data = Vec::new();
        data.push(0); // source of grid definition
        data.extend_from_slice(&65160u32.to_be_bytes()); // num_points
        data.push(0); // num octets for optional list
        data.push(0); // interpretation
        data.extend_from_slice(&0u16.to_be_bytes()); // template 3.0

        data.push(6); // shape of earth: spherical
        data.push(0); // scale factor radius
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled radius
        data.push(0); // scale factor major
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled major
        data.push(0); // scale factor minor
        data.extend_from_slice(&0u32.to_be_bytes()); // scaled minor
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
                assert!((grid.lo1 - 0.0).abs() < 1e-6);
                assert!((grid.lo2 - 359.0).abs() < 1e-6);
                assert!((grid.di - 1.0).abs() < 1e-6);
                assert!((grid.dj - 1.0).abs() < 1e-6);
            }
            other => panic!("expected LatLon grid, got {other:?}"),
        }
    }
}
