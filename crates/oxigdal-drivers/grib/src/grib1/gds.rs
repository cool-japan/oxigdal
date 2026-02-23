//! GRIB1 Grid Definition Section (GDS).
//!
//! The GDS describes the geometry and geographical characteristics of the grid.

use crate::error::{GribError, Result};
use crate::grid::{
    GridDefinition, LambertConformalGrid, LatLonGrid, MercatorGrid, PolarStereographicGrid,
    ScanMode,
};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Read;

/// GRIB1 Grid Definition Section
#[derive(Debug, Clone)]
pub struct GridDefinitionSection {
    /// Grid definition
    pub grid: GridDefinition,
}

impl GridDefinitionSection {
    /// Parse GDS from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        // Read section length (3 bytes)
        let length_bytes = [reader.read_u8()?, reader.read_u8()?, reader.read_u8()?];
        let _length = ((length_bytes[0] as usize) << 16)
            | ((length_bytes[1] as usize) << 8)
            | (length_bytes[2] as usize);

        // NV - number of vertical coordinate parameters
        let _nv = reader.read_u8()?;

        // PV/PL - location of vertical coordinate parameters or list of numbers of points
        let _pv_pl = reader.read_u8()?;

        // Data representation type
        let grid_type = reader.read_u8()?;

        let grid = match grid_type {
            0 | 4 => {
                // Type 0: Latitude/Longitude grid
                // Type 4: Gaussian latitude/longitude grid
                Self::parse_latlon_grid(reader)?
            }
            1 => {
                // Mercator projection
                Self::parse_mercator_grid(reader)?
            }
            3 => {
                // Lambert Conformal
                Self::parse_lambert_grid(reader)?
            }
            5 => {
                // Polar Stereographic
                Self::parse_polar_stereographic_grid(reader)?
            }
            _ => {
                return Err(GribError::UnsupportedGridTemplate(grid_type as u16));
            }
        };

        Ok(Self { grid })
    }

    /// Parse regular lat/lon grid
    fn parse_latlon_grid<R: Read>(reader: &mut R) -> Result<GridDefinition> {
        let ni = reader.read_u16::<BigEndian>()? as u32;
        let nj = reader.read_u16::<BigEndian>()? as u32;
        let la1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let _resolution_flag = reader.read_u8()?;
        let la2 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo2 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let di = reader.read_u16::<BigEndian>()? as f64 / 1000.0;
        let dj = reader.read_u16::<BigEndian>()? as f64 / 1000.0;
        let scan_flags = reader.read_u8()?;

        let scan_mode = ScanMode::from_flags(scan_flags);

        Ok(GridDefinition::LatLon(LatLonGrid {
            ni,
            nj,
            la1,
            lo1,
            la2,
            lo2,
            di,
            dj,
            scan_mode,
        }))
    }

    /// Parse Mercator grid
    fn parse_mercator_grid<R: Read>(reader: &mut R) -> Result<GridDefinition> {
        let ni = reader.read_u16::<BigEndian>()? as u32;
        let nj = reader.read_u16::<BigEndian>()? as u32;
        let la1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let _resolution_flag = reader.read_u8()?;
        let la2 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo2 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let latin = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let _reserved = reader.read_u8()?;
        let scan_flags = reader.read_u8()?;
        let di = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;
        let dj = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;

        let scan_mode = ScanMode::from_flags(scan_flags);

        Ok(GridDefinition::Mercator(MercatorGrid {
            ni,
            nj,
            la1,
            lo1,
            la2,
            lo2,
            latin,
            di,
            dj,
            scan_mode,
        }))
    }

    /// Parse Lambert Conformal grid
    fn parse_lambert_grid<R: Read>(reader: &mut R) -> Result<GridDefinition> {
        let nx = reader.read_u16::<BigEndian>()? as u32;
        let ny = reader.read_u16::<BigEndian>()? as u32;
        let la1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let _resolution_flag = reader.read_u8()?;
        let lov = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let dx = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;
        let dy = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;
        let _projection_flag = reader.read_u8()?;
        let scan_flags = reader.read_u8()?;
        let latin1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let latin2 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lat_south_pole = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lon_south_pole = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;

        let scan_mode = ScanMode::from_flags(scan_flags);

        Ok(GridDefinition::LambertConformal(LambertConformalGrid {
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
            scan_mode,
        }))
    }

    /// Parse Polar Stereographic grid
    fn parse_polar_stereographic_grid<R: Read>(reader: &mut R) -> Result<GridDefinition> {
        let nx = reader.read_u16::<BigEndian>()? as u32;
        let ny = reader.read_u16::<BigEndian>()? as u32;
        let la1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let lo1 = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let _resolution_flag = reader.read_u8()?;
        let lov = ReadI24Ext::read_i24::<BigEndian>(reader)? as f64 / 1000.0;
        let dx = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;
        let dy = ReadU24Ext::read_u24::<BigEndian>(reader)? as f64 / 1000.0;
        let projection_flag = reader.read_u8()?;
        let scan_flags = reader.read_u8()?;

        let projection_center = if (projection_flag & 0b1000_0000) == 0 {
            0 // North Pole
        } else {
            1 // South Pole
        };

        let scan_mode = ScanMode::from_flags(scan_flags);

        Ok(GridDefinition::PolarStereographic(PolarStereographicGrid {
            nx,
            ny,
            la1,
            lo1,
            lov,
            dx,
            dy,
            projection_center,
            scan_mode,
        }))
    }
}

/// Extension trait for reading signed 24-bit integers
trait ReadI24Ext: Read {
    fn read_i24<T: byteorder::ByteOrder>(&mut self) -> std::io::Result<i32>;
}

impl<R: Read> ReadI24Ext for R {
    fn read_i24<T: byteorder::ByteOrder>(&mut self) -> std::io::Result<i32> {
        let mut buf = [0u8; 3];
        self.read_exact(&mut buf)?;

        let value = if T::read_u16(&[0, 0]) == 0 {
            // Big endian
            ((buf[0] as i32) << 16) | ((buf[1] as i32) << 8) | (buf[2] as i32)
        } else {
            // Little endian
            ((buf[2] as i32) << 16) | ((buf[1] as i32) << 8) | (buf[0] as i32)
        };

        // Sign extend
        if value & 0x800000 != 0 {
            Ok(value | !0xFFFFFF)
        } else {
            Ok(value)
        }
    }
}

/// Extension trait for reading unsigned 24-bit integers
trait ReadU24Ext: Read {
    fn read_u24<T: byteorder::ByteOrder>(&mut self) -> std::io::Result<u32>;
}

impl<R: Read> ReadU24Ext for R {
    fn read_u24<T: byteorder::ByteOrder>(&mut self) -> std::io::Result<u32> {
        let mut buf = [0u8; 3];
        self.read_exact(&mut buf)?;

        let value = if T::read_u16(&[0, 0]) == 0 {
            // Big endian
            ((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32)
        } else {
            // Little endian
            ((buf[2] as u32) << 16) | ((buf[1] as u32) << 8) | (buf[0] as u32)
        };

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_i24() {
        let data = [0x00, 0x01, 0x00]; // 256
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, 256);

        let data = [0xFF, 0xFF, 0xFF]; // -1
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, -1);
    }

    #[test]
    fn test_read_u24() {
        let data = [0x01, 0x00, 0x00]; // 65536
        let mut cursor = Cursor::new(&data);
        let value = ReadU24Ext::read_u24::<BigEndian>(&mut cursor).expect("read_u24 failed");
        assert_eq!(value, 65536);
    }
}
