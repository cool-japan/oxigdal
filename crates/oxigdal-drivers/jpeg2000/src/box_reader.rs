//! JP2 box structure parsing
//!
//! JP2 format is based on the ISO Base Media File Format (similar to JPEG 2000 Part 1).
//! It consists of a series of boxes (similar to atoms in QuickTime).

use crate::error::{Jpeg2000Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

/// JP2 file signature (first 12 bytes)
pub const JP2_SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, // Box length (12 bytes)
    0x6A, 0x50, 0x20, 0x20, // 'jP  ' box type
    0x0D, 0x0A, 0x87, 0x0A, // JP2 signature
];

/// JP2 box types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    /// JPEG2000 signature box
    Signature,
    /// File type box
    FileType,
    /// JP2 header box
    Jp2Header,
    /// Image header box
    ImageHeader,
    /// Bits per component box
    BitsPerComponent,
    /// Color specification box
    ColorSpecification,
    /// Palette box
    Palette,
    /// Component mapping box
    ComponentMapping,
    /// Channel definition box
    ChannelDefinition,
    /// Resolution box
    Resolution,
    /// Capture resolution box
    CaptureResolution,
    /// Display resolution box
    DisplayResolution,
    /// Contiguous codestream box
    ContiguousCodestream,
    /// XML box
    Xml,
    /// UUID box
    Uuid,
    /// UUID info box
    UuidInfo,
    /// UUID list box
    UuidList,
    /// URL box
    Url,
    /// Intellectual property rights box
    IntellectualProperty,
    /// Unknown/unsupported box type
    Unknown([u8; 4]),
}

impl BoxType {
    /// Create BoxType from 4-byte identifier
    pub fn from_bytes(bytes: &[u8; 4]) -> Self {
        match bytes {
            b"jP  " => Self::Signature,
            b"ftyp" => Self::FileType,
            b"jp2h" => Self::Jp2Header,
            b"ihdr" => Self::ImageHeader,
            b"bpcc" => Self::BitsPerComponent,
            b"colr" => Self::ColorSpecification,
            b"pclr" => Self::Palette,
            b"cmap" => Self::ComponentMapping,
            b"cdef" => Self::ChannelDefinition,
            b"res " => Self::Resolution,
            b"resc" => Self::CaptureResolution,
            b"resd" => Self::DisplayResolution,
            b"jp2c" => Self::ContiguousCodestream,
            b"xml " => Self::Xml,
            b"uuid" => Self::Uuid,
            b"uinf" => Self::UuidInfo,
            b"ulst" => Self::UuidList,
            b"url " => Self::Url,
            b"iprp" => Self::IntellectualProperty,
            _ => Self::Unknown(*bytes),
        }
    }

    /// Convert BoxType to 4-byte identifier
    pub fn to_bytes(&self) -> [u8; 4] {
        match self {
            Self::Signature => *b"jP  ",
            Self::FileType => *b"ftyp",
            Self::Jp2Header => *b"jp2h",
            Self::ImageHeader => *b"ihdr",
            Self::BitsPerComponent => *b"bpcc",
            Self::ColorSpecification => *b"colr",
            Self::Palette => *b"pclr",
            Self::ComponentMapping => *b"cmap",
            Self::ChannelDefinition => *b"cdef",
            Self::Resolution => *b"res ",
            Self::CaptureResolution => *b"resc",
            Self::DisplayResolution => *b"resd",
            Self::ContiguousCodestream => *b"jp2c",
            Self::Xml => *b"xml ",
            Self::Uuid => *b"uuid",
            Self::UuidInfo => *b"uinf",
            Self::UuidList => *b"ulst",
            Self::Url => *b"url ",
            Self::IntellectualProperty => *b"iprp",
            Self::Unknown(bytes) => *bytes,
        }
    }

    /// Get box type as string
    pub fn as_str(&self) -> String {
        let bytes = self.to_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }
}

/// JP2 box header
#[derive(Debug, Clone)]
pub struct BoxHeader {
    /// Box type
    pub box_type: BoxType,
    /// Box length (including header)
    pub length: u64,
    /// Offset of box content (after header)
    pub data_offset: u64,
}

