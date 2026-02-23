//! Data type definitions for raster and vector data
//!
//! This module provides type-safe representations of various geospatial data types,
//! supporting both raster pixel types and vector attribute types.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Raster data types representing pixel values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[derive(Default)]
pub enum RasterDataType {
    /// Unsigned 8-bit integer (0-255)
    #[default]
    UInt8 = 1,
    /// Signed 8-bit integer (-128 to 127)
    Int8 = 2,
    /// Unsigned 16-bit integer (0-65535)
    UInt16 = 3,
    /// Signed 16-bit integer
    Int16 = 4,
    /// Unsigned 32-bit integer
    UInt32 = 5,
    /// Signed 32-bit integer
    Int32 = 6,
    /// Unsigned 64-bit integer
    UInt64 = 7,
    /// Signed 64-bit integer
    Int64 = 8,
    /// 32-bit floating point
    Float32 = 9,
    /// 64-bit floating point
    Float64 = 10,
    /// Complex 32-bit floating point (real + imaginary)
    CFloat32 = 11,
    /// Complex 64-bit floating point (real + imaginary)
    CFloat64 = 12,
}

impl RasterDataType {
    /// Returns the size in bytes of this data type
    #[must_use]
    pub const fn size_bytes(self) -> usize {
        match self {
            Self::UInt8 | Self::Int8 => 1,
            Self::UInt16 | Self::Int16 => 2,
            Self::UInt32 | Self::Int32 | Self::Float32 => 4,
            Self::UInt64 | Self::Int64 | Self::Float64 | Self::CFloat32 => 8,
            Self::CFloat64 => 16,
        }
    }

    /// Returns the size in bits of this data type
    #[must_use]
    pub const fn size_bits(self) -> usize {
        self.size_bytes() * 8
    }

    /// Returns true if this is a signed type
    #[must_use]
    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            Self::Int8
                | Self::Int16
                | Self::Int32
                | Self::Int64
                | Self::Float32
                | Self::Float64
                | Self::CFloat32
                | Self::CFloat64
        )
    }

    /// Returns true if this is a floating-point type
    #[must_use]
    pub const fn is_floating_point(self) -> bool {
        matches!(
            self,
            Self::Float32 | Self::Float64 | Self::CFloat32 | Self::CFloat64
        )
    }

    /// Returns true if this is a complex type
    #[must_use]
    pub const fn is_complex(self) -> bool {
        matches!(self, Self::CFloat32 | Self::CFloat64)
    }

    /// Returns true if this is an integer type
    #[must_use]
    pub const fn is_integer(self) -> bool {
        !self.is_floating_point()
    }

    /// Returns the minimum value for this data type as f64
    #[must_use]
    pub const fn min_value(self) -> f64 {
        match self {
            Self::UInt8 => 0.0,
            Self::Int8 => i8::MIN as f64,
            Self::UInt16 => 0.0,
            Self::Int16 => i16::MIN as f64,
            Self::UInt32 => 0.0,
            Self::Int32 => i32::MIN as f64,
            Self::UInt64 => 0.0,
            Self::Int64 => i64::MIN as f64,
            Self::Float32 => f32::MIN as f64,
            Self::Float64 | Self::CFloat32 | Self::CFloat64 => f64::MIN,
        }
    }

    /// Returns the maximum value for this data type as f64
    #[must_use]
    pub const fn max_value(self) -> f64 {
        match self {
            Self::UInt8 => u8::MAX as f64,
            Self::Int8 => i8::MAX as f64,
            Self::UInt16 => u16::MAX as f64,
            Self::Int16 => i16::MAX as f64,
            Self::UInt32 => u32::MAX as f64,
            Self::Int32 => i32::MAX as f64,
            Self::UInt64 => u64::MAX as f64,
            Self::Int64 => i64::MAX as f64,
            Self::Float32 => f32::MAX as f64,
            Self::Float64 | Self::CFloat32 | Self::CFloat64 => f64::MAX,
        }
    }

    /// Converts from TIFF sample format and bits per sample
    ///
    /// # Arguments
    /// * `sample_format` - TIFF sample format tag value (1=unsigned, 2=signed, 3=float)
    /// * `bits_per_sample` - Bits per sample
    ///
    /// # Returns
    /// The corresponding `RasterDataType`, or `None` if the combination is not supported
    #[must_use]
    pub const fn from_tiff_sample_format(sample_format: u16, bits_per_sample: u16) -> Option<Self> {
        match (sample_format, bits_per_sample) {
            // Unsigned integer
            (1, 8) => Some(Self::UInt8),
            (1, 16) => Some(Self::UInt16),
            (1, 32) => Some(Self::UInt32),
            (1, 64) => Some(Self::UInt64),
            // Signed integer
            (2, 8) => Some(Self::Int8),
            (2, 16) => Some(Self::Int16),
            (2, 32) => Some(Self::Int32),
            (2, 64) => Some(Self::Int64),
            // Floating point
            (3, 32) => Some(Self::Float32),
            (3, 64) => Some(Self::Float64),
            // Complex floating point
            (6, 64) => Some(Self::CFloat32),
            (6, 128) => Some(Self::CFloat64),
            _ => None,
        }
    }

    /// Converts to TIFF sample format
    ///
    /// # Returns
    /// A tuple of (`sample_format`, `bits_per_sample`)
    #[must_use]
    pub const fn to_tiff_sample_format(self) -> (u16, u16) {
        match self {
            Self::UInt8 => (1, 8),
            Self::Int8 => (2, 8),
            Self::UInt16 => (1, 16),
            Self::Int16 => (2, 16),
            Self::UInt32 => (1, 32),
            Self::Int32 => (2, 32),
            Self::UInt64 => (1, 64),
            Self::Int64 => (2, 64),
            Self::Float32 => (3, 32),
            Self::Float64 => (3, 64),
            Self::CFloat32 => (6, 64),
            Self::CFloat64 => (6, 128),
        }
    }

    /// Returns the name of this data type
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UInt8 => "UInt8",
            Self::Int8 => "Int8",
            Self::UInt16 => "UInt16",
            Self::Int16 => "Int16",
            Self::UInt32 => "UInt32",
            Self::Int32 => "Int32",
            Self::UInt64 => "UInt64",
            Self::Int64 => "Int64",
            Self::Float32 => "Float32",
            Self::Float64 => "Float64",
            Self::CFloat32 => "CFloat32",
            Self::CFloat64 => "CFloat64",
        }
    }
}

