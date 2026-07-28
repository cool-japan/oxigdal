//! JPEG2000 JP2 box structure parser.

use crate::error::{Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

/// JP2 box type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxType {
    /// JPEG2000 Signature box
    Signature,
    /// File Type box
    FileType,
    /// JP2 Header box
    Jp2Header,
    /// Image Header box
    ImageHeader,
    /// Color Specification box
    ColorSpec,
    /// Contiguous Codestream box
    CodeStream,
    /// XML box
    Xml,
    /// UUID box
    Uuid,
    /// UUID Info box
    UuidInfo,
    /// UUID List box
    UuidList,
    /// URL box
    Url,
    /// Palette box
    Palette,
    /// Component Mapping box
    ComponentMapping,
    /// Channel Definition box
    ChannelDefinition,
    /// Resolution box
    Resolution,
    /// Capture Resolution box
    CaptureResolution,
    /// Display Resolution box
    DisplayResolution,
    /// Bits Per Component box
    BitsPerComponent,
    /// JPX (Part-2) Codestream Header box (`jpch`)
    JpxCodestreamHeader,
    /// JPX (Part-2) Compositing Layer Header box (`jplh`)
    JpxCompositingLayerHeader,
    /// JPX (Part-2) Association box (`asoc`)
    JpxAssociation,
    /// JPX (Part-2) Number List box (`nlst`)
    JpxNumberList,
    /// JPX (Part-2) Opacity box (`opct`)
    JpxOpacity,
    /// JPX (Part-2) Reader Requirements box (`creq`)
    JpxReaderRequirements,
    /// Unknown box type
    Unknown(u32),
}

impl BoxType {
    /// Create box type from 4-byte identifier.
    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        match &bytes {
            b"jP  " => Self::Signature,
            b"ftyp" => Self::FileType,
            b"jp2h" => Self::Jp2Header,
            b"ihdr" => Self::ImageHeader,
            b"colr" => Self::ColorSpec,
            b"jp2c" => Self::CodeStream,
            b"xml " => Self::Xml,
            b"uuid" => Self::Uuid,
            b"uinf" => Self::UuidInfo,
            b"ulst" => Self::UuidList,
            b"url " => Self::Url,
            b"pclr" => Self::Palette,
            b"cmap" => Self::ComponentMapping,
            b"cdef" => Self::ChannelDefinition,
            b"res " => Self::Resolution,
            b"resc" => Self::CaptureResolution,
            b"resd" => Self::DisplayResolution,
            b"bpcc" => Self::BitsPerComponent,
            b"jpch" => Self::JpxCodestreamHeader,
            b"jplh" => Self::JpxCompositingLayerHeader,
            b"asoc" => Self::JpxAssociation,
            b"nlst" => Self::JpxNumberList,
            b"opct" => Self::JpxOpacity,
            b"creq" => Self::JpxReaderRequirements,
            _ => Self::Unknown(u32::from_be_bytes(bytes)),
        }
    }

    /// Get box type as 4-byte identifier.
    pub fn to_bytes(&self) -> [u8; 4] {
        match self {
            Self::Signature => *b"jP  ",
            Self::FileType => *b"ftyp",
            Self::Jp2Header => *b"jp2h",
            Self::ImageHeader => *b"ihdr",
            Self::ColorSpec => *b"colr",
            Self::CodeStream => *b"jp2c",
            Self::Xml => *b"xml ",
            Self::Uuid => *b"uuid",
            Self::UuidInfo => *b"uinf",
            Self::UuidList => *b"ulst",
            Self::Url => *b"url ",
            Self::Palette => *b"pclr",
            Self::ComponentMapping => *b"cmap",
            Self::ChannelDefinition => *b"cdef",
            Self::Resolution => *b"res ",
            Self::CaptureResolution => *b"resc",
            Self::DisplayResolution => *b"resd",
            Self::BitsPerComponent => *b"bpcc",
            Self::JpxCodestreamHeader => *b"jpch",
            Self::JpxCompositingLayerHeader => *b"jplh",
            Self::JpxAssociation => *b"asoc",
            Self::JpxNumberList => *b"nlst",
            Self::JpxOpacity => *b"opct",
            Self::JpxReaderRequirements => *b"creq",
            Self::Unknown(val) => val.to_be_bytes(),
        }
    }

    /// Whether this box type is a JPEG2000 Part-2 (JPX) only structure that the
    /// Part-1 decode path cannot interpret.
    pub fn is_jpx_part2(&self) -> bool {
        matches!(
            self,
            Self::JpxCodestreamHeader
                | Self::JpxCompositingLayerHeader
                | Self::JpxAssociation
                | Self::JpxNumberList
                | Self::JpxOpacity
                | Self::JpxReaderRequirements
        )
    }
}

