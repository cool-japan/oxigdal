//! JP2 (JPEG2000 Part 1) file-format box parser
//!
//! JP2 files are structured as a flat or nested sequence of *boxes* (also called
//! *atoms*), each with a 4-byte type code and a length field.  Some boxes are
//! *super-boxes* that contain other boxes as their payload.
//!
//! Reference: ISO 15444-1:2019 §I (JP2 file format)

use crate::error::{Jpeg2000Error, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

// ---------------------------------------------------------------------------
// JP2 magic bytes (ISO 15444-1 Table I-1)
// ---------------------------------------------------------------------------

/// The 12-byte JPEG2000 file signature.
pub const JP2_MAGIC: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, // Box length: 12
    0x6A, 0x50, 0x20, 0x20, // Box type: 'jP  '
    0x0D, 0x0A, 0x87, 0x0A, // Compatibility signature
];

// ---------------------------------------------------------------------------
// BoxType
// ---------------------------------------------------------------------------

/// JP2 box type codes (ISO 15444-1 Table I-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxType {
    /// `jP  ` — JPEG2000 signature box (0x6A502020)
    Signature,
    /// `ftyp` — File type box
    FileType,
    /// `JP2H` — JP2 header super-box (note: uppercase in spec, actual bytes `jp2h`)
    Jp2Header,
    /// `ihdr` — Image header box
    ImageHeader,
    /// `colr` — Colour specification box
    ColourSpec,
    /// `pclr` — Palette box
    Palette,
    /// `cmap` — Component mapping box
    ComponentMapping,
    /// `cdef` — Channel definition box
    ChannelDef,
    /// `res ` — Resolution super-box
    Resolution,
    /// `resc` — Capture resolution box
    CaptureResolution,
    /// `resd` — Display resolution box
    DisplayResolution,
    /// `jp2c` — Contiguous codestream box
    ContiguousCodestream,
    /// `jp2i` — Intellectual property rights box
    IntellectualProperty,
    /// `xml ` — XML box
    Xml,
    /// `uuid` — UUID box
    Uuid,
    /// `uinf` — UUID info super-box
    UuidInfo,
    /// Unknown box type (raw 4-byte code stored for pass-through).
    Unknown(u32),
}

impl BoxType {
    /// Decode a `BoxType` from its 4-byte big-endian representation.
    pub fn from_u32(code: u32) -> Self {
        match code {
            0x6A502020 => Self::Signature,            // 'jP  '
            0x66747970 => Self::FileType,             // 'ftyp'
            0x6A703268 => Self::Jp2Header,            // 'jp2h'
            0x69686472 => Self::ImageHeader,          // 'ihdr'
            0x636F6C72 => Self::ColourSpec,           // 'colr'
            0x70636C72 => Self::Palette,              // 'pclr'
            0x636D6170 => Self::ComponentMapping,     // 'cmap'
            0x63646566 => Self::ChannelDef,           // 'cdef'
            0x72657320 => Self::Resolution,           // 'res '
            0x72657363 => Self::CaptureResolution,    // 'resc'
            0x72657364 => Self::DisplayResolution,    // 'resd'
            0x6A703263 => Self::ContiguousCodestream, // 'jp2c'
            0x6A703269 => Self::IntellectualProperty, // 'jp2i'
            0x786D6C20 => Self::Xml,                  // 'xml '
            0x75756964 => Self::Uuid,                 // 'uuid'
            0x75696E66 => Self::UuidInfo,             // 'uinf'
            other => Self::Unknown(other),
        }
    }

    /// Encode this `BoxType` as its 4-byte big-endian representation.
    pub fn to_u32(&self) -> u32 {
        match self {
            Self::Signature => 0x6A502020,
            Self::FileType => 0x66747970,
            Self::Jp2Header => 0x6A703268,
            Self::ImageHeader => 0x69686472,
            Self::ColourSpec => 0x636F6C72,
            Self::Palette => 0x70636C72,
            Self::ComponentMapping => 0x636D6170,
            Self::ChannelDef => 0x63646566,
            Self::Resolution => 0x72657320,
            Self::CaptureResolution => 0x72657363,
            Self::DisplayResolution => 0x72657364,
            Self::ContiguousCodestream => 0x6A703263,
            Self::IntellectualProperty => 0x6A703269,
            Self::Xml => 0x786D6C20,
            Self::Uuid => 0x75756964,
            Self::UuidInfo => 0x75696E66,
            Self::Unknown(v) => *v,
        }
    }

    /// Return the 4 ASCII bytes for this box type.
    pub fn to_bytes(&self) -> [u8; 4] {
        self.to_u32().to_be_bytes()
    }

    /// Return `true` if this box type is a *super-box* that may contain children.
    pub fn is_superbox(&self) -> bool {
        matches!(self, Self::Jp2Header | Self::Resolution | Self::UuidInfo)
    }
}

