//! JPEG2000 codestream parsing
//!
//! This module handles parsing of JPEG2000 codestream markers and segments.

use crate::error::{Jpeg2000Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Read;

/// JPEG2000 codestream markers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// Start of codestream (SOC)
    Soc = 0xFF4F,
    /// Start of tile-part (SOT)
    Sot = 0xFF90,
    /// Start of data (SOD)
    Sod = 0xFF93,
    /// End of codestream (EOC)
    Eoc = 0xFFD9,
    /// Image and tile size (SIZ)
    Siz = 0xFF51,
    /// Coding style default (COD)
    Cod = 0xFF52,
    /// Coding style component (COC)
    Coc = 0xFF53,
    /// Quantization default (QCD)
    Qcd = 0xFF5C,
    /// Quantization component (QCC)
    Qcc = 0xFF5D,
    /// Region of interest (RGN)
    Rgn = 0xFF5E,
    /// Progression order change (POC)
    Poc = 0xFF5F,
    /// Packet length, main header (PLM)
    Plm = 0xFF57,
    /// Packet length, tile-part header (PLT)
    Plt = 0xFF58,
    /// Packed packet headers, main header (PPM)
    Ppm = 0xFF60,
    /// Packed packet headers, tile-part header (PPT)
    Ppt = 0xFF61,
    /// Component registration (CRG)
    Crg = 0xFF63,
    /// Comment (COM)
    Com = 0xFF64,
}

impl Marker {
    /// Create marker from u16 value
    pub fn from_u16(value: u16) -> Result<Self> {
        match value {
            0xFF4F => Ok(Self::Soc),
            0xFF90 => Ok(Self::Sot),
            0xFF93 => Ok(Self::Sod),
            0xFFD9 => Ok(Self::Eoc),
            0xFF51 => Ok(Self::Siz),
            0xFF52 => Ok(Self::Cod),
            0xFF53 => Ok(Self::Coc),
            0xFF5C => Ok(Self::Qcd),
            0xFF5D => Ok(Self::Qcc),
            0xFF5E => Ok(Self::Rgn),
            0xFF5F => Ok(Self::Poc),
            0xFF57 => Ok(Self::Plm),
            0xFF58 => Ok(Self::Plt),
            0xFF60 => Ok(Self::Ppm),
            0xFF61 => Ok(Self::Ppt),
            0xFF63 => Ok(Self::Crg),
            0xFF64 => Ok(Self::Com),
            _ => Err(Jpeg2000Error::InvalidMarker(value)),
        }
    }

    /// Check if marker has associated segment
    pub fn has_segment(&self) -> bool {
        !matches!(self, Self::Soc | Self::Sod | Self::Eoc)
    }
}

/// Image and tile size parameters from SIZ marker
#[derive(Debug, Clone)]
pub struct ImageSize {
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
    /// Component parameters
    pub components: Vec<ComponentSize>,
}

/// Component size parameters
#[derive(Debug, Clone)]
pub struct ComponentSize {
    /// Precision in bits (1-38)
    pub precision: u8,
    /// Is signed
    pub is_signed: bool,
    /// Horizontal subsampling
    pub dx: u8,
    /// Vertical subsampling
    pub dy: u8,
}

impl ImageSize {
    /// Parse SIZ marker segment
    pub fn parse<R: Read>(reader: &mut R, length: u16) -> Result<Self> {
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

        if num_components == 0 {
            return Err(Jpeg2000Error::InvalidImageHeader(
                "Number of components must be > 0".to_string(),
            ));
        }

        // Length parameter has already had the 2-byte length field subtracted by read_segment_length()
        // Expected: 2 (capability) + 32 (8×4 byte fields) + 2 (num_components) + (num_components × 3)
        let expected_length = 36 + (num_components as usize * 3);
        if length as usize != expected_length {
            return Err(Jpeg2000Error::InvalidImageHeader(format!(
                "Invalid SIZ segment length: expected {}, got {}",
                expected_length, length
            )));
        }

        let mut components = Vec::with_capacity(num_components as usize);
        for _ in 0..num_components {
            let ssiz = reader.read_u8()?;
            let dx = reader.read_u8()?;
            let dy = reader.read_u8()?;

            let is_signed = (ssiz & 0x80) != 0;
            let precision = (ssiz & 0x7F) + 1;

            components.push(ComponentSize {
                precision,
                is_signed,
                dx,
                dy,
            });
        }

        Ok(Self {
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
        })
    }