/// JP2 box structure.
#[derive(Debug, Clone)]
pub struct Jp2Box {
    /// Box type
    pub box_type: BoxType,
    /// Box data offset in file
    pub data_offset: u64,
    /// Box data length
    pub data_length: u64,
    /// Box content
    pub content: Vec<u8>,
}

impl Jp2Box {
    /// Create a new JP2 box.
    pub fn new(box_type: BoxType, data_offset: u64, data_length: u64) -> Self {
        Self {
            box_type,
            data_offset,
            data_length,
            content: Vec::new(),
        }
    }

    /// Read box content from reader.
    pub fn read_content<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        reader.seek(SeekFrom::Start(self.data_offset))?;
        let len = self.data_length.min(u32::MAX as u64) as usize;
        self.content.resize(len, 0);
        reader.read_exact(&mut self.content)?;
        Ok(())
    }

    /// Check if box is a container box (can contain other boxes).
    pub fn is_container(&self) -> bool {
        matches!(
            self.box_type,
            BoxType::Jp2Header
                | BoxType::Resolution
                | BoxType::UuidInfo
                | BoxType::JpxCodestreamHeader
                | BoxType::JpxCompositingLayerHeader
                | BoxType::JpxAssociation
        )
    }
}

/// JP2 file parser.
pub struct Jp2Parser<R> {
    reader: R,
    boxes: Vec<Jp2Box>,
    position: u64,
}

impl<R: Read + Seek> Jp2Parser<R> {
    /// Create a new JP2 parser.
    pub fn new(mut reader: R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;
        Ok(Self {
            reader,
            boxes: Vec::new(),
            position: 0,
        })
    }

    /// Parse all boxes in the JP2 file.
    pub fn parse(&mut self) -> Result<()> {
        self.reader.seek(SeekFrom::Start(0))?;
        self.position = 0;

        // Verify JP2 signature
        self.verify_signature()?;

        // Parse all boxes
        while self.parse_box()? {}

        Ok(())
    }

    /// Verify JP2 file signature.
    fn verify_signature(&mut self) -> Result<()> {
        // Read signature box
        let length = self.reader.read_u32::<BigEndian>()?;
        let mut box_type = [0u8; 4];
        self.reader.read_exact(&mut box_type)?;

        if length != 12 || &box_type != b"jP  " {
            return Err(Error::jpeg2000("Invalid JP2 signature"));
        }

        // Read signature value (should be 0x0D0A870A)
        let signature = self.reader.read_u32::<BigEndian>()?;
        if signature != 0x0D0A870A {
            return Err(Error::jpeg2000("Invalid JP2 signature value"));
        }

        self.position = 12;
        Ok(())
    }

    /// Parse a single box at `self.position`.
    ///
    /// Container (superbox) types — `jp2h`, `res `, `uinf` and the JPX Part-2
    /// grouping boxes — are recursed into: their child boxes are parsed within
    /// the container's byte range and registered (flattened) in `self.boxes`, so
    /// that [`Self::find_box`] can locate nested boxes such as `ihdr`/`colr` that
    /// always live inside `jp2h` on a spec-conformant file. On return
    /// `self.position` points just past the box that was parsed.
    fn parse_box(&mut self) -> Result<bool> {
        self.reader.seek(SeekFrom::Start(self.position))?;

        let header_start = self.position;

        // Try to read box header
        let length = match self.reader.read_u32::<BigEndian>() {
            Ok(l) => l,
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(e) => return Err(e.into()),
        };

        let mut box_type_bytes = [0u8; 4];
        self.reader.read_exact(&mut box_type_bytes)?;
        let box_type = BoxType::from_bytes(box_type_bytes);

        // Resolve the box's payload offset/length and the offset of the next box.
        let (data_offset, data_length, next_position) = if length == 1 {
            // Extended (64-bit) length: 8-byte header + 8-byte XLBox.
            let xl = self.reader.read_u64::<BigEndian>()?;
            let data_offset = header_start + 16;
            let data_length = xl.saturating_sub(16);
            (data_offset, data_length, data_offset + data_length)
        } else if length == 0 {
            // Box extends to end of file.
            let file_size = self.reader.seek(SeekFrom::End(0))?;
            let data_offset = header_start + 8;
            let data_length = file_size.saturating_sub(data_offset);
            (data_offset, data_length, file_size)
        } else {
            let data_offset = header_start + 8;
            let data_length = (length as u64).saturating_sub(8);
            (data_offset, data_length, data_offset + data_length)
        };

        let jp2_box = Jp2Box::new(box_type, data_offset, data_length);
        let is_container = jp2_box.is_container();
        self.boxes.push(jp2_box);

        if is_container && data_length >= 8 {
            self.parse_boxes_in_range(data_offset, data_offset + data_length)?;
        }

        self.position = next_position;
        Ok(true)
    }