impl BoxHeader {
    /// Size of standard box header (8 bytes)
    pub const STANDARD_HEADER_SIZE: u64 = 8;
    /// Size of extended box header (16 bytes)
    pub const EXTENDED_HEADER_SIZE: u64 = 16;

    /// Read box header from reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let length_u32 = reader.read_u32::<BigEndian>()?;
        let mut type_bytes = [0u8; 4];
        reader.read_exact(&mut type_bytes)?;
        let box_type = BoxType::from_bytes(&type_bytes);

        let (length, data_offset) = if length_u32 == 1 {
            // Extended length (64-bit)
            let length = reader.read_u64::<BigEndian>()?;
            (length, Self::EXTENDED_HEADER_SIZE)
        } else {
            // Standard length (32-bit)
            (u64::from(length_u32), Self::STANDARD_HEADER_SIZE)
        };

        Ok(Self {
            box_type,
            length,
            data_offset,
        })
    }

    /// Get the size of box data (excluding header)
    pub fn data_size(&self) -> u64 {
        self.length.saturating_sub(self.data_offset)
    }
}

/// JP2 box reader
pub struct BoxReader<R> {
    reader: R,
    position: u64,
}

impl<R: Read + Seek> BoxReader<R> {
    /// Create new box reader
    pub fn new(mut reader: R) -> Result<Self> {
        // Verify JP2 signature
        let mut sig = [0u8; 12];
        reader.read_exact(&mut sig)?;

        if sig != JP2_SIGNATURE {
            return Err(Jpeg2000Error::InvalidSignature);
        }

        Ok(Self {
            reader,
            position: 12,
        })
    }