    /// Calculate number of tiles in horizontal direction
    pub fn num_tiles_x(&self) -> u32 {
        (self.width + self.tile_width - 1 - self.tile_x_offset) / self.tile_width
    }

    /// Calculate number of tiles in vertical direction
    pub fn num_tiles_y(&self) -> u32 {
        (self.height + self.tile_height - 1 - self.tile_y_offset) / self.tile_height
    }

    /// Calculate total number of tiles
    pub fn num_tiles(&self) -> u32 {
        self.num_tiles_x() * self.num_tiles_y()
    }
}

/// Coding style parameters from COD marker
#[derive(Debug, Clone)]
pub struct CodingStyle {
    /// Progression order
    pub progression_order: ProgressionOrder,
    /// Number of quality layers
    pub num_layers: u16,
    /// Multiple component transform
    pub use_mct: bool,
    /// Number of decomposition levels
    pub num_levels: u8,
    /// Code-block width exponent
    pub code_block_width: u8,
    /// Code-block height exponent
    pub code_block_height: u8,
    /// Code-block style flags
    pub code_block_style: u8,
    /// Wavelet transformation
    pub wavelet: WaveletTransform,
}

/// Progression order types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressionOrder {
    /// Layer-resolution-component-position
    Lrcp = 0,
    /// Resolution-layer-component-position
    Rlcp = 1,
    /// Resolution-position-component-layer
    Rpcl = 2,
    /// Position-component-resolution-layer
    Pcrl = 3,
    /// Component-position-resolution-layer
    Cprl = 4,
}

impl ProgressionOrder {
    /// Create from u8 value
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Lrcp),
            1 => Ok(Self::Rlcp),
            2 => Ok(Self::Rpcl),
            3 => Ok(Self::Pcrl),
            4 => Ok(Self::Cprl),
            _ => Err(Jpeg2000Error::CodestreamError(format!(
                "Invalid progression order: {}",
                value
            ))),
        }
    }
}

/// Wavelet transformation types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveletTransform {
    /// 9-7 irreversible (lossy)
    Irreversible97,
    /// 5-3 reversible (lossless)
    Reversible53,
}

impl WaveletTransform {
    /// Create from u8 value
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Irreversible97),
            1 => Ok(Self::Reversible53),
            _ => Err(Jpeg2000Error::CodestreamError(format!(
                "Invalid wavelet transform: {}",
                value
            ))),
        }
    }
}

impl CodingStyle {
    /// Parse COD marker segment
    pub fn parse<R: Read>(reader: &mut R, _length: u16) -> Result<Self> {
        let scod = reader.read_u8()?;
        let progression_order = ProgressionOrder::from_u8(reader.read_u8()?)?;
        let num_layers = reader.read_u16::<BigEndian>()?;
        let mct = reader.read_u8()?;
        let num_levels = reader.read_u8()?;
        let code_block_width = reader.read_u8()?;
        let code_block_height = reader.read_u8()?;
        let code_block_style = reader.read_u8()?;
        let wavelet = WaveletTransform::from_u8(reader.read_u8()?)?;

        let use_mct = mct != 0;

        // Verify coding style flags
        if (scod & 0x01) != 0 {
            // Precinct size is defined
            return Err(Jpeg2000Error::UnsupportedFeature(
                "Custom precinct sizes not yet supported".to_string(),
            ));
        }

        Ok(Self {
            progression_order,
            num_layers,
            use_mct,
            num_levels,
            code_block_width,
            code_block_height,
            code_block_style,
            wavelet,
        })
    }
}

/// Quantization parameters from QCD marker
#[derive(Debug, Clone)]
pub struct Quantization {
    /// Quantization style
    pub style: QuantizationStyle,
    /// Step sizes
    pub step_sizes: Vec<u16>,
}

