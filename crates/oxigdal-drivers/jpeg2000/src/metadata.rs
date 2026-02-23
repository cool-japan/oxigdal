//! JP2 metadata boxes
//!
//! This module handles parsing and representation of JP2 metadata boxes.

use crate::box_reader::{BoxReader, BoxType};
use crate::error::{Jpeg2000Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Seek};

/// JP2 file type box (ftyp)
#[derive(Debug, Clone)]
pub struct FileType {
    /// Brand (e.g., "jp2 ")
    pub brand: [u8; 4],
    /// Minor version
    pub minor_version: u32,
    /// Compatibility list
    pub compatibility: Vec<[u8; 4]>,
}

impl FileType {
    /// Parse file type box
    pub fn parse<R: Read>(reader: &mut R, length: u64) -> Result<Self> {
        let mut brand = [0u8; 4];
        reader.read_exact(&mut brand)?;

        let minor_version = reader.read_u32::<BigEndian>()?;

        let mut compatibility = Vec::new();
        let remaining = (length - 8) as usize;
        let num_compat = remaining / 4;

        for _ in 0..num_compat {
            let mut compat = [0u8; 4];
            reader.read_exact(&mut compat)?;
            compatibility.push(compat);
        }

        Ok(Self {
            brand,
            minor_version,
            compatibility,
        })
    }

    /// Check if brand is JP2
    pub fn is_jp2(&self) -> bool {
        &self.brand == b"jp2 "
    }
}

/// Image header box (ihdr)
#[derive(Debug, Clone)]
pub struct ImageHeader {
    /// Image height
    pub height: u32,
    /// Image width
    pub width: u32,
    /// Number of components
    pub num_components: u16,
    /// Bits per component
    pub bits_per_component: u8,
    /// Compression type (should be 7 for JPEG2000)
    pub compression_type: u8,
    /// Colorspace unknown flag
    pub colorspace_unknown: bool,
    /// Intellectual property flag
    pub has_ipr: bool,
}

impl ImageHeader {
    /// Parse image header box
    pub fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let height = reader.read_u32::<BigEndian>()?;
        let width = reader.read_u32::<BigEndian>()?;
        let num_components = reader.read_u16::<BigEndian>()?;

        let bpc = reader.read_u8()?;
        let bits_per_component = (bpc & 0x7F) + 1;

        let compression_type = reader.read_u8()?;
        if compression_type != 7 {
            tracing::warn!(
                "Non-standard compression type: {} (expected 7)",
                compression_type
            );
        }

        let colorspace_unknown = reader.read_u8()? != 0;
        let has_ipr = reader.read_u8()? != 0;

        Ok(Self {
            height,
            width,
            num_components,
            bits_per_component,
            compression_type,
            colorspace_unknown,
            has_ipr,
        })
    }
}

/// Color specification box (colr)
#[derive(Debug, Clone)]
pub struct ColorSpecification {
    /// Method (1 = enumerated, 2 = restricted ICC profile)
    pub method: u8,
    /// Precedence
    pub precedence: i8,
    /// Approximation
    pub approximation: u8,
    /// Enumerated color space (if method == 1)
    pub enum_cs: Option<EnumeratedColorSpace>,
    /// ICC profile data (if method == 2)
    pub icc_profile: Option<Vec<u8>>,
}

/// Enumerated color spaces
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EnumeratedColorSpace {
    /// sRGB
    Srgb = 16,
    /// Grayscale
    Grayscale = 17,
    /// sYCC
    Sycc = 18,
    /// Custom
    Custom(u32),
}

impl EnumeratedColorSpace {
    /// Create from u32 value
    pub fn from_u32(value: u32) -> Self {
        match value {
            16 => Self::Srgb,
            17 => Self::Grayscale,
            18 => Self::Sycc,
            v => Self::Custom(v),
        }
    }

    /// Get u32 value
    pub fn to_u32(&self) -> u32 {
        match self {
            Self::Srgb => 16,
            Self::Grayscale => 17,
            Self::Sycc => 18,
            Self::Custom(v) => *v,
        }
    }
}

impl ColorSpecification {
    /// Parse color specification box
    pub fn parse<R: Read>(reader: &mut R, length: u64) -> Result<Self> {
        let method = reader.read_u8()?;
        let precedence = reader.read_i8()?;
        let approximation = reader.read_u8()?;

        let (enum_cs, icc_profile) = if method == 1 {
            // Enumerated color space
            let cs_value = reader.read_u32::<BigEndian>()?;
            (Some(EnumeratedColorSpace::from_u32(cs_value)), None)
        } else if method == 2 {
            // ICC profile
            let remaining = (length - 3) as usize;
            let mut profile = vec![0u8; remaining];
            reader.read_exact(&mut profile)?;
            (None, Some(profile))
        } else {
            return Err(Jpeg2000Error::InvalidMetadata(format!(
                "Invalid color specification method: {}",
                method
            )));
        };

        Ok(Self {
            method,
            precedence,
            approximation,
            enum_cs,
            icc_profile,
        })
    }
}

/// Resolution box
#[derive(Debug, Clone)]
pub struct Resolution {
    /// Vertical resolution (pixels per meter)
    pub vertical: f64,
    /// Horizontal resolution (pixels per meter)
    pub horizontal: f64,
}

impl Resolution {
    /// Parse resolution box (capture or display)
    pub fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let vr_num = reader.read_u16::<BigEndian>()?;
        let vr_den = reader.read_u16::<BigEndian>()?;
        let hr_num = reader.read_u16::<BigEndian>()?;
        let hr_den = reader.read_u16::<BigEndian>()?;

