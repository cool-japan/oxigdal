//! JPEG2000 codestream decoder.

use crate::error::{Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};

/// Codestream marker codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Marker {
    /// Start of codestream
    Soc = 0xFF4F,
    /// Image and tile size
    Siz = 0xFF51,
    /// Coding style default
    Cod = 0xFF52,
    /// Coding style component
    Coc = 0xFF53,
    /// Region of interest
    Rgn = 0xFF5E,
    /// Quantization default
    Qcd = 0xFF5C,
    /// Quantization component
    Qcc = 0xFF5D,
    /// Progression order change
    Poc = 0xFF5F,
    /// Packed packet headers, main header
    Ppm = 0xFF60,
    /// Packed packet headers, tile-part header
    Ppt = 0xFF61,
    /// Start of tile-part
    Sot = 0xFF90,
    /// Start of data
    Sod = 0xFF93,
    /// End of codestream
    Eoc = 0xFFD9,
    /// Comment
    Com = 0xFF64,
}

impl Marker {
    /// Try to parse marker from u16 value.
    pub fn from_u16(val: u16) -> Option<Self> {
        match val {
            0xFF4F => Some(Self::Soc),
            0xFF51 => Some(Self::Siz),
            0xFF52 => Some(Self::Cod),
            0xFF53 => Some(Self::Coc),
            0xFF5E => Some(Self::Rgn),
            0xFF5C => Some(Self::Qcd),
            0xFF5D => Some(Self::Qcc),
            0xFF5F => Some(Self::Poc),
            0xFF60 => Some(Self::Ppm),
            0xFF61 => Some(Self::Ppt),
            0xFF90 => Some(Self::Sot),
            0xFF93 => Some(Self::Sod),
            0xFFD9 => Some(Self::Eoc),
            0xFF64 => Some(Self::Com),
            _ => None,
        }
    }
}

/// Component information.
#[derive(Debug, Clone)]
pub struct ComponentInfo {
    /// Component index
    pub index: u16,
    /// Bits per sample (precision)
    pub bit_depth: u8,
    /// Is signed
    pub is_signed: bool,
    /// Horizontal separation
    pub dx: u8,
    /// Vertical separation
    pub dy: u8,
}

impl ComponentInfo {
    /// Create new component info.
    pub fn new(index: u16, bit_depth: u8, is_signed: bool) -> Self {
        Self {
            index,
            bit_depth,
            is_signed,
            dx: 1,
            dy: 1,
        }
    }

    /// Create with subsampling.
    pub fn with_subsampling(index: u16, bit_depth: u8, is_signed: bool, dx: u8, dy: u8) -> Self {
        Self {
            index,
            bit_depth,
            is_signed,
            dx,
            dy,
        }
    }
}

/// Codestream header information.
#[derive(Debug, Clone)]
pub struct CodestreamHeader {
    /// Reference grid width
    pub width: u32,
    /// Reference grid height
    pub height: u32,
    /// Horizontal offset
    pub x_offset: u32,
    /// Vertical offset
    pub y_offset: u32,
    /// Tile width
    pub tile_width: u32,
    /// Tile height
    pub tile_height: u32,
    /// Tile horizontal offset
    pub tile_x_offset: u32,
    /// Tile vertical offset
    pub tile_y_offset: u32,
    /// Number of components
    pub num_components: u16,
    /// Component information
    pub components: Vec<ComponentInfo>,
    /// Number of decomposition levels
    pub decomposition_levels: u8,
    /// Code block width exponent
    pub code_block_width: u8,
    /// Code block height exponent
    pub code_block_height: u8,
}

impl CodestreamHeader {
    /// Calculate number of tiles.
    pub fn num_tiles(&self) -> (u32, u32) {
        let tiles_x = self.width.div_ceil(self.tile_width);
        let tiles_y = self.height.div_ceil(self.tile_height);
        (tiles_x, tiles_y)
    }

    /// Get tile at index.
    pub fn tile_bounds(&self, tile_x: u32, tile_y: u32) -> Option<TileBounds> {
        let (tiles_x, tiles_y) = self.num_tiles();
        if tile_x >= tiles_x || tile_y >= tiles_y {
            return None;
        }

        let x0 = tile_x * self.tile_width;
        let y0 = tile_y * self.tile_height;
        let x1 = ((tile_x + 1) * self.tile_width).min(self.width);
        let y1 = ((tile_y + 1) * self.tile_height).min(self.height);

        Some(TileBounds {
            x0,
            y0,
            x1,
            y1,
            width: x1 - x0,
            height: y1 - y0,
        })
    }
}

/// Tile bounds.
#[derive(Debug, Clone, Copy)]
pub struct TileBounds {
    /// Left coordinate
    pub x0: u32,
    /// Top coordinate
    pub y0: u32,
    /// Right coordinate
    pub x1: u32,
    /// Bottom coordinate
    pub y1: u32,
    /// Tile width
    pub width: u32,
    /// Tile height
    pub height: u32,
}

/// JPEG2000 codestream decoder.
pub struct CodestreamDecoder {
    data: Vec<u8>,
    #[allow(dead_code)]
    position: usize,
    header: Option<CodestreamHeader>,
}

