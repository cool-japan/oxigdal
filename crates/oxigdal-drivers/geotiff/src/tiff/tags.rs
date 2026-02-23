//! TIFF tag definitions
//!
//! This module defines the standard TIFF tags and GeoTIFF-specific tags.

/// Standard TIFF tags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum TiffTag {
    // Baseline TIFF tags
    /// Image width in pixels
    ImageWidth = 256,
    /// Image height in pixels
    ImageLength = 257,
    /// Bits per sample (component)
    BitsPerSample = 258,
    /// Compression scheme
    Compression = 259,
    /// Photometric interpretation
    PhotometricInterpretation = 262,
    /// Strip offsets
    StripOffsets = 273,
    /// Samples per pixel
    SamplesPerPixel = 277,
    /// Rows per strip
    RowsPerStrip = 278,
    /// Strip byte counts
    StripByteCounts = 279,
    /// X resolution
    XResolution = 282,
    /// Y resolution
    YResolution = 283,
    /// Planar configuration
    PlanarConfiguration = 284,
    /// Resolution unit
    ResolutionUnit = 296,
    /// Software
    Software = 305,
    /// DateTime
    DateTime = 306,
    /// Predictor for compression
    Predictor = 317,
    /// Color map for palette images
    ColorMap = 320,
    /// Tile width
    TileWidth = 322,
    /// Tile height (length)
    TileLength = 323,
    /// Tile offsets
    TileOffsets = 324,
    /// Tile byte counts
    TileByteCounts = 325,
    /// Sample format (1=uint, 2=int, 3=float, 6=complex)
    SampleFormat = 339,
    /// Min sample value
    SMinSampleValue = 340,
    /// Max sample value
    SMaxSampleValue = 341,
    /// Sub-IFD pointers (for multi-page/pyramid images)
    SubIfd = 330,
    /// JPEG tables
    JpegTables = 347,
    /// YCbCr coefficients
    YCbCrCoefficients = 529,
    /// YCbCr subsampling
    YCbCrSubSampling = 530,
    /// YCbCr positioning
    YCbCrPositioning = 531,

    // GeoTIFF tags
    /// Model pixel scale
    ModelPixelScale = 33550,
    /// Model tiepoint
    ModelTiepoint = 33922,
    /// Model transformation (4x4 matrix)
    ModelTransformation = 34264,
    /// GeoKey directory
    GeoKeyDirectory = 34735,
    /// GeoDouble params
    GeoDoubleParams = 34736,
    /// GeoAscii params
    GeoAsciiParams = 34737,

    // GDAL metadata tags
    /// GDAL metadata
    GdalMetadata = 42112,
    /// GDAL NoData
    GdalNodata = 42113,

    // COG-specific
    /// Ghost area marker
    GhostArea = 65535,
}