    /// Parse the sequence of child boxes occupying `[start, end)`, recursing into
    /// nested superboxes.
    fn parse_boxes_in_range(&mut self, start: u64, end: u64) -> Result<()> {
        let mut pos = start;
        // A box header is at least 8 bytes (length + type).
        while pos + 8 <= end {
            self.position = pos;
            if !self.parse_box()? {
                break;
            }
            // Guard against non-progress / malformed lengths that would loop.
            if self.position <= pos || self.position > end {
                break;
            }
            pos = self.position;
        }
        Ok(())
    }

    /// Get all parsed boxes.
    pub fn boxes(&self) -> &[Jp2Box] {
        &self.boxes
    }

    /// Get box by type.
    pub fn find_box(&self, box_type: BoxType) -> Option<&Jp2Box> {
        self.boxes.iter().find(|b| b.box_type == box_type)
    }

    /// Get all boxes of a specific type.
    pub fn find_boxes(&self, box_type: BoxType) -> Vec<&Jp2Box> {
        self.boxes
            .iter()
            .filter(|b| b.box_type == box_type)
            .collect()
    }

    /// Get mutable reference to reader.
    pub fn reader_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Get reference to reader.
    pub fn reader(&self) -> &R {
        &self.reader
    }

    /// Read image header information.
    pub fn read_image_header(&mut self) -> Result<ImageHeaderBox> {
        let ihdr_box = self
            .find_box(BoxType::ImageHeader)
            .ok_or_else(|| Error::jpeg2000("Missing image header box"))?;

        let mut ihdr = ihdr_box.clone();
        ihdr.read_content(&mut self.reader)?;

        if ihdr.content.len() < 14 {
            return Err(Error::jpeg2000("Invalid image header box size"));
        }

        let height = u32::from_be_bytes([
            ihdr.content[0],
            ihdr.content[1],
            ihdr.content[2],
            ihdr.content[3],
        ]);
        let width = u32::from_be_bytes([
            ihdr.content[4],
            ihdr.content[5],
            ihdr.content[6],
            ihdr.content[7],
        ]);
        let num_components = u16::from_be_bytes([ihdr.content[8], ihdr.content[9]]);
        let bits_per_component = ihdr.content[10];
        let compression_type = ihdr.content[11];
        let colorspace_unknown = ihdr.content[12];
        let ipr = ihdr.content[13];

        Ok(ImageHeaderBox {
            height,
            width,
            num_components,
            bits_per_component,
            compression_type,
            colorspace_unknown,
            ipr,
        })
    }

    /// Read codestream box.
    pub fn read_codestream(&mut self) -> Result<Vec<u8>> {
        let cs_box = self
            .find_box(BoxType::CodeStream)
            .ok_or_else(|| Error::jpeg2000("Missing codestream box"))?;

        let mut cs = cs_box.clone();
        cs.read_content(&mut self.reader)?;

        Ok(cs.content)
    }
}

/// Image Header box content.
#[derive(Debug, Clone)]
pub struct ImageHeaderBox {
    /// Image height
    pub height: u32,
    /// Image width
    pub width: u32,
    /// Number of components
    pub num_components: u16,
    /// Bits per component
    pub bits_per_component: u8,
    /// Compression type (should be 7 for JP2)
    pub compression_type: u8,
    /// Colorspace unknown flag
    pub colorspace_unknown: u8,
    /// Intellectual property rights flag
    pub ipr: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_type_conversion() {
        let bt = BoxType::from_bytes(*b"jP  ");
        assert_eq!(bt, BoxType::Signature);

        let bytes = bt.to_bytes();
        assert_eq!(&bytes, b"jP  ");
    }

    #[test]
    fn test_box_type_unknown() {
        let bt = BoxType::from_bytes(*b"test");
        assert!(matches!(bt, BoxType::Unknown(_)));
    }

    #[test]
    fn test_box_creation() {
        let jp2_box = Jp2Box::new(BoxType::ImageHeader, 100, 50);
        assert_eq!(jp2_box.box_type, BoxType::ImageHeader);
        assert_eq!(jp2_box.data_offset, 100);
        assert_eq!(jp2_box.data_length, 50);
    }

    #[test]
    fn test_box_is_container() {
        let header_box = Jp2Box::new(BoxType::Jp2Header, 0, 0);
        assert!(header_box.is_container());

        let image_box = Jp2Box::new(BoxType::ImageHeader, 0, 0);
        assert!(!image_box.is_container());
    }
}