    /// Read next box header
    pub fn next_box(&mut self) -> Result<Option<BoxHeader>> {
        match BoxHeader::read(&mut self.reader) {
            Ok(header) => {
                self.position += header.data_offset;
                Ok(Some(header))
            }
            Err(Jpeg2000Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Read box data
    pub fn read_box_data(&mut self, header: &BoxHeader) -> Result<Vec<u8>> {
        let data_size = header.data_size();

        if data_size > usize::MAX as u64 {
            return Err(Jpeg2000Error::AllocationError(format!(
                "Box data size too large: {}",
                data_size
            )));
        }

        let mut data = vec![0u8; data_size as usize];
        self.reader.read_exact(&mut data)?;
        self.position += data_size;

        Ok(data)
    }

    /// Skip box data
    pub fn skip_box(&mut self, header: &BoxHeader) -> Result<()> {
        let data_size = header.data_size();
        self.reader.seek(SeekFrom::Current(data_size as i64))?;
        self.position += data_size;
        Ok(())
    }

    /// Find box by type
    pub fn find_box(&mut self, target_type: BoxType) -> Result<Option<BoxHeader>> {
        loop {
            match self.next_box()? {
                Some(header) => {
                    if header.box_type == target_type {
                        return Ok(Some(header));
                    }
                    self.skip_box(&header)?;
                }
                None => return Ok(None),
            }
        }
    }

    /// Reset reader to beginning (after signature)
    pub fn reset(&mut self) -> Result<()> {
        self.reader.seek(SeekFrom::Start(12))?;
        self.position = 12;
        Ok(())
    }

    /// Get current position
    pub fn position(&self) -> u64 {
        self.position
    }
}

// ============================================================================
// JP2 Box Writing Support
// ============================================================================

/// JP2 box writer for creating JP2 files
pub struct BoxWriter<W> {
    writer: W,
    position: u64,
}

impl<W: Write + Seek> BoxWriter<W> {
    /// Create a new box writer
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            position: 0,
        }
    }

    /// Write the JP2 signature box
    pub fn write_signature(&mut self) -> Result<()> {
        self.writer.write_all(&JP2_SIGNATURE)?;
        self.position += 12;
        Ok(())
    }

    /// Write a file type box (ftyp)
    ///
    /// Creates a standard JP2 ftyp box with brand "jp2 " and
    /// compatibility list including "jp2 ".
    pub fn write_file_type(&mut self) -> Result<()> {
        // ftyp box: brand (4) + minor_version (4) + compatibility (4)
        let data_size: u32 = 12;
        let box_length = 8 + data_size; // header + data

        // Write box header
        self.write_u32_be(box_length)?;
        self.writer.write_all(b"ftyp")?;

        // Brand: "jp2 "
        self.writer.write_all(b"jp2 ")?;
        // Minor version: 0
        self.write_u32_be(0)?;
        // Compatibility list: "jp2 "
        self.writer.write_all(b"jp2 ")?;

        self.position += box_length as u64;
        Ok(())
    }

    /// Write a JP2 header superbox (jp2h) containing ihdr and colr
    pub fn write_jp2_header(
        &mut self,
        width: u32,
        height: u32,
        num_components: u16,
        bits_per_component: u8,
        color_space: u32,
    ) -> Result<()> {
        // Calculate inner box sizes
        let ihdr_data_size = 14u32; // 4+4+2+1+1+1+1
        let ihdr_box_size = 8 + ihdr_data_size;

        let colr_data_size = 7u32; // method(1) + precedence(1) + approximation(1) + enum_cs(4)
        let colr_box_size = 8 + colr_data_size;

        // jp2h superbox needs its own header wrapping ihdr + colr + jp2h signature
        // But jp2h does NOT have the JP2 signature inside - the sub-boxes start immediately
        let jp2h_data_size = ihdr_box_size + colr_box_size;
        let jp2h_box_size = 8 + jp2h_data_size;

        // Write jp2h box header
        self.write_u32_be(jp2h_box_size)?;
        self.writer.write_all(b"jp2h")?;

        // Write ihdr box
        self.write_u32_be(ihdr_box_size)?;
        self.writer.write_all(b"ihdr")?;
        self.write_u32_be(height)?;
        self.write_u32_be(width)?;
        self.write_u16_be(num_components)?;
        // BPC: bits_per_component - 1 (unsigned)
        self.writer
            .write_all(&[bits_per_component.saturating_sub(1)])?;
        // Compression type: 7 (JPEG2000)
        self.writer.write_all(&[7])?;
        // Colorspace unknown: 0
        self.writer.write_all(&[0])?;
        // IPR: 0 (no intellectual property)
        self.writer.write_all(&[0])?;

        // Write colr box
        self.write_u32_be(colr_box_size)?;
        self.writer.write_all(b"colr")?;
        // Method: 1 (enumerated)
        self.writer.write_all(&[1])?;
        // Precedence: 0
        self.writer.write_all(&[0])?;
        // Approximation: 0
        self.writer.write_all(&[0])?;
        // Enumerated color space
        self.write_u32_be(color_space)?;

        self.position += jp2h_box_size as u64;
        Ok(())
    }

    /// Write a contiguous codestream box (jp2c) wrapping raw codestream data
    pub fn write_codestream_box(&mut self, codestream: &[u8]) -> Result<()> {
        let data_len = codestream.len() as u64;
        if data_len > u32::MAX as u64 - 8 {
            // Use extended length
            let box_length: u64 = 16 + data_len; // extended header + data
            self.write_u32_be(1)?; // extended length marker
            self.writer.write_all(b"jp2c")?;
            self.write_u64_be(box_length)?;
        } else {
            let box_length = 8 + data_len as u32;
            self.write_u32_be(box_length)?;
            self.writer.write_all(b"jp2c")?;
        }

        self.writer.write_all(codestream)?;
        self.position += data_len;
        Ok(())
    }

    /// Write an XML metadata box
    pub fn write_xml_box(&mut self, xml_content: &str) -> Result<()> {
        let data = xml_content.as_bytes();
        let box_length = 8 + data.len() as u32;
        self.write_u32_be(box_length)?;
        self.writer.write_all(b"xml ")?;
        self.writer.write_all(data)?;
        self.position += box_length as u64;
        Ok(())
    }

    /// Write a UUID box
    pub fn write_uuid_box(&mut self, uuid: &[u8; 16], data: &[u8]) -> Result<()> {
        let box_length = 8 + 16 + data.len() as u32;
        self.write_u32_be(box_length)?;
        self.writer.write_all(b"uuid")?;
        self.writer.write_all(uuid)?;
        self.writer.write_all(data)?;
        self.position += box_length as u64;
        Ok(())
    }

    /// Flush the writer
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    /// Get current position
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Helper to write u32 in big endian
    fn write_u32_be(&mut self, value: u32) -> Result<()> {
        use byteorder::WriteBytesExt;
        self.writer.write_u32::<BigEndian>(value)?;
        Ok(())
    }

    /// Helper to write u16 in big endian
    fn write_u16_be(&mut self, value: u16) -> Result<()> {
        use byteorder::WriteBytesExt;
        self.writer.write_u16::<BigEndian>(value)?;
        Ok(())
    }

    /// Helper to write u64 in big endian
    fn write_u64_be(&mut self, value: u64) -> Result<()> {
        use byteorder::WriteBytesExt;
        self.writer.write_u64::<BigEndian>(value)?;
        Ok(())
    }
}

// ============================================================================
// Region of Interest (ROI) Support
// ============================================================================

/// ROI (Region of Interest) definition
///
/// Defines a region within an image that should receive higher quality
/// encoding. The ROI is encoded with a shift value that scales up
/// the coefficients in the ROI region, ensuring they survive quantization
/// better than background regions.
#[derive(Debug, Clone)]
pub struct RoiRegion {
    /// Top-left X coordinate
    pub x: u32,
    /// Top-left Y coordinate
    pub y: u32,
    /// Width of the ROI
    pub width: u32,
    /// Height of the ROI
    pub height: u32,
    /// Shift value (higher = more quality for ROI, typically 0-31)
    pub shift: u8,
    /// Component index (None = all components)
    pub component: Option<u16>,
}

impl RoiRegion {
    /// Create a new ROI region
    pub fn new(x: u32, y: u32, width: u32, height: u32, shift: u8) -> Self {
        Self {
            x,
            y,
            width,
            height,
            shift,
            component: None,
        }
    }

