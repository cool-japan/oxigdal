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

        // WMO sign-and-magnitude decoding (Manual on Codes / FM 92-XI Ext.
        // GRIB Regulation 92.1.5, confirmed against eccodes'
        // `grib_decode_signed_long`, definitions/grib1/grid_definition_lambert.def
        // and grid_definition_latlon.def, which declare La1/Lo1/La2/Lo2/LoV/
        // Latin1/Latin2/latitudeOfSouthernPole/longitudeOfSouthernPole as
        // `signed[3]`): the MSB of the most-significant octet is a sign
        // flag, and the remaining 23 bits are the magnitude. This is NOT
        // two's-complement sign extension.
        let magnitude = value & 0x007F_FFFF;
        if value & 0x0080_0000 != 0 {
            Ok(-magnitude)
        } else {
            Ok(magnitude)
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
        let data = [0x00, 0x01, 0x00]; // positive: 256
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, 256);

        // Sign-and-magnitude -1: sign bit set, magnitude = 1 (NOT the
        // two's-complement bit pattern for -1, which would be 0xFFFFFF).
        let data = [0x80, 0x00, 0x01];
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, -1);

        // All-magnitude-bits-set is the largest representable negative
        // magnitude under sign-and-magnitude, NOT -1 as two's complement
        // would decode it.
        let data = [0xFF, 0xFF, 0xFF];
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, -8_388_607);
    }

    /// Regression test: a stored La1 of -10.000 degrees is encoded per WMO
    /// sign-and-magnitude as sign bit set + magnitude 10000 (millidegrees),
    /// i.e. raw bytes 0x80, 0x27, 0x10. A plain two's-complement decode of
    /// the same bytes would incorrectly yield a huge negative value.
    #[test]
    fn test_read_i24_sign_magnitude_negative_coordinate() {
        let data = [0x80, 0x27, 0x10];
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, -10_000);
        assert!((value as f64 / 1000.0 - (-10.0)).abs() < 1e-9);

        // The positive counterpart (sign bit clear) must decode unchanged.
        let data = [0x00, 0x27, 0x10];
        let mut cursor = Cursor::new(&data);
        let value = ReadI24Ext::read_i24::<BigEndian>(&mut cursor).expect("read_i24 failed");
        assert_eq!(value, 10_000);
    }

    #[test]
    fn test_read_u24() {
        let data = [0x01, 0x00, 0x00]; // 65536
        let mut cursor = Cursor::new(&data);
        let value = ReadU24Ext::read_u24::<BigEndian>(&mut cursor).expect("read_u24 failed");
        assert_eq!(value, 65536);
    }

    /// Full `from_reader` round-trip for a regular lat/lon grid (type 0)
    /// spanning the southern hemisphere, confirming the whole GDS parse
    /// path (not just the low-level `read_i24` helper) produces correct
    /// coordinates for negative La1/La2.
    #[test]
    fn test_from_reader_latlon_grid_negative_coordinates() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x20]); // section length (unused)
        data.push(0); // NV
        data.push(0); // PV/PL
        data.push(0); // grid type: regular lat/lon
        data.extend_from_slice(&2u16.to_be_bytes()); // Ni
        data.extend_from_slice(&2u16.to_be_bytes()); // Nj
        data.extend_from_slice(&[0x80, 0x27, 0x10]); // La1 = -10.000
        data.extend_from_slice(&[0x02, 0x49, 0xF0]); // Lo1 = 150.000
        data.push(0); // resolution flags
        data.extend_from_slice(&[0x80, 0x4E, 0x20]); // La2 = -20.000
        data.extend_from_slice(&[0x02, 0x71, 0x00]); // Lo2 = 160.000
        data.extend_from_slice(&1000u16.to_be_bytes()); // Di
        data.extend_from_slice(&1000u16.to_be_bytes()); // Dj
        data.push(0); // scan flags

        let mut cursor = Cursor::new(data);
        let gds = GridDefinitionSection::from_reader(&mut cursor)
            .expect("failed to parse regular lat/lon GDS");

        match gds.grid {
            GridDefinition::LatLon(grid) => {
                assert!((grid.la1 - (-10.0)).abs() < 1e-9);
                assert!((grid.lo1 - 150.0).abs() < 1e-9);
                assert!((grid.la2 - (-20.0)).abs() < 1e-9);
                assert!((grid.lo2 - 160.0).abs() < 1e-9);
            }
            other => panic!("expected LatLon grid, got {other:?}"),
        }
    }
}