impl TiffTag {
    /// Creates a TiffTag from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            256 => Some(Self::ImageWidth),
            257 => Some(Self::ImageLength),
            258 => Some(Self::BitsPerSample),
            259 => Some(Self::Compression),
            262 => Some(Self::PhotometricInterpretation),
            273 => Some(Self::StripOffsets),
            277 => Some(Self::SamplesPerPixel),
            278 => Some(Self::RowsPerStrip),
            279 => Some(Self::StripByteCounts),
            282 => Some(Self::XResolution),
            283 => Some(Self::YResolution),
            284 => Some(Self::PlanarConfiguration),
            296 => Some(Self::ResolutionUnit),
            305 => Some(Self::Software),
            306 => Some(Self::DateTime),
            317 => Some(Self::Predictor),
            320 => Some(Self::ColorMap),
            322 => Some(Self::TileWidth),
            323 => Some(Self::TileLength),
            324 => Some(Self::TileOffsets),
            325 => Some(Self::TileByteCounts),
            330 => Some(Self::SubIfd),
            339 => Some(Self::SampleFormat),
            340 => Some(Self::SMinSampleValue),
            341 => Some(Self::SMaxSampleValue),
            347 => Some(Self::JpegTables),
            529 => Some(Self::YCbCrCoefficients),
            530 => Some(Self::YCbCrSubSampling),
            531 => Some(Self::YCbCrPositioning),
            33550 => Some(Self::ModelPixelScale),
            33922 => Some(Self::ModelTiepoint),
            34264 => Some(Self::ModelTransformation),
            34735 => Some(Self::GeoKeyDirectory),
            34736 => Some(Self::GeoDoubleParams),
            34737 => Some(Self::GeoAsciiParams),
            42112 => Some(Self::GdalMetadata),
            42113 => Some(Self::GdalNodata),
            65535 => Some(Self::GhostArea),
            _ => None,
        }
    }

    /// Returns the name of this tag
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ImageWidth => "ImageWidth",
            Self::ImageLength => "ImageLength",
            Self::BitsPerSample => "BitsPerSample",
            Self::Compression => "Compression",
            Self::PhotometricInterpretation => "PhotometricInterpretation",
            Self::StripOffsets => "StripOffsets",
            Self::SamplesPerPixel => "SamplesPerPixel",
            Self::RowsPerStrip => "RowsPerStrip",
            Self::StripByteCounts => "StripByteCounts",
            Self::XResolution => "XResolution",
            Self::YResolution => "YResolution",
            Self::PlanarConfiguration => "PlanarConfiguration",
            Self::ResolutionUnit => "ResolutionUnit",
            Self::Software => "Software",
            Self::DateTime => "DateTime",
            Self::Predictor => "Predictor",
            Self::ColorMap => "ColorMap",
            Self::TileWidth => "TileWidth",
            Self::TileLength => "TileLength",
            Self::TileOffsets => "TileOffsets",
            Self::TileByteCounts => "TileByteCounts",
            Self::SubIfd => "SubIFD",
            Self::SampleFormat => "SampleFormat",
            Self::SMinSampleValue => "SMinSampleValue",
            Self::SMaxSampleValue => "SMaxSampleValue",
            Self::JpegTables => "JPEGTables",
            Self::YCbCrCoefficients => "YCbCrCoefficients",
            Self::YCbCrSubSampling => "YCbCrSubSampling",
            Self::YCbCrPositioning => "YCbCrPositioning",
            Self::ModelPixelScale => "ModelPixelScale",
            Self::ModelTiepoint => "ModelTiepoint",
            Self::ModelTransformation => "ModelTransformation",
            Self::GeoKeyDirectory => "GeoKeyDirectory",
            Self::GeoDoubleParams => "GeoDoubleParams",
            Self::GeoAsciiParams => "GeoAsciiParams",
            Self::GdalMetadata => "GDAL_METADATA",
            Self::GdalNodata => "GDAL_NODATA",
            Self::GhostArea => "GhostArea",
        }
    }

    /// Returns true if this is a GeoTIFF tag
    #[must_use]
    pub const fn is_geotiff_tag(self) -> bool {
        matches!(
            self,
            Self::ModelPixelScale
                | Self::ModelTiepoint
                | Self::ModelTransformation
                | Self::GeoKeyDirectory
                | Self::GeoDoubleParams
                | Self::GeoAsciiParams
        )
    }
}

/// TIFF compression schemes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Compression {
    /// No compression
    None = 1,
    /// CCITT Huffman RLE
    CcittRle = 2,
    /// CCITT Group 3 fax
    CcittFax3 = 3,
    /// CCITT Group 4 fax
    CcittFax4 = 4,
    /// LZW compression
    Lzw = 5,
    /// JPEG (old-style)
    OldJpeg = 6,
    /// JPEG compression
    Jpeg = 7,
    /// Adobe DEFLATE (zlib)
    AdobeDeflate = 8,
    /// JBIG (T.85)
    Jbig = 9,
    /// JBIG (T.43)
    JbigT43 = 10,
    /// PackBits compression
    Packbits = 32773,
    /// DEFLATE (zip)
    Deflate = 32946,
    /// ZSTD compression
    Zstd = 50000,
    /// WebP compression
    WebP = 50001,
    /// JPEG XL compression
    JpegXl = 50002,
    /// LERC compression
    Lerc = 34887,
    /// LZMA compression
    Lzma = 34925,
}

