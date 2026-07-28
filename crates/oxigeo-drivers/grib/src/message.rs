//! GRIB message parsing and structure.
//!
//! This module provides the core message structure for GRIB files and parsing utilities.
//! GRIB files consist of one or more messages, each containing sections with metadata and data.

use crate::error::{GribError, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Read;

/// GRIB edition/version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GribEdition {
    /// GRIB Edition 1
    Grib1,
    /// GRIB Edition 2
    Grib2,
}

impl GribEdition {
    /// Get edition number
    pub fn number(&self) -> u8 {
        match self {
            Self::Grib1 => 1,
            Self::Grib2 => 2,
        }
    }

    /// Create from edition number
    pub fn from_number(n: u8) -> Result<Self> {
        match n {
            1 => Ok(Self::Grib1),
            2 => Ok(Self::Grib2),
            _ => Err(GribError::UnsupportedEdition(n)),
        }
    }
}

/// GRIB message header
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// GRIB edition
    pub edition: GribEdition,
    /// Total message length in bytes
    pub total_length: usize,
    /// Discipline (GRIB2 only)
    pub discipline: Option<u8>,
}

/// GRIB message container
#[derive(Debug, Clone)]
pub struct GribMessage {
    /// Message header
    pub header: MessageHeader,
    /// Raw message bytes (excluding 'GRIB' and '7777' markers)
    pub data: Vec<u8>,
}

impl GribMessage {
    /// Parse a GRIB message from a reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Option<Self>> {
        // Read 'GRIB' magic bytes
        let mut magic = [0u8; 4];
        match reader.read_exact(&mut magic) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None); // End of file
            }
            Err(e) => return Err(GribError::Io(e)),
        }

        if &magic != b"GRIB" {
            return Err(GribError::InvalidHeader(magic.to_vec()));
        }

        // Read reserved bytes (2 bytes for GRIB1, varies for GRIB2)
        let mut reserved = [0u8; 2];
        reader.read_exact(&mut reserved)?;

        // Read discipline (GRIB2 only, byte 7)
        let mut discipline_byte = [0u8; 1];
        reader.read_exact(&mut discipline_byte)?;

        // Read edition number (byte 8 for both GRIB1 and GRIB2)
        let edition_num = reader.read_u8()?;
        let edition = GribEdition::from_number(edition_num)?;

        // Read total length
        let total_length = match edition {
            GribEdition::Grib1 => {
                // GRIB1: 3-byte length at bytes 5-7 (we already read bytes 5-6 as reserved)
                // Go back and read the 3-byte length properly
                // For GRIB1, total length is at bytes 4-6 (24 bits)
                let b1 = reserved[0];
                let b2 = reserved[1];
                let b3 = discipline_byte[0];
                ((b1 as u32) << 16 | (b2 as u32) << 8 | (b3 as u32)) as usize
            }
            GribEdition::Grib2 => {
                // GRIB2: 8-byte length at bytes 9-16
                reader.read_u64::<BigEndian>()? as usize
            }
        };

        // Validate total length
        if total_length < 16 {
            return Err(GribError::InvalidSectionLength {
                expected: 16,
                actual: total_length,
            });
        }

        // Calculate data length (excluding header and end marker)
        let header_size = match edition {
            GribEdition::Grib1 => 8,  // 'GRIB' + 3 bytes length + 1 byte edition
            GribEdition::Grib2 => 16, // 'GRIB' + reserved + discipline + edition + 8 bytes length
        };
        let data_length = total_length.checked_sub(header_size + 4).ok_or_else(|| {
            GribError::InvalidSectionLength {
                expected: header_size + 4,
                actual: total_length,
            }
        })?;

        // Read the message data. `data_length` derives directly from the
        // attacker-controllable total-length header field (8 bytes for GRIB2),
        // so a `vec![0u8; data_length]` preallocation would let a tiny
        // truncated file request a multi-gigabyte buffer (OOM / DoS). Reading
        // through `take(..).read_to_end(..)` grows the buffer only as bytes
        // actually arrive, then we verify the full section was present.
        let mut data = Vec::new();
        let got = reader
            .by_ref()
            .take(data_length as u64)
            .read_to_end(&mut data)?;
        if got != data_length {
            return Err(GribError::TruncatedMessage {
                expected: data_length,
                actual: got,
            });
        }

        // Read end marker '7777'
        let mut end_marker = [0u8; 4];
        reader.read_exact(&mut end_marker)?;
        if &end_marker != b"7777" {
            return Err(GribError::InvalidEndMarker(end_marker.to_vec()));
        }

        let header = MessageHeader {
            edition,
            total_length,
            discipline: match edition {
                GribEdition::Grib2 => Some(discipline_byte[0]),
                GribEdition::Grib1 => None,
            },
        };

        Ok(Some(GribMessage { header, data }))
    }

    /// Get the edition of this message
    pub fn edition(&self) -> GribEdition {
        self.header.edition
    }

    /// Get the discipline (GRIB2 only)
    pub fn discipline(&self) -> Option<u8> {
        self.header.discipline
    }

    /// Get message data as slice
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get total message length
    pub fn total_length(&self) -> usize {
        self.header.total_length
    }
}