impl CodestreamDecoder {
    /// Create new codestream decoder.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            position: 0,
            header: None,
        }
    }

    /// Parse codestream header.
    pub fn parse_header(&mut self) -> Result<&CodestreamHeader> {
        let mut cursor = Cursor::new(&self.data);

        // Read SOC marker
        let marker = cursor.read_u16::<BigEndian>()?;
        if Marker::from_u16(marker) != Some(Marker::Soc) {
            return Err(Error::jpeg2000("Missing SOC marker"));
        }

        // Read SIZ marker
        let marker = cursor.read_u16::<BigEndian>()?;
        if Marker::from_u16(marker) != Some(Marker::Siz) {
            return Err(Error::jpeg2000("Missing SIZ marker"));
        }

        let header = self.parse_siz_segment(&mut cursor)?;
        self.header = Some(header);

        self.header
            .as_ref()
            .ok_or_else(|| Error::jpeg2000("No header"))
    }

    /// Parse SIZ marker segment.
    fn parse_siz_segment<R: Read>(&self, reader: &mut R) -> Result<CodestreamHeader> {
        let length = reader.read_u16::<BigEndian>()?;
        if length < 41 {
            return Err(Error::jpeg2000("Invalid SIZ segment length"));
        }

        let _capability = reader.read_u16::<BigEndian>()?;
        let width = reader.read_u32::<BigEndian>()?;
        let height = reader.read_u32::<BigEndian>()?;
        let x_offset = reader.read_u32::<BigEndian>()?;
        let y_offset = reader.read_u32::<BigEndian>()?;
        let tile_width = reader.read_u32::<BigEndian>()?;
        let tile_height = reader.read_u32::<BigEndian>()?;
        let tile_x_offset = reader.read_u32::<BigEndian>()?;
        let tile_y_offset = reader.read_u32::<BigEndian>()?;
        let num_components = reader.read_u16::<BigEndian>()?;

        let mut components = Vec::with_capacity(num_components as usize);
        for i in 0..num_components {
            let ssiz = reader.read_u8()?;
            let is_signed = (ssiz & 0x80) != 0;
            let bit_depth = (ssiz & 0x7F) + 1;
            let dx = reader.read_u8()?;
            let dy = reader.read_u8()?;

            components.push(ComponentInfo::with_subsampling(
                i, bit_depth, is_signed, dx, dy,
            ));
        }

        Ok(CodestreamHeader {
            width,
            height,
            x_offset,
            y_offset,
            tile_width,
            tile_height,
            tile_x_offset,
            tile_y_offset,
            num_components,
            components,
            decomposition_levels: 5, // Default, should be parsed from COD
            code_block_width: 6,     // Default (2^6 = 64)
            code_block_height: 6,    // Default
        })
    }

    /// Decode to raw image data (simplified).
    /// This is a placeholder for a full JPEG2000 decoder.
    /// In production, you would use a complete wavelet decoder here.
    pub fn decode(&mut self) -> Result<Vec<u8>> {
        let header = self.parse_header()?;

        let width = header.width as usize;
        let height = header.height as usize;
        let num_components = header.num_components as usize;

        // Placeholder: Create empty image
        // In a real implementation, this would decode the wavelet transform,
        // dequantize, and reconstruct the image.
        let size = width * height * num_components;
        let data = vec![128u8; size]; // Gray image as placeholder

        tracing::warn!(
            "JPEG2000 decoding is simplified - returning placeholder data ({}x{}, {} components)",
            width,
            height,
            num_components
        );

        Ok(data)
    }

    /// Get parsed header.
    pub fn header(&self) -> Option<&CodestreamHeader> {
        self.header.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_parsing() {
        assert_eq!(Marker::from_u16(0xFF4F), Some(Marker::Soc));
        assert_eq!(Marker::from_u16(0xFF51), Some(Marker::Siz));
        assert_eq!(Marker::from_u16(0xFFD9), Some(Marker::Eoc));
        assert_eq!(Marker::from_u16(0x0000), None);
    }

    #[test]
    fn test_component_info() {
        let comp = ComponentInfo::new(0, 8, false);
        assert_eq!(comp.index, 0);
        assert_eq!(comp.bit_depth, 8);
        assert!(!comp.is_signed);
        assert_eq!(comp.dx, 1);
        assert_eq!(comp.dy, 1);
    }

    #[test]
    fn test_component_info_with_subsampling() {
        let comp = ComponentInfo::with_subsampling(1, 8, false, 2, 2);
        assert_eq!(comp.index, 1);
        assert_eq!(comp.dx, 2);
        assert_eq!(comp.dy, 2);
    }

    #[test]
    fn test_tile_bounds_calculation() {
        let header = CodestreamHeader {
            width: 1024,
            height: 768,
            x_offset: 0,
            y_offset: 0,
            tile_width: 512,
            tile_height: 512,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
            decomposition_levels: 5,
            code_block_width: 6,
            code_block_height: 6,
        };

        let (tiles_x, tiles_y) = header.num_tiles();
        assert_eq!(tiles_x, 2);
        assert_eq!(tiles_y, 2);

        let bounds = header.tile_bounds(0, 0);
        assert!(bounds.is_some());
        if let Some(b) = bounds {
            assert_eq!(b.width, 512);
            assert_eq!(b.height, 512);
        }
    }
}