// ---------------------------------------------------------------------------
// Jp2Box
// ---------------------------------------------------------------------------

/// A parsed JP2 box.
///
/// For super-boxes (`jp2h`, `res `, `uinf`) the payload is recursively parsed
/// and stored in `children`.  For all other boxes the raw payload bytes are
/// stored in `data`.
#[derive(Debug, Clone)]
pub struct Jp2Box {
    /// Box type.
    pub box_type: BoxType,
    /// Byte offset of the *start* of this box (including the header) within the
    /// original data slice passed to [`Jp2Parser::parse`].
    pub offset: u64,
    /// Total box length in bytes (header + payload).  A value of `0` means the
    /// box extends to the end of the enclosing container.
    pub length: u64,
    /// Raw payload bytes (empty for super-boxes that have `children`).
    pub data: Vec<u8>,
    /// Child boxes for super-boxes.
    pub children: Vec<Jp2Box>,
}

impl Jp2Box {
    /// Return the payload length (total length minus the header size).
    pub fn payload_len(&self) -> u64 {
        let hdr = if self.length > u32::MAX as u64 { 16 } else { 8 };
        self.length.saturating_sub(hdr)
    }
}

// ---------------------------------------------------------------------------
// ColorSpace
// ---------------------------------------------------------------------------

/// Color space decoded from a JP2 `colr` box.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorSpace {
    /// sRGB (enumerated color space 16).
    SRgb,
    /// Greyscale (enumerated color space 17).
    Grayscale,
    /// YCbCr (enumerated color space 18).
    YCbCr,
    /// Embedded ICC profile (method 2 or 3).
    Icc(Vec<u8>),
    /// Other enumerated color space (raw value).
    Other(u32),
}

// ---------------------------------------------------------------------------
// Jp2Parser
// ---------------------------------------------------------------------------

/// Parses a JP2 byte stream into a list of [`Jp2Box`] structures.
pub struct Jp2Parser;

impl Jp2Parser {
    /// Parse all top-level boxes from `data`.
    ///
    /// Does **not** require the data to start with the JP2 signature box — use
    /// [`Jp2Parser::validate_signature`] separately if needed.
    pub fn parse(data: &[u8]) -> Result<Vec<Jp2Box>> {
        Self::parse_boxes(data, 0)
    }

    /// Return `true` if `data` begins with the 12-byte JP2 file signature.
    pub fn validate_signature(data: &[u8]) -> bool {
        data.len() >= 12 && data[..12] == JP2_MAGIC
    }

    /// Locate and return a reference to the first `jp2c` (contiguous codestream) box.
    pub fn find_codestream(boxes: &[Jp2Box]) -> Option<&Jp2Box> {
        Self::find_box_recursive(boxes, &BoxType::ContiguousCodestream)
    }