/// Quantization style types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizationStyle {
    /// No quantization
    NoQuantization,
    /// Scalar derived (used with reversible transform)
    ScalarDerived,
    /// Scalar expounded (explicit step sizes)
    ScalarExpounded,
}

impl QuantizationStyle {
    /// Create from u8 value
    pub fn from_u8(value: u8) -> Result<Self> {
        match value & 0x1F {
            0 => Ok(Self::NoQuantization),
            1 => Ok(Self::ScalarDerived),
            2 => Ok(Self::ScalarExpounded),
            _ => Err(Jpeg2000Error::CodestreamError(format!(
                "Invalid quantization style: {}",
                value
            ))),
        }
    }
}

impl Quantization {
    /// Parse QCD marker segment
    pub fn parse<R: Read>(reader: &mut R, length: u16) -> Result<Self> {
        let sqcd = reader.read_u8()?;
        let style = QuantizationStyle::from_u8(sqcd)?;

        let mut step_sizes = Vec::new();
        let remaining = (length - 1) as usize;

        match style {
            QuantizationStyle::NoQuantization | QuantizationStyle::ScalarDerived => {
                // Each step size is 1 byte
                for _ in 0..remaining {
                    step_sizes.push(u16::from(reader.read_u8()?));
                }
            }
            QuantizationStyle::ScalarExpounded => {
                // Each step size is 2 bytes
                for _ in 0..(remaining / 2) {
                    step_sizes.push(reader.read_u16::<BigEndian>()?);
                }
            }
        }

        Ok(Self { style, step_sizes })
    }
}

/// Codestream parser
pub struct CodestreamParser<R> {
    reader: R,
}

impl<R: Read> CodestreamParser<R> {
    /// Create new codestream parser
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Read next marker
    pub fn read_marker(&mut self) -> Result<Option<Marker>> {
        match self.reader.read_u16::<BigEndian>() {
            Ok(value) => Ok(Some(Marker::from_u16(value)?)),
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Read marker segment length (excluding marker)
    pub fn read_segment_length(&mut self) -> Result<u16> {
        let length = self.reader.read_u16::<BigEndian>()?;
        // Length includes the 2 bytes of the length field itself
        if length < 2 {
            return Err(Jpeg2000Error::CodestreamError(format!(
                "Invalid segment length: {}",
                length
            )));
        }
        Ok(length - 2)
    }

    /// Skip segment data
    pub fn skip_segment(&mut self, length: u16) -> Result<()> {
        let mut buffer = vec![0u8; length as usize];
        self.reader.read_exact(&mut buffer)?;
        Ok(())
    }

    /// Parse SIZ marker segment
    pub fn parse_siz(&mut self) -> Result<ImageSize> {
        let length = self.read_segment_length()?;
        ImageSize::parse(&mut self.reader, length)
    }

    /// Parse COD marker segment
    pub fn parse_cod(&mut self) -> Result<CodingStyle> {
        let length = self.read_segment_length()?;
        CodingStyle::parse(&mut self.reader, length)
    }

    /// Parse QCD marker segment
    pub fn parse_qcd(&mut self) -> Result<Quantization> {
        let length = self.read_segment_length()?;
        Quantization::parse(&mut self.reader, length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_conversion() {
        assert_eq!(Marker::from_u16(0xFF4F).ok(), Some(Marker::Soc));
        assert_eq!(Marker::from_u16(0xFFD9).ok(), Some(Marker::Eoc));
        assert!(Marker::from_u16(0x0000).is_err());
    }

    #[test]
    fn test_progression_order() {
        assert_eq!(
            ProgressionOrder::from_u8(0).ok(),
            Some(ProgressionOrder::Lrcp)
        );
        assert_eq!(
            ProgressionOrder::from_u8(4).ok(),
            Some(ProgressionOrder::Cprl)
        );
        assert!(ProgressionOrder::from_u8(5).is_err());
    }

    #[test]
    fn test_wavelet_transform() {
        assert_eq!(
            WaveletTransform::from_u8(0).ok(),
            Some(WaveletTransform::Irreversible97)
        );
        assert_eq!(
            WaveletTransform::from_u8(1).ok(),
            Some(WaveletTransform::Reversible53)
        );
        assert!(WaveletTransform::from_u8(2).is_err());
    }
}