    /// Create a new ROI region for a specific component
    pub fn for_component(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        shift: u8,
        component: u16,
    ) -> Self {
        Self {
            x,
            y,
            width,
            height,
            shift,
            component: Some(component),
        }
    }

    /// Check if a pixel coordinate falls within this ROI
    pub fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// ROI encoder that applies MaxShift method
///
/// The MaxShift method works by scaling up wavelet coefficients in the ROI
/// region by `2^shift`, ensuring that ROI coefficients are always more
/// significant than background coefficients. During decoding, the decoder
/// can identify ROI coefficients by their magnitude and shift them back down.
#[derive(Debug)]
pub struct RoiEncoder {
    /// ROI regions
    regions: Vec<RoiRegion>,
    /// Image width
    image_width: u32,
    /// Image height
    image_height: u32,
}

impl RoiEncoder {
    /// Create a new ROI encoder
    pub fn new(image_width: u32, image_height: u32) -> Self {
        Self {
            regions: Vec::new(),
            image_width,
            image_height,
        }
    }

    /// Add an ROI region
    pub fn add_region(&mut self, region: RoiRegion) -> Result<()> {
        if region.x + region.width > self.image_width
            || region.y + region.height > self.image_height
        {
            return Err(Jpeg2000Error::InvalidDimension(format!(
                "ROI region ({},{})+({}x{}) exceeds image bounds ({}x{})",
                region.x,
                region.y,
                region.width,
                region.height,
                self.image_width,
                self.image_height
            )));
        }
        if region.shift > 31 {
            return Err(Jpeg2000Error::Other(
                "ROI shift value must be 0-31".to_string(),
            ));
        }
        self.regions.push(region);
        Ok(())
    }

    /// Generate an ROI mask for the given component
    ///
    /// Returns a boolean mask where `true` indicates the pixel belongs
    /// to at least one ROI region.
    pub fn generate_mask(&self, component: u16) -> Vec<bool> {
        let total = self.image_width as usize * self.image_height as usize;
        let mut mask = vec![false; total];

        for region in &self.regions {
            // Skip if this region is for a different component
            if let Some(comp) = region.component {
                if comp != component {
                    continue;
                }
            }

            for py in region.y..(region.y + region.height) {
                for px in region.x..(region.x + region.width) {
                    if py < self.image_height && px < self.image_width {
                        let idx = py as usize * self.image_width as usize + px as usize;
                        if idx < mask.len() {
                            mask[idx] = true;
                        }
                    }
                }
            }
        }

        mask
    }

