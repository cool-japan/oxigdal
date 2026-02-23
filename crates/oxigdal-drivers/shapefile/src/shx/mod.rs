//! Shapefile index (.shx) file handling
//!
//! This module handles reading and writing the Shapefile index (.shx) file,
//! which contains the offset and length of each record in the .shp file.
//!
//! The .shx file has the same header as the .shp file, followed by pairs of
//! integers (offset and content length in 16-bit words) for each record.

use crate::error::{Result, ShapefileError};
use crate::shp::header::{BoundingBox, HEADER_SIZE, ShapefileHeader};
use crate::shp::shapes::ShapeType;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Seek, SeekFrom, Write};

/// Index entry size in bytes (offset + content length)
pub const INDEX_ENTRY_SIZE: usize = 8;

/// Index entry (offset and length of a record in the .shp file)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexEntry {
    /// Offset in 16-bit words from the start of the .shp file
    pub offset: i32,
    /// Content length in 16-bit words
    pub content_length: i32,
}

impl IndexEntry {
    /// Creates a new index entry
    pub fn new(offset: i32, content_length: i32) -> Self {
        Self {
            offset,
            content_length,
        }
    }

    /// Reads an index entry from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let offset = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading index offset"))?;

        let content_length = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading index content length"))?;

        Ok(Self {
            offset,
            content_length,
        })
    }

    /// Writes an index entry to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer
            .write_i32::<BigEndian>(self.offset)
            .map_err(ShapefileError::Io)?;

        writer
            .write_i32::<BigEndian>(self.content_length)
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

/// Shapefile index (.shx) reader
pub struct ShxReader<R: Read> {
    reader: R,
    header: ShapefileHeader,
}

impl<R: Read> ShxReader<R> {
    /// Creates a new Shapefile index reader
    pub fn new(mut reader: R) -> Result<Self> {
        let header = ShapefileHeader::read(&mut reader)?;
        Ok(Self { reader, header })
    }

    /// Returns the header
    pub fn header(&self) -> &ShapefileHeader {
        &self.header
    }

    /// Reads all index entries
    pub fn read_all_entries(&mut self) -> Result<Vec<IndexEntry>> {
        let mut entries = Vec::new();

        loop {
            match IndexEntry::read(&mut self.reader) {
                Ok(entry) => entries.push(entry),
                Err(ShapefileError::UnexpectedEof { .. }) => {
                    // Expected EOF when we've read all entries
                    break;
                }
                Err(ShapefileError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(entries)
    }

    /// Calculates the number of records from file length
    pub fn record_count(&self) -> usize {
        // File length is in 16-bit words, header is 100 bytes (50 words)
        // Each index entry is 8 bytes (4 words)
        let file_length_bytes = (self.header.file_length as usize) * 2;
        if file_length_bytes < HEADER_SIZE {
            return 0;
        }
        (file_length_bytes - HEADER_SIZE) / INDEX_ENTRY_SIZE
    }
}

/// Shapefile index (.shx) writer
pub struct ShxWriter<W: Write> {
    writer: W,
    header: ShapefileHeader,
    entries: Vec<IndexEntry>,
}

impl<W: Write> ShxWriter<W> {
    /// Creates a new Shapefile index writer
    pub fn new(writer: W, shape_type: ShapeType, bbox: BoundingBox) -> Self {
        let header = ShapefileHeader::new(shape_type, bbox);
        Self {
            writer,
            header,
            entries: Vec::new(),
        }
    }

    /// Adds an index entry
    pub fn add_entry(&mut self, offset: i32, content_length: i32) {
        self.entries.push(IndexEntry::new(offset, content_length));
    }

    /// Writes the header and all entries
    pub fn write_all(&mut self) -> Result<()> {
        // Update file length in header
        // File length in 16-bit words = header (50 words) + entries (4 words each)
        self.header.file_length = 50 + (self.entries.len() as i32 * 4);

        // Write header
        self.header.write(&mut self.writer)?;

        // Write all entries
        for entry in &self.entries {
            entry.write(&mut self.writer)?;
        }

        Ok(())
    }

    /// Flushes the internal writer to ensure all data is written
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush().map_err(ShapefileError::Io)
    }
}

impl<W: Write + Seek> ShxWriter<W> {
    /// Updates the file length in the header (for seekable writers)
    pub fn update_file_length(&mut self) -> Result<()> {
        // Calculate file length in 16-bit words
        self.header.file_length = 50 + (self.entries.len() as i32 * 4);

        // Seek to file length position in header (byte 24)
        self.writer
            .seek(SeekFrom::Start(24))
            .map_err(ShapefileError::Io)?;

        // Write file length (big endian)
        self.writer
            .write_i32::<BigEndian>(self.header.file_length)
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_index_entry_round_trip() {
        let entry = IndexEntry::new(50, 100);

        let mut buffer = Vec::new();
        entry.write(&mut buffer).expect("write index entry");

        assert_eq!(buffer.len(), INDEX_ENTRY_SIZE);

        let mut cursor = Cursor::new(buffer);
        let read_entry = IndexEntry::read(&mut cursor).expect("read index entry");

        assert_eq!(read_entry, entry);
    }

    #[test]
    fn test_shx_reader_writer() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0).expect("valid bbox");
        let mut buffer = Cursor::new(Vec::new());

        // Write
        {
            let mut writer = ShxWriter::new(&mut buffer, ShapeType::Point, bbox);
            writer.add_entry(50, 10); // First record at offset 50, length 10
            writer.add_entry(60, 10); // Second record at offset 60, length 10
            writer.add_entry(70, 10); // Third record at offset 70, length 10
            writer.write_all().expect("write all shx entries");
        }

        // Read
        buffer.set_position(0);
        let mut reader = ShxReader::new(buffer).expect("create shx reader");

        assert_eq!(reader.header().shape_type, ShapeType::Point);
        assert_eq!(reader.record_count(), 3);

        let entries = reader.read_all_entries().expect("read all shx entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].offset, 50);
        assert_eq!(entries[0].content_length, 10);
        assert_eq!(entries[1].offset, 60);
        assert_eq!(entries[2].offset, 70);
    }

    #[test]
    fn test_shx_file_length_calculation() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0)
            .expect("valid bbox for shx file length test");
        let mut buffer = Vec::new();

        let mut writer = ShxWriter::new(&mut buffer, ShapeType::Point, bbox);
        writer.add_entry(50, 10);
        writer.add_entry(60, 10);

        // Before writing, calculate expected file length
        // Header: 50 words, 2 entries: 2 * 4 = 8 words, Total: 58 words
        writer
            .write_all()
            .expect("write all for file length calculation");

        let cursor = Cursor::new(buffer);
        let reader = ShxReader::new(cursor).expect("create reader for file length check");
        assert_eq!(reader.header().file_length, 58);
    }

    #[test]
    fn test_seekable_update() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0)
            .expect("valid bbox for seekable update test");
        let mut buffer = Cursor::new(Vec::new());

        let mut writer = ShxWriter::new(&mut buffer, ShapeType::Point, bbox);
        writer.add_entry(50, 10);
        writer.add_entry(60, 10);
        writer.write_all().expect("write all for seekable update");

        // Update file length
        writer.update_file_length().expect("update file length");

        // Verify
        buffer.set_position(0);
        let reader = ShxReader::new(buffer).expect("create reader after seekable update");
        assert_eq!(reader.header().file_length, 58);
    }
}