impl Compression {
    /// Creates a Compression from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::None),
            2 => Some(Self::CcittRle),
            3 => Some(Self::CcittFax3),
            4 => Some(Self::CcittFax4),
            5 => Some(Self::Lzw),
            6 => Some(Self::OldJpeg),
            7 => Some(Self::Jpeg),
            8 => Some(Self::AdobeDeflate),
            9 => Some(Self::Jbig),
            10 => Some(Self::JbigT43),
            32773 => Some(Self::Packbits),
            32946 => Some(Self::Deflate),
            50000 => Some(Self::Zstd),
            50001 => Some(Self::WebP),
            50002 => Some(Self::JpegXl),
            34887 => Some(Self::Lerc),
            34925 => Some(Self::Lzma),
            _ => None,
        }
    }

    /// Returns the name of this compression scheme
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::CcittRle => "CCITT RLE",
            Self::CcittFax3 => "CCITT Fax3",
            Self::CcittFax4 => "CCITT Fax4",
            Self::Lzw => "LZW",
            Self::OldJpeg => "Old JPEG",
            Self::Jpeg => "JPEG",
            Self::AdobeDeflate => "Adobe Deflate",
            Self::Jbig => "JBIG",
            Self::JbigT43 => "JBIG T.43",
            Self::Packbits => "PackBits",
            Self::Deflate => "DEFLATE",
            Self::Zstd => "ZSTD",
            Self::WebP => "WebP",
            Self::JpegXl => "JPEG XL",
            Self::Lerc => "LERC",
            Self::Lzma => "LZMA",
        }
    }
}

/// Photometric interpretation values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PhotometricInterpretation {
    /// WhiteIsZero (for grayscale)
    WhiteIsZero = 0,
    /// BlackIsZero (for grayscale)
    BlackIsZero = 1,
    /// RGB
    Rgb = 2,
    /// Palette color
    Palette = 3,
    /// Transparency mask
    TransparencyMask = 4,
    /// CMYK
    Cmyk = 5,
    /// YCbCr
    YCbCr = 6,
    /// CIE Lab
    CieLab = 8,
    /// ICC Lab
    IccLab = 9,
    /// ITU Lab
    ItuLab = 10,
    /// Log L
    LogL = 32844,
    /// Log Luv
    LogLuv = 32845,
}

impl PhotometricInterpretation {
    /// Creates a PhotometricInterpretation from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::WhiteIsZero),
            1 => Some(Self::BlackIsZero),
            2 => Some(Self::Rgb),
            3 => Some(Self::Palette),
            4 => Some(Self::TransparencyMask),
            5 => Some(Self::Cmyk),
            6 => Some(Self::YCbCr),
            8 => Some(Self::CieLab),
            9 => Some(Self::IccLab),
            10 => Some(Self::ItuLab),
            32844 => Some(Self::LogL),
            32845 => Some(Self::LogLuv),
            _ => None,
        }
    }
}

/// Planar configuration values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PlanarConfiguration {
    /// Chunky (RGBRGBRGB...)
    Chunky = 1,
    /// Planar (RRR...GGG...BBB...)
    Planar = 2,
}

impl PlanarConfiguration {
    /// Creates a PlanarConfiguration from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Chunky),
            2 => Some(Self::Planar),
            _ => None,
        }
    }
}

/// Sample format values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum SampleFormat {
    /// Unsigned integer
    UnsignedInteger = 1,
    /// Signed integer
    SignedInteger = 2,
    /// IEEE floating point
    IeeeFloatingPoint = 3,
    /// Undefined
    Undefined = 4,
    /// Complex integer
    ComplexInteger = 5,
    /// Complex floating point
    ComplexFloatingPoint = 6,
}

impl SampleFormat {
    /// Creates a SampleFormat from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::UnsignedInteger),
            2 => Some(Self::SignedInteger),
            3 => Some(Self::IeeeFloatingPoint),
            4 => Some(Self::Undefined),
            5 => Some(Self::ComplexInteger),
            6 => Some(Self::ComplexFloatingPoint),
            _ => None,
        }
    }
}

/// Predictor values for compression
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Predictor {
    /// No predictor
    None = 1,
    /// Horizontal differencing
    HorizontalDifferencing = 2,
    /// Floating point predictor
    FloatingPoint = 3,
}

impl Predictor {
    /// Creates a Predictor from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::None),
            2 => Some(Self::HorizontalDifferencing),
            3 => Some(Self::FloatingPoint),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_roundtrip() {
        let tag = TiffTag::ImageWidth;
        let value = tag as u16;
        assert_eq!(TiffTag::from_u16(value), Some(tag));
    }

    #[test]
    fn test_compression_names() {
        assert_eq!(Compression::Lzw.name(), "LZW");
        assert_eq!(Compression::Deflate.name(), "DEFLATE");
        assert_eq!(Compression::Zstd.name(), "ZSTD");
    }

    #[test]
    fn test_geotiff_tags() {
        assert!(TiffTag::ModelPixelScale.is_geotiff_tag());
        assert!(TiffTag::GeoKeyDirectory.is_geotiff_tag());
        assert!(!TiffTag::ImageWidth.is_geotiff_tag());
    }
}