    /// Extract the color space from the first `colr` box in the tree.
    pub fn extract_color_space(boxes: &[Jp2Box]) -> Option<ColorSpace> {
        let colr = Self::find_box_recursive(boxes, &BoxType::ColourSpec)?;
        Self::parse_color_space(&colr.data).ok()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn parse_boxes(data: &[u8], base_offset: u64) -> Result<Vec<Jp2Box>> {
        let mut boxes = Vec::new();
        let mut cursor = Cursor::new(data);
        let global_offset = base_offset;

        loop {
            let start = cursor.position() as usize;
            if start >= data.len() {
                break;
            }

            // Read 4-byte length
            let len_u32 = match cursor.read_u32::<BigEndian>() {
                Ok(v) => v,
                Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(Jpeg2000Error::IoError(e)),
            };

            // Read 4-byte type code
            let type_code = match cursor.read_u32::<BigEndian>() {
                Ok(v) => v,
                Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(Jpeg2000Error::IoError(e)),
            };

            let box_type = BoxType::from_u32(type_code);

            // Determine box length and header size
            let (total_len, header_size): (u64, u64) = if len_u32 == 1 {
                // Extended 64-bit length follows
                let xl = match cursor.read_u64::<BigEndian>() {
                    Ok(v) => v,
                    Err(e) => return Err(Jpeg2000Error::IoError(e)),
                };
                (xl, 16)
            } else if len_u32 == 0 {
                // Box extends to end of data
                (data.len() as u64 - start as u64, 8)
            } else {
                (u64::from(len_u32), 8)
            };

            if total_len < header_size {
                return Err(Jpeg2000Error::BoxParseError {
                    box_type: format!("{:08X}", type_code),
                    reason: format!(
                        "Box length {} is smaller than header size {}",
                        total_len, header_size
                    ),
                });
            }

            let payload_len = total_len - header_size;
            let payload_start = cursor.position() as usize;
            let payload_end = payload_start + payload_len as usize;

            if payload_end > data.len() {
                return Err(Jpeg2000Error::InsufficientData {
                    expected: payload_end,
                    actual: data.len(),
                });
            }

            let payload = &data[payload_start..payload_end];

            let (box_data, children) = if box_type.is_superbox() {
                // Recursively parse child boxes
                let child_offset = global_offset + start as u64 + header_size;
                let children = Self::parse_boxes(payload, child_offset)?;
                (Vec::new(), children)
            } else {
                (payload.to_vec(), Vec::new())
            };

            boxes.push(Jp2Box {
                box_type,
                offset: global_offset + start as u64,
                length: total_len,
                data: box_data,
                children,
            });

            // Advance cursor past the payload
            cursor.set_position(payload_end as u64);
        }

        Ok(boxes)
    }

    fn find_box_recursive<'a>(boxes: &'a [Jp2Box], target: &BoxType) -> Option<&'a Jp2Box> {
        for b in boxes {
            if &b.box_type == target {
                return Some(b);
            }
            if !b.children.is_empty() {
                if let Some(found) = Self::find_box_recursive(&b.children, target) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn parse_color_space(colr_data: &[u8]) -> Result<ColorSpace> {
        if colr_data.len() < 3 {
            return Err(Jpeg2000Error::BoxParseError {
                box_type: "colr".to_string(),
                reason: "colr payload too short".to_string(),
            });
        }

        let method = colr_data[0];
        // Bytes 1 and 2 are precedence and approximation (ignored here)

        match method {
            1 => {
                // Enumerated color space
                if colr_data.len() < 7 {
                    return Err(Jpeg2000Error::BoxParseError {
                        box_type: "colr".to_string(),
                        reason: "colr payload too short for enumerated CS".to_string(),
                    });
                }
                let mut cur = Cursor::new(&colr_data[3..]);
                let cs_code = cur.read_u32::<BigEndian>()?;
                let cs = match cs_code {
                    16 => ColorSpace::SRgb,
                    17 => ColorSpace::Grayscale,
                    18 => ColorSpace::YCbCr,
                    other => ColorSpace::Other(other),
                };
                Ok(cs)
            }
            2 | 3 => {
                // Restricted/full ICC profile
                let profile = colr_data[3..].to_vec();
                Ok(ColorSpace::Icc(profile))
            }
            _ => Err(Jpeg2000Error::UnsupportedFeature(format!(
                "colr method {} not supported",
                method
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Build a minimal JP2 signature block
    fn jp2_sig_bytes() -> Vec<u8> {
        JP2_MAGIC.to_vec()
    }

    // Build a synthetic box with given type code and payload
    fn make_box(type_code: u32, payload: &[u8]) -> Vec<u8> {
        let total_len = 8u32 + payload.len() as u32;
        let mut v = Vec::new();
        v.extend_from_slice(&total_len.to_be_bytes());
        v.extend_from_slice(&type_code.to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn test_validate_signature_valid() {
        let data = jp2_sig_bytes();
        assert!(Jp2Parser::validate_signature(&data));
    }

    #[test]
    fn test_validate_signature_invalid() {
        let data = vec![0u8; 12];
        assert!(!Jp2Parser::validate_signature(&data));
    }

    #[test]
    fn test_validate_signature_too_short() {
        let data = vec![0x6Au8, 0x50];
        assert!(!Jp2Parser::validate_signature(&data));
    }

    #[test]
    fn test_box_type_roundtrip() {
        let types = [
            BoxType::Signature,
            BoxType::FileType,
            BoxType::Jp2Header,
            BoxType::ImageHeader,
            BoxType::ColourSpec,
            BoxType::Palette,
            BoxType::ComponentMapping,
            BoxType::ChannelDef,
            BoxType::Resolution,
            BoxType::CaptureResolution,
            BoxType::DisplayResolution,
            BoxType::ContiguousCodestream,
            BoxType::IntellectualProperty,
            BoxType::Xml,
            BoxType::Uuid,
            BoxType::UuidInfo,
        ];
        for t in &types {
            assert_eq!(BoxType::from_u32(t.to_u32()), *t);
        }
    }

    #[test]
    fn test_box_type_unknown() {
        let t = BoxType::from_u32(0xDEADBEEF);
        assert_eq!(t, BoxType::Unknown(0xDEADBEEF));
        assert_eq!(t.to_u32(), 0xDEADBEEF);
    }

    #[test]
    fn test_box_type_is_superbox() {
        assert!(BoxType::Jp2Header.is_superbox());
        assert!(BoxType::Resolution.is_superbox());
        assert!(BoxType::UuidInfo.is_superbox());
        assert!(!BoxType::ImageHeader.is_superbox());
        assert!(!BoxType::ContiguousCodestream.is_superbox());
    }

    #[test]
    fn test_parse_single_box() {
        // A minimal ftyp box
        let payload = b"jp2 \x00\x00\x00\x00jp2 ";
        let data = make_box(0x66747970, payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].box_type, BoxType::FileType);
        assert_eq!(boxes[0].data, payload.as_ref());
    }

    #[test]
    fn test_parse_multiple_boxes() {
        let mut data = Vec::new();
        data.extend(make_box(0x66747970, b"jp2 ")); // ftyp
        data.extend(make_box(0x786D6C20, b"<meta/>")); // xml
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].box_type, BoxType::FileType);
        assert_eq!(boxes[1].box_type, BoxType::Xml);
    }

    #[test]
    fn test_parse_full_jp2_structure() {
        let mut data = jp2_sig_bytes();
        data.extend(make_box(0x66747970, b"jp2 \x00\x00\x00\x00jp2 ")); // ftyp
        // jp2c with SOC + EOC minimal codestream
        data.extend(make_box(0x6A703263, &[0xFF, 0x4F, 0xFF, 0xD9]));

        let boxes = Jp2Parser::parse(&data).unwrap();
        // Signature + ftyp + jp2c = 3 boxes
        assert_eq!(boxes.len(), 3);
    }

    #[test]
    fn test_find_codestream() {
        let mut data = jp2_sig_bytes();
        data.extend(make_box(0x6A703263, &[0xFF, 0x4F, 0xFF, 0xD9])); // jp2c

        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::find_codestream(&boxes);
        assert!(cs.is_some());
        assert_eq!(cs.unwrap().box_type, BoxType::ContiguousCodestream);
        assert_eq!(cs.unwrap().data, [0xFF, 0x4F, 0xFF, 0xD9]);
    }

    #[test]
    fn test_find_codestream_none() {
        let data = jp2_sig_bytes();
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert!(Jp2Parser::find_codestream(&boxes).is_none());
    }

    #[test]
    fn test_extract_color_space_srgb() {
        // colr: method=1, precedence=0, approximation=0, enumCS=16
        let colr_payload = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10];
        let data = make_box(0x636F6C72, &colr_payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::extract_color_space(&boxes);
        assert_eq!(cs, Some(ColorSpace::SRgb));
    }

    #[test]
    fn test_extract_color_space_grayscale() {
        let colr_payload = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11];
        let data = make_box(0x636F6C72, &colr_payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::extract_color_space(&boxes);
        assert_eq!(cs, Some(ColorSpace::Grayscale));
    }

    #[test]
    fn test_extract_color_space_icc() {
        let icc_profile = vec![0xAA, 0xBB, 0xCC];
        let mut colr_payload = vec![0x02u8, 0x00, 0x00]; // method=2
        colr_payload.extend_from_slice(&icc_profile);
        let data = make_box(0x636F6C72, &colr_payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::extract_color_space(&boxes).unwrap();
        if let ColorSpace::Icc(profile) = cs {
            assert_eq!(profile, icc_profile);
        } else {
            panic!("expected ICC color space");
        }
    }

    #[test]
    fn test_jp2_box_offset() {
        let data = make_box(0x66747970, b"test");
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes[0].offset, 0);
    }

    #[test]
    fn test_jp2_box_length() {
        let payload = b"hello";
        let data = make_box(0x786D6C20, payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes[0].length, 8 + 5); // header + payload
    }

    #[test]
    fn test_jp2_box_payload_len() {
        let payload = b"world";
        let data = make_box(0x786D6C20, payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes[0].payload_len(), 5);
    }

    #[test]
    fn test_parse_empty_data() {
        let boxes = Jp2Parser::parse(&[]).unwrap();
        assert!(boxes.is_empty());
    }

    #[test]
    fn test_box_type_to_bytes() {
        let bt = BoxType::ContiguousCodestream;
        let bytes = bt.to_bytes();
        assert_eq!(&bytes, b"jp2c");
    }

    #[test]
    fn test_color_space_ycbcr() {
        // enumCS = 18 = 0x12
        let colr_payload = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12];
        let data = make_box(0x636F6C72, &colr_payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::extract_color_space(&boxes);
        assert_eq!(cs, Some(ColorSpace::YCbCr));
    }

    #[test]
    fn test_color_space_other_enumcs() {
        // enumCS = 999 = 0x3E7
        let colr_payload = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x03, 0xE7];
        let data = make_box(0x636F6C72, &colr_payload);
        let boxes = Jp2Parser::parse(&data).unwrap();
        let cs = Jp2Parser::extract_color_space(&boxes).unwrap();
        assert_eq!(cs, ColorSpace::Other(999));
    }

    #[test]
    fn test_unknown_box_preserved() {
        let data = make_box(0xDEADBEEF, b"payload");
        let boxes = Jp2Parser::parse(&data).unwrap();
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].box_type, BoxType::Unknown(0xDEADBEEF));
        assert_eq!(boxes[0].data, b"payload".as_ref());
    }
}