    /// Apply ROI MaxShift scaling to wavelet coefficients (integer version)
    ///
    /// Coefficients within the ROI are scaled up by `2^shift` to make them
    /// more significant than background coefficients. The maximum shift value
    /// across all applicable regions is used for each coefficient.
    pub fn apply_maxshift_i32(
        &self,
        coefficients: &mut [i32],
        width: usize,
        height: usize,
        component: u16,
    ) -> Result<()> {
        if coefficients.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Coefficient buffer size mismatch: expected {}, got {}",
                width * height,
                coefficients.len()
            )));
        }

        for region in &self.regions {
            // Skip if this region is for a different component
            if let Some(comp) = region.component {
                if comp != component {
                    continue;
                }
            }

            let shift = region.shift as u32;

            for py in region.y..(region.y + region.height) {
                for px in region.x..(region.x + region.width) {
                    let ux = px as usize;
                    let uy = py as usize;
                    if ux < width && uy < height {
                        let idx = uy * width + ux;
                        if idx < coefficients.len() {
                            coefficients[idx] = coefficients[idx]
                                .saturating_mul(1i32.checked_shl(shift).unwrap_or(i32::MAX));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Apply ROI MaxShift scaling to wavelet coefficients (float version)
    pub fn apply_maxshift_f32(
        &self,
        coefficients: &mut [f32],
        width: usize,
        height: usize,
        component: u16,
    ) -> Result<()> {
        if coefficients.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Coefficient buffer size mismatch: expected {}, got {}",
                width * height,
                coefficients.len()
            )));
        }

        for region in &self.regions {
            if let Some(comp) = region.component {
                if comp != component {
                    continue;
                }
            }

            let scale = (1u32 << region.shift) as f32;

            for py in region.y..(region.y + region.height) {
                for px in region.x..(region.x + region.width) {
                    let ux = px as usize;
                    let uy = py as usize;
                    if ux < width && uy < height {
                        let idx = uy * width + ux;
                        if idx < coefficients.len() {
                            coefficients[idx] *= scale;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Remove ROI MaxShift scaling (for decoding)
    pub fn remove_maxshift_i32(
        &self,
        coefficients: &mut [i32],
        width: usize,
        height: usize,
        component: u16,
    ) -> Result<()> {
        if coefficients.len() != width * height {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Coefficient buffer size mismatch: expected {}, got {}",
                width * height,
                coefficients.len()
            )));
        }

        for region in &self.regions {
            if let Some(comp) = region.component {
                if comp != component {
                    continue;
                }
            }

            let shift = region.shift as u32;

            for py in region.y..(region.y + region.height) {
                for px in region.x..(region.x + region.width) {
                    let ux = px as usize;
                    let uy = py as usize;
                    if ux < width && uy < height {
                        let idx = uy * width + ux;
                        if idx < coefficients.len() {
                            coefficients[idx] >>= shift;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the maximum shift value across all regions
    pub fn max_shift(&self) -> u8 {
        self.regions.iter().map(|r| r.shift).max().unwrap_or(0)
    }

    /// Get the number of ROI regions
    pub fn num_regions(&self) -> usize {
        self.regions.len()
    }

    /// Serialize ROI info to RGN marker segment data for the codestream
    ///
    /// Returns pairs of (component_index, rgn_marker_data) for embedding
    /// in the codestream.
    pub fn to_rgn_markers(&self) -> Vec<(u16, Vec<u8>)> {
        let mut markers = Vec::new();

        for region in &self.regions {
            let component = region.component.unwrap_or(0);
            // RGN marker data: Srgn (1 byte ROI style) + SPrgn (1 byte shift)
            // ROI style 0 = implicit (MaxShift method)
            let data = vec![0u8, region.shift];
            markers.push((component, data));
        }

        markers
    }
}

use std::io::Write;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_box_type_conversion() {
        let type_bytes = b"jp2h";
        let box_type = BoxType::from_bytes(type_bytes);
        assert_eq!(box_type, BoxType::Jp2Header);
        assert_eq!(box_type.to_bytes(), *type_bytes);
    }

    #[test]
    fn test_signature_validation() {
        let data = JP2_SIGNATURE.to_vec();
        let cursor = Cursor::new(data);
        let result = BoxReader::new(cursor);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_signature() {
        let data = vec![0u8; 12];
        let cursor = Cursor::new(data);
        let result = BoxReader::new(cursor);
        assert!(result.is_err());
    }

    // ========================================================================
    // JP2 Box Writing Tests
    // ========================================================================

    #[test]
    fn test_write_signature() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = BoxWriter::new(&mut buffer);
        writer.write_signature().expect("write signature failed");

        let data = buffer.into_inner();
        assert_eq!(&data, &JP2_SIGNATURE);
    }

    #[test]
    fn test_write_file_type() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = BoxWriter::new(&mut buffer);
        writer.write_file_type().expect("write ftyp failed");

        let data = buffer.into_inner();
        // Box length (4) + type (4) + brand (4) + minor (4) + compat (4) = 20
        assert_eq!(data.len(), 20);
        assert_eq!(&data[4..8], b"ftyp");
        assert_eq!(&data[8..12], b"jp2 ");
    }

    #[test]
    fn test_write_jp2_header() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = BoxWriter::new(&mut buffer);
        writer
            .write_jp2_header(256, 256, 3, 8, 16) // sRGB
            .expect("write jp2h failed");

        let data = buffer.into_inner();
        // jp2h header (8) + ihdr box (22) + colr box (15) = 45
        assert_eq!(data.len(), 45);
        assert_eq!(&data[4..8], b"jp2h");
        assert_eq!(&data[12..16], b"ihdr");
    }

    #[test]
    fn test_write_complete_jp2() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = BoxWriter::new(&mut buffer);

        writer.write_signature().expect("sig");
        writer.write_file_type().expect("ftyp");
        writer.write_jp2_header(64, 64, 3, 8, 16).expect("jp2h");

        // Minimal codestream: SOC + EOC
        let codestream = vec![0xFF, 0x4F, 0xFF, 0xD9];
        writer.write_codestream_box(&codestream).expect("jp2c");

        let data = buffer.into_inner();
        assert!(data.len() > 12);
        // Verify we can read back the signature
        assert_eq!(&data[4..8], b"jP  ");
    }

    #[test]
    fn test_write_xml_box() {
        let mut buffer = Cursor::new(Vec::new());
        let mut writer = BoxWriter::new(&mut buffer);
        writer
            .write_xml_box("<metadata>test</metadata>")
            .expect("xml failed");

        let data = buffer.into_inner();
        assert_eq!(&data[4..8], b"xml ");
    }

    // ========================================================================
    // ROI Encoding Tests
    // ========================================================================

    #[test]
    fn test_roi_region_creation() {
        let roi = RoiRegion::new(10, 20, 50, 30, 8);
        assert_eq!(roi.x, 10);
        assert_eq!(roi.y, 20);
        assert_eq!(roi.width, 50);
        assert_eq!(roi.height, 30);
        assert_eq!(roi.shift, 8);
        assert!(roi.component.is_none());
    }

    #[test]
    fn test_roi_region_contains() {
        let roi = RoiRegion::new(10, 10, 20, 20, 5);
        assert!(roi.contains(15, 15));
        assert!(roi.contains(10, 10));
        assert!(roi.contains(29, 29));
        assert!(!roi.contains(30, 15));
        assert!(!roi.contains(5, 5));
    }

    #[test]
    fn test_roi_encoder_creation() {
        let encoder = RoiEncoder::new(256, 256);
        assert_eq!(encoder.num_regions(), 0);
        assert_eq!(encoder.max_shift(), 0);
    }

    #[test]
    fn test_roi_add_region() {
        let mut encoder = RoiEncoder::new(256, 256);
        let result = encoder.add_region(RoiRegion::new(10, 10, 50, 50, 8));
        assert!(result.is_ok());
        assert_eq!(encoder.num_regions(), 1);
        assert_eq!(encoder.max_shift(), 8);
    }

    #[test]
    fn test_roi_invalid_region() {
        let mut encoder = RoiEncoder::new(256, 256);
        let result = encoder.add_region(RoiRegion::new(200, 200, 100, 100, 5));
        assert!(result.is_err()); // Exceeds bounds
    }

    #[test]
    fn test_roi_invalid_shift() {
        let mut encoder = RoiEncoder::new(256, 256);
        let result = encoder.add_region(RoiRegion::new(0, 0, 10, 10, 32));
        assert!(result.is_err()); // Shift > 31
    }

    #[test]
    fn test_roi_generate_mask() {
        let mut encoder = RoiEncoder::new(4, 4);
        encoder
            .add_region(RoiRegion::new(1, 1, 2, 2, 5))
            .expect("add region");

        let mask = encoder.generate_mask(0);
        assert_eq!(mask.len(), 16);

        // Check specific pixels
        assert!(!mask[0]); // (0,0)
        assert!(!mask[1]); // (1,0)
        assert!(mask[5]); // (1,1)
        assert!(mask[6]); // (2,1)
        assert!(mask[9]); // (1,2)
        assert!(mask[10]); // (2,2)
        assert!(!mask[15]); // (3,3)
    }

    #[test]
    fn test_roi_maxshift_i32() {
        let mut encoder = RoiEncoder::new(4, 4);
        encoder
            .add_region(RoiRegion::new(0, 0, 2, 2, 3))
            .expect("add region");

        let mut coeffs = vec![1i32; 16];
        encoder
            .apply_maxshift_i32(&mut coeffs, 4, 4, 0)
            .expect("apply maxshift");

        // ROI pixels should be scaled by 2^3 = 8
        assert_eq!(coeffs[0], 8); // (0,0) in ROI
        assert_eq!(coeffs[1], 8); // (1,0) in ROI
        assert_eq!(coeffs[4], 8); // (0,1) in ROI
        assert_eq!(coeffs[5], 8); // (1,1) in ROI

        // Non-ROI pixels should be unchanged
        assert_eq!(coeffs[2], 1); // (2,0) not in ROI
        assert_eq!(coeffs[15], 1); // (3,3) not in ROI
    }

    #[test]
    fn test_roi_maxshift_round_trip_i32() {
        let mut encoder = RoiEncoder::new(4, 4);
        encoder
            .add_region(RoiRegion::new(0, 0, 2, 2, 4))
            .expect("add region");

        let mut coeffs = vec![
            10i32, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
        ];
        let original_roi_values: Vec<i32> = vec![coeffs[0], coeffs[1], coeffs[4], coeffs[5]];

        encoder
            .apply_maxshift_i32(&mut coeffs, 4, 4, 0)
            .expect("apply");

        encoder
            .remove_maxshift_i32(&mut coeffs, 4, 4, 0)
            .expect("remove");

        // ROI values should be restored
        assert_eq!(coeffs[0], original_roi_values[0]);
        assert_eq!(coeffs[1], original_roi_values[1]);
        assert_eq!(coeffs[4], original_roi_values[2]);
        assert_eq!(coeffs[5], original_roi_values[3]);
    }

    #[test]
    fn test_roi_maxshift_f32() {
        let mut encoder = RoiEncoder::new(4, 4);
        encoder
            .add_region(RoiRegion::new(0, 0, 2, 2, 2))
            .expect("add region");

        let mut coeffs = vec![1.0f32; 16];
        encoder
            .apply_maxshift_f32(&mut coeffs, 4, 4, 0)
            .expect("apply");

        // ROI pixels should be scaled by 2^2 = 4.0
        assert!((coeffs[0] - 4.0).abs() < f32::EPSILON);
        assert!((coeffs[2] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_roi_component_specific() {
        let mut encoder = RoiEncoder::new(4, 4);
        encoder
            .add_region(RoiRegion::for_component(0, 0, 2, 2, 3, 1))
            .expect("add region");

        // Component 0: should not be affected
        let mask_c0 = encoder.generate_mask(0);
        assert!(mask_c0.iter().all(|&m| !m));

        // Component 1: should have the ROI
        let mask_c1 = encoder.generate_mask(1);
        assert!(mask_c1[0]); // (0,0) in ROI for component 1
    }

    #[test]
    fn test_roi_to_rgn_markers() {
        let mut encoder = RoiEncoder::new(256, 256);
        encoder
            .add_region(RoiRegion::new(10, 10, 50, 50, 8))
            .expect("add region");

        let markers = encoder.to_rgn_markers();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].0, 0); // default component
        assert_eq!(markers[0].1, vec![0, 8]); // style=0, shift=8
    }
}
