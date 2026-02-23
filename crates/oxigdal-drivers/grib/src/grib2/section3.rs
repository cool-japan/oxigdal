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
                let la1 = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
                let lo1 = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
                let _resolution = cursor.read_u8()?;
                let la2 = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
                let lo2 = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
                let di = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
                let dj = cursor.read_i32::<BigEndian>()? as f64 / 1_000_000.0;
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