impl fmt::Display for RasterDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Sample interpretation for raster data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[derive(Default)]
pub enum SampleInterpretation {
    /// Undefined interpretation
    #[default]
    Undefined = 0,
    /// Gray scale (min is black)
    GrayMinIsBlack = 1,
    /// Gray scale (min is white)
    GrayMinIsWhite = 2,
    /// RGB color
    Rgb = 3,
    /// Palette/color map
    Palette = 4,
    /// Transparency mask
    TransparencyMask = 5,
    /// CMYK color
    Cmyk = 6,
    /// YCbCr color
    YCbCr = 7,
    /// CIE Lab color
    CieLab = 8,
}

/// Color interpretation for individual bands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ColorInterpretation {
    /// Unknown/undefined
    #[default]
    Undefined,
    /// Grayscale
    Gray,
    /// Palette index
    PaletteIndex,
    /// Red band
    Red,
    /// Green band
    Green,
    /// Blue band
    Blue,
    /// Alpha/transparency band
    Alpha,
    /// Hue
    Hue,
    /// Saturation
    Saturation,
    /// Lightness
    Lightness,
    /// Cyan
    Cyan,
    /// Magenta
    Magenta,
    /// Yellow
    Yellow,
    /// Black (key)
    Black,
    /// Y luminance (YCbCr)
    YCbCrY,
    /// Cb chrominance (YCbCr)
    YCbCrCb,
    /// Cr chrominance (YCbCr)
    YCbCrCr,
}

/// Pixel layout in memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PixelLayout {
    /// Band sequential (BSQ): all pixels of band 1, then all pixels of band 2, etc.
    #[default]
    BandSequential,
    /// Band interleaved by line (BIL): all bands of row 1, then all bands of row 2, etc.
    BandInterleavedByLine,
    /// Band interleaved by pixel (BIP): RGB RGB RGB for each pixel
    BandInterleavedByPixel,
    /// Tiled: data organized in rectangular tiles
    Tiled {
        /// Tile width in pixels
        tile_width: u32,
        /// Tile height in pixels
        tile_height: u32,
    },
}

/// `NoData` value representation
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum NoDataValue {
    /// No `NoData` value defined
    #[default]
    None,
    /// Integer `NoData` value
    Integer(i64),
    /// Floating-point `NoData` value
    Float(f64),
}

impl NoDataValue {
    /// Returns the `NoData` value as f64, if defined
    #[must_use]
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::None => None,
            Self::Integer(v) => Some(*v as f64),
            Self::Float(v) => Some(*v),
        }
    }

    /// Returns true if this represents "no `NoData`"
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Creates a `NoData` value from an integer
    #[must_use]
    pub const fn from_integer(value: i64) -> Self {
        Self::Integer(value)
    }

    /// Creates a `NoData` value from a float
    #[must_use]
    pub const fn from_float(value: f64) -> Self {
        Self::Float(value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn test_raster_data_type_size() {
        assert_eq!(RasterDataType::UInt8.size_bytes(), 1);
        assert_eq!(RasterDataType::UInt16.size_bytes(), 2);
        assert_eq!(RasterDataType::UInt32.size_bytes(), 4);
        assert_eq!(RasterDataType::Float64.size_bytes(), 8);
        assert_eq!(RasterDataType::CFloat64.size_bytes(), 16);
    }

    #[test]
    fn test_raster_data_type_properties() {
        assert!(!RasterDataType::UInt8.is_signed());
        assert!(RasterDataType::Int8.is_signed());
        assert!(!RasterDataType::UInt8.is_floating_point());
        assert!(RasterDataType::Float32.is_floating_point());
        assert!(!RasterDataType::Float32.is_complex());
        assert!(RasterDataType::CFloat32.is_complex());
    }

    #[test]
    fn test_tiff_sample_format_roundtrip() {
        for dt in [
            RasterDataType::UInt8,
            RasterDataType::Int8,
            RasterDataType::UInt16,
            RasterDataType::Int16,
            RasterDataType::UInt32,
            RasterDataType::Int32,
            RasterDataType::Float32,
            RasterDataType::Float64,
        ] {
            let (sample_format, bits_per_sample) = dt.to_tiff_sample_format();
            let recovered = RasterDataType::from_tiff_sample_format(sample_format, bits_per_sample);
            assert_eq!(recovered, Some(dt), "Roundtrip failed for {dt:?}");
        }
    }

    #[test]
    fn test_nodata_value() {
        assert!(NoDataValue::None.is_none());
        assert!(!NoDataValue::Integer(0).is_none());

        assert_eq!(NoDataValue::Integer(-9999).as_f64(), Some(-9999.0));
        assert_eq!(
            NoDataValue::Float(f64::NAN).as_f64().map(f64::is_nan),
            Some(true)
        );
        assert_eq!(NoDataValue::None.as_f64(), None);
    }

    #[test]
    fn test_data_type_display() {
        assert_eq!(RasterDataType::UInt8.to_string(), "UInt8");
        assert_eq!(RasterDataType::Float64.to_string(), "Float64");
    }
}