        let vr_exp = reader.read_i8()?;
        let hr_exp = reader.read_i8()?;

        let vertical = f64::from(vr_num) / f64::from(vr_den) * 10f64.powi(i32::from(vr_exp));
        let horizontal = f64::from(hr_num) / f64::from(hr_den) * 10f64.powi(i32::from(hr_exp));

        Ok(Self {
            vertical,
            horizontal,
        })
    }

    /// Convert to DPI (assuming 1 meter = 39.3701 inches)
    pub fn to_dpi(&self) -> (f64, f64) {
        const INCH_PER_METER: f64 = 39.3701;
        (
            self.horizontal / INCH_PER_METER,
            self.vertical / INCH_PER_METER,
        )
    }
}

/// XML metadata box
#[derive(Debug, Clone)]
pub struct XmlMetadata {
    /// XML content
    pub content: String,
}

impl XmlMetadata {
    /// Parse XML box
    pub fn parse<R: Read>(reader: &mut R, length: u64) -> Result<Self> {
        let mut buffer = vec![0u8; length as usize];
        reader.read_exact(&mut buffer)?;

        let content = String::from_utf8(buffer).map_err(|e| {
            Jpeg2000Error::InvalidMetadata(format!("Invalid UTF-8 in XML box: {}", e))
        })?;

        Ok(Self { content })
    }
}

/// UUID box
#[derive(Debug, Clone)]
pub struct UuidBox {
    /// UUID
    pub uuid: [u8; 16],
    /// Data
    pub data: Vec<u8>,
}

impl UuidBox {
    /// Parse UUID box
    pub fn parse<R: Read>(reader: &mut R, length: u64) -> Result<Self> {
        let mut uuid = [0u8; 16];
        reader.read_exact(&mut uuid)?;

        let data_len = (length - 16) as usize;
        let mut data = vec![0u8; data_len];
        reader.read_exact(&mut data)?;

        Ok(Self { uuid, data })
    }

    /// Get UUID as string
    pub fn uuid_string(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.uuid[0],
            self.uuid[1],
            self.uuid[2],
            self.uuid[3],
            self.uuid[4],
            self.uuid[5],
            self.uuid[6],
            self.uuid[7],
            self.uuid[8],
            self.uuid[9],
            self.uuid[10],
            self.uuid[11],
            self.uuid[12],
            self.uuid[13],
            self.uuid[14],
            self.uuid[15]
        )
    }
}

/// JP2 metadata collection
#[derive(Debug, Clone, Default)]
pub struct Jp2Metadata {
    /// File type
    pub file_type: Option<FileType>,
    /// Image header
    pub image_header: Option<ImageHeader>,
    /// Color specification
    pub color_spec: Option<ColorSpecification>,
    /// Capture resolution
    pub capture_resolution: Option<Resolution>,
    /// Display resolution
    pub display_resolution: Option<Resolution>,
    /// XML metadata boxes
    pub xml_boxes: Vec<XmlMetadata>,
    /// UUID boxes
    pub uuid_boxes: Vec<UuidBox>,
}

impl Jp2Metadata {
    /// Create new empty metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse metadata from JP2 file
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let mut box_reader = BoxReader::new(reader)?;
        let mut metadata = Self::new();

        // Parse file type box
        if let Some(ftyp_header) = box_reader.find_box(BoxType::FileType)? {
            let data = box_reader.read_box_data(&ftyp_header)?;
            let mut cursor = std::io::Cursor::new(&data);
            metadata.file_type = Some(FileType::parse(&mut cursor, ftyp_header.data_size())?);
        }

        // Reset and find JP2 header box
        box_reader.reset()?;
        if let Some(jp2h_header) = box_reader.find_box(BoxType::Jp2Header)? {
            // JP2 header is a superbox containing other boxes
            let data = box_reader.read_box_data(&jp2h_header)?;
            let mut cursor = std::io::Cursor::new(&data);

            // Parse ihdr
            let mut sub_reader = BoxReader::new(&mut cursor)?;
            if let Some(ihdr_header) = sub_reader.find_box(BoxType::ImageHeader)? {
                let ihdr_data = sub_reader.read_box_data(&ihdr_header)?;
                let mut ihdr_cursor = std::io::Cursor::new(&ihdr_data);
                metadata.image_header = Some(ImageHeader::parse(&mut ihdr_cursor)?);
            }

            // Parse colr
            sub_reader.reset()?;
            if let Some(colr_header) = sub_reader.find_box(BoxType::ColorSpecification)? {
                let colr_data = sub_reader.read_box_data(&colr_header)?;
                let mut colr_cursor = std::io::Cursor::new(&colr_data);
                metadata.color_spec = Some(ColorSpecification::parse(
                    &mut colr_cursor,
                    colr_header.data_size(),
                )?);
            }
        }

        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enumerated_colorspace() {
        let cs = EnumeratedColorSpace::from_u32(16);
        assert_eq!(cs, EnumeratedColorSpace::Srgb);
        assert_eq!(cs.to_u32(), 16);
    }

    #[test]
    fn test_resolution_to_dpi() {
        let res = Resolution {
            horizontal: 11811.0, // ~300 DPI
            vertical: 11811.0,
        };

        let (h_dpi, v_dpi) = res.to_dpi();
        assert!((h_dpi - 300.0).abs() < 1.0);
        assert!((v_dpi - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_uuid_string() {
        let uuid_box = UuidBox {
            uuid: [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ],
            data: Vec::new(),
        };

        let uuid_str = uuid_box.uuid_string();
        assert_eq!(uuid_str, "00112233-4455-6677-8899-aabbccddeeff");
    }
}