/// Iterator over GRIB messages in a file
pub struct MessageIterator<R: Read> {
    reader: R,
    message_count: usize,
}

impl<R: Read> MessageIterator<R> {
    /// Create a new message iterator
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            message_count: 0,
        }
    }

    /// Get the number of messages read so far
    pub fn message_count(&self) -> usize {
        self.message_count
    }
}

impl<R: Read> Iterator for MessageIterator<R> {
    type Item = Result<GribMessage>;

    fn next(&mut self) -> Option<Self::Item> {
        match GribMessage::from_reader(&mut self.reader) {
            Ok(Some(msg)) => {
                self.message_count += 1;
                Some(Ok(msg))
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Section identifier for GRIB2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionNumber {
    /// Section 0: Indicator Section (header)
    Indicator = 0,
    /// Section 1: Identification Section
    Identification = 1,
    /// Section 2: Local Use Section (optional)
    LocalUse = 2,
    /// Section 3: Grid Definition Section
    GridDefinition = 3,
    /// Section 4: Product Definition Section
    ProductDefinition = 4,
    /// Section 5: Data Representation Section
    DataRepresentation = 5,
    /// Section 6: Bit-Map Section
    BitMap = 6,
    /// Section 7: Data Section
    Data = 7,
    /// Section 8: End Section
    End = 8,
}

impl SectionNumber {
    /// Create from section number
    pub fn from_u8(n: u8) -> Result<Self> {
        match n {
            0 => Ok(Self::Indicator),
            1 => Ok(Self::Identification),
            2 => Ok(Self::LocalUse),
            3 => Ok(Self::GridDefinition),
            4 => Ok(Self::ProductDefinition),
            5 => Ok(Self::DataRepresentation),
            6 => Ok(Self::BitMap),
            7 => Ok(Self::Data),
            8 => Ok(Self::End),
            _ => Err(GribError::InvalidSection(n)),
        }
    }
}

/// GRIB2 section header
#[derive(Debug, Clone)]
pub struct SectionHeader {
    /// Section length in bytes
    pub length: u32,
    /// Section number
    pub number: SectionNumber,
}

impl SectionHeader {
    /// Parse section header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 5 {
            return Err(GribError::InvalidSectionLength {
                expected: 5,
                actual: bytes.len(),
            });
        }

        let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let number = SectionNumber::from_u8(bytes[4])?;

        Ok(Self { length, number })
    }

    /// Returns the section payload length (the parsed `length` minus
    /// `header_octets`, the number of octets already consumed by the
    /// section's fixed-size header before the variable-length payload).
    ///
    /// Validates that `length >= header_octets` first, returning
    /// [`GribError::InvalidSectionLength`] instead of underflowing the
    /// `usize` subtraction. A crafted or corrupt GRIB2 message can set a
    /// section's length field below the minimum header size (e.g. 0-4 for a
    /// 5-octet section header, 0-5 for the 6-octet bitmap-section header);
    /// without this check the subtraction wraps to near-`usize::MAX` in
    /// release builds, and the resulting slice range panics.
    pub fn payload_len(&self, header_octets: usize) -> Result<usize> {
        let length = self.length as usize;
        if length < header_octets {
            return Err(GribError::InvalidSectionLength {
                expected: header_octets,
                actual: length,
            });
        }
        Ok(length - header_octets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_grib_edition() {
        assert_eq!(GribEdition::Grib1.number(), 1);
        assert_eq!(GribEdition::Grib2.number(), 2);

        assert_eq!(
            GribEdition::from_number(1).expect("Failed to create GRIB1 edition from number"),
            GribEdition::Grib1
        );
        assert_eq!(
            GribEdition::from_number(2).expect("Failed to create GRIB2 edition from number"),
            GribEdition::Grib2
        );
        assert!(GribEdition::from_number(3).is_err());
    }

    #[test]
    fn test_invalid_header() {
        let data = b"XXXX";
        let mut cursor = Cursor::new(data);
        let result = GribMessage::from_reader(&mut cursor);
        assert!(result.is_err());
    }

    /// A GRIB2 indicator section that declares a multi-gigabyte total length
    /// but supplies no body must fail fast with a truncation error rather than
    /// speculatively allocating gigabytes from the untrusted length field.
    #[test]
    fn test_from_reader_rejects_truncated_oversized_message() {
        let mut data = Vec::new();
        data.extend_from_slice(b"GRIB"); // magic
        data.extend_from_slice(&[0x00, 0x00]); // reserved
        data.push(0x00); // discipline
        data.push(0x02); // edition = GRIB2
        data.extend_from_slice(&4_000_000_000u64.to_be_bytes()); // total length ~4 GB
        // ...and no message body follows.

        let mut cursor = Cursor::new(data);
        let result = GribMessage::from_reader(&mut cursor);
        assert!(matches!(result, Err(GribError::TruncatedMessage { .. })));
    }

    #[test]
    fn test_section_number() {
        assert_eq!(
            SectionNumber::from_u8(0).expect("Failed to create Indicator section from u8"),
            SectionNumber::Indicator
        );
        assert_eq!(
            SectionNumber::from_u8(1).expect("Failed to create Identification section from u8"),
            SectionNumber::Identification
        );
        assert_eq!(
            SectionNumber::from_u8(3).expect("Failed to create GridDefinition section from u8"),
            SectionNumber::GridDefinition
        );
        assert!(SectionNumber::from_u8(99).is_err());
    }

    #[test]
    fn test_section_header() {
        let bytes = [0, 0, 0, 21, 3]; // Length=21, Section=3
        let header =
            SectionHeader::from_bytes(&bytes).expect("Failed to parse section header from bytes");
        assert_eq!(header.length, 21);
        assert_eq!(header.number, SectionNumber::GridDefinition);
    }

    #[test]
    fn test_message_iterator_empty() {
        let data: &[u8] = &[];
        let mut iter = MessageIterator::new(Cursor::new(data));
        assert!(iter.next().is_none());
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_payload_len_valid() {
        let header = SectionHeader {
            length: 21,
            number: SectionNumber::GridDefinition,
        };
        assert_eq!(
            header.payload_len(5).expect("21 - 5 must not underflow"),
            16
        );
    }

    /// Regression test: a crafted/corrupt section length smaller than the
    /// header size must return a typed error instead of underflowing the
    /// `usize` subtraction (which would wrap to near-`usize::MAX` in
    /// release builds and panic at the subsequent slice operation).
    #[test]
    fn test_payload_len_underflow_returns_error() {
        for length in 0..5u32 {
            let header = SectionHeader {
                length,
                number: SectionNumber::GridDefinition,
            };
            let result = header.payload_len(5);
            assert!(
                result.is_err(),
                "length {length} < header_octets 5 must error, not underflow"
            );
        }

        // Boundary: length == header_octets must yield a zero-length payload.
        let header = SectionHeader {
            length: 5,
            number: SectionNumber::GridDefinition,
        };
        assert_eq!(header.payload_len(5).expect("5 - 5 = 0"), 0);
    }

    #[test]
    fn test_payload_len_underflow_bitmap_header() {
        for length in 0..6u32 {
            let header = SectionHeader {
                length,
                number: SectionNumber::BitMap,
            };
            assert!(header.payload_len(6).is_err());
        }
    }
}
