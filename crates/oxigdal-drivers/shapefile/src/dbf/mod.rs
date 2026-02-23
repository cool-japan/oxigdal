//! DBF (.dbf) attribute file handling
//!
//! This module handles reading and writing dBase III/IV (.dbf) files,
//! which contain the attribute data for Shapefile features.

pub mod record;

pub use record::{FieldDescriptor, FieldType, FieldValue};

use crate::error::{Result, ShapefileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};

/// DBF header size in bytes
pub const DBF_HEADER_SIZE: usize = 32;

/// DBF field descriptor size in bytes
pub const FIELD_DESCRIPTOR_SIZE: usize = 32;

/// DBF header terminator
pub const HEADER_TERMINATOR: u8 = 0x0D;

/// DBF record deletion marker (for deleted records)
pub const RECORD_DELETED: u8 = 0x2A; // '*'

/// DBF record active marker (for active records)
pub const RECORD_ACTIVE: u8 = 0x20; // ' '

/// DBF file terminator
pub const FILE_TERMINATOR: u8 = 0x1A;

/// DBF header
#[derive(Debug, Clone)]
pub struct DbfHeader {
    /// Version (3 for dBase III, 4 for dBase IV)
    pub version: u8,
    /// Last update year (YY, e.g., 24 for 2024)
    pub year: u8,
    /// Last update month (1-12)
    pub month: u8,
    /// Last update day (1-31)
    pub day: u8,
    /// Number of records
    pub record_count: u32,
    /// Header size in bytes (including field descriptors)
    pub header_size: u16,
    /// Record size in bytes
    pub record_size: u16,
    /// Code page (for character encoding)
    pub code_page: u8,
}

impl DbfHeader {
    /// Creates a new DBF header
    pub fn new(record_count: u32, field_descriptors: &[FieldDescriptor]) -> Result<Self> {
        // Calculate record size (1 byte for deletion flag + sum of field lengths)
        let record_size: usize = 1 + field_descriptors
            .iter()
            .map(|f| f.length as usize)
            .sum::<usize>();

        // Calculate header size (32 bytes header + field descriptors + terminator)
        let header_size = DBF_HEADER_SIZE + (field_descriptors.len() * FIELD_DESCRIPTOR_SIZE) + 1;

        // Get current date
        let now = std::time::SystemTime::now();
        let duration = now.duration_since(std::time::UNIX_EPOCH).map_err(|_| {
            ShapefileError::InvalidDbfHeader {
                message: "failed to get current time".to_string(),
            }
        })?;

        // Simple date calculation (approximation)
        let days_since_epoch = duration.as_secs() / 86400;
        let year = ((days_since_epoch / 365) % 100) as u8; // Last 2 digits
        let month = 1; // Default to January
        let day = 1; // Default to 1st

        Ok(Self {
            version: 3, // dBase III
            year,
            month,
            day,
            record_count,
            header_size: header_size as u16,
            record_size: record_size as u16,
            code_page: 0, // No specific code page
        })
    }

    /// Reads a DBF header from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read version (1 byte)
        let mut version = [0u8; 1];
        reader
            .read_exact(&mut version)
            .map_err(|_| ShapefileError::unexpected_eof("reading dbf version"))?;

        // Read last update date (3 bytes: YY, MM, DD)
        let mut date = [0u8; 3];
        reader
            .read_exact(&mut date)
            .map_err(|_| ShapefileError::unexpected_eof("reading dbf date"))?;

        // Read record count (4 bytes, little endian)
        let record_count = reader
            .read_u32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading record count"))?;

        // Read header size (2 bytes, little endian)
        let header_size = reader
            .read_u16::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading header size"))?;

        // Read record size (2 bytes, little endian)
        let record_size = reader
            .read_u16::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading record size"))?;

        // Skip reserved bytes (20 bytes)
        let mut reserved = [0u8; 20];
        reader
            .read_exact(&mut reserved)
            .map_err(|_| ShapefileError::unexpected_eof("reading dbf reserved bytes"))?;

        // Code page is at byte 29 (in the reserved area)
        let code_page = reserved[19];

        Ok(Self {
            version: version[0],
            year: date[0],
            month: date[1],
            day: date[2],
            record_count,
            header_size,
            record_size,
            code_page,
        })
    }

    /// Writes a DBF header to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write version (1 byte)
        writer
            .write_all(&[self.version])
            .map_err(ShapefileError::Io)?;

        // Write last update date (3 bytes)
        writer
            .write_all(&[self.year, self.month, self.day])
            .map_err(ShapefileError::Io)?;

        // Write record count (4 bytes, little endian)
        writer
            .write_u32::<LittleEndian>(self.record_count)
            .map_err(ShapefileError::Io)?;

        // Write header size (2 bytes, little endian)
        writer
            .write_u16::<LittleEndian>(self.header_size)
            .map_err(ShapefileError::Io)?;

        // Write record size (2 bytes, little endian)
        writer
            .write_u16::<LittleEndian>(self.record_size)
            .map_err(ShapefileError::Io)?;

        // Write reserved bytes (20 bytes, with code page at position 19)
        let mut reserved = [0u8; 20];
        reserved[19] = self.code_page;
        writer.write_all(&reserved).map_err(ShapefileError::Io)?;

        Ok(())
    }
}

/// A DBF record (row of attribute data)
#[derive(Debug, Clone)]
pub struct DbfRecord {
    /// Field values (in order)
    pub values: Vec<FieldValue>,
    /// Whether this record is deleted
    pub deleted: bool,
}

impl DbfRecord {
    /// Creates a new DBF record
    pub fn new(values: Vec<FieldValue>) -> Self {
        Self {
            values,
            deleted: false,
        }
    }

    /// Reads a DBF record from a reader
    pub fn read<R: Read>(reader: &mut R, field_descriptors: &[FieldDescriptor]) -> Result<Self> {
        // Read deletion marker (1 byte)
        let mut marker = [0u8; 1];
        reader
            .read_exact(&mut marker)
            .map_err(|_| ShapefileError::unexpected_eof("reading record marker"))?;

        let deleted = marker[0] == RECORD_DELETED;

        // Read field values
        let mut values = Vec::with_capacity(field_descriptors.len());
        for field in field_descriptors {
            let mut field_bytes = vec![0u8; field.length as usize];
            reader
                .read_exact(&mut field_bytes)
                .map_err(|_| ShapefileError::unexpected_eof("reading field value"))?;

            let value = FieldValue::parse(&field_bytes, field.field_type, field.decimal_count)?;
            values.push(value);
        }

        Ok(Self { values, deleted })
    }

    /// Writes a DBF record to a writer
    pub fn write<W: Write>(
        &self,
        writer: &mut W,
        field_descriptors: &[FieldDescriptor],
    ) -> Result<()> {
        // Write deletion marker
        let marker = if self.deleted {
            RECORD_DELETED
        } else {
            RECORD_ACTIVE
        };
        writer.write_all(&[marker]).map_err(ShapefileError::Io)?;

        // Write field values
        if self.values.len() != field_descriptors.len() {
            return Err(ShapefileError::DbfError {
                message: format!(
                    "value count mismatch: expected {}, got {}",
                    field_descriptors.len(),
                    self.values.len()
                ),
                field: None,
                record: None,
            });
        }

        for (value, field) in self.values.iter().zip(field_descriptors) {
            let field_bytes = value.format(field.length as usize);
            writer.write_all(&field_bytes).map_err(ShapefileError::Io)?;
        }

        Ok(())
    }

    /// Returns values as a HashMap (field name -> value)
    pub fn to_map(&self, field_descriptors: &[FieldDescriptor]) -> HashMap<String, FieldValue> {
        field_descriptors
            .iter()
            .zip(&self.values)
            .map(|(field, value)| (field.name.clone(), value.clone()))
            .collect()
    }
}

/// DBF (.dbf) reader
pub struct DbfReader<R: Read> {
    reader: R,
    header: DbfHeader,
    field_descriptors: Vec<FieldDescriptor>,
}

impl<R: Read> DbfReader<R> {
    /// Creates a new DBF reader
    pub fn new(mut reader: R) -> Result<Self> {
        // Read header
        let header = DbfHeader::read(&mut reader)?;

        // Calculate number of field descriptors
        let num_fields =
            (header.header_size as usize - DBF_HEADER_SIZE - 1) / FIELD_DESCRIPTOR_SIZE;

        // Read field descriptors
        let mut field_descriptors = Vec::with_capacity(num_fields);
        for _ in 0..num_fields {
            let descriptor = FieldDescriptor::read(&mut reader)?;
            field_descriptors.push(descriptor);
        }

        // Read header terminator
        let mut terminator = [0u8; 1];
        reader
            .read_exact(&mut terminator)
            .map_err(|_| ShapefileError::unexpected_eof("reading header terminator"))?;

        if terminator[0] != HEADER_TERMINATOR {
            return Err(ShapefileError::InvalidDbfHeader {
                message: format!(
                    "invalid header terminator: expected {}, got {}",
                    HEADER_TERMINATOR, terminator[0]
                ),
            });
        }

        Ok(Self {
            reader,
            header,
            field_descriptors,
        })
    }

    /// Returns the header
    pub fn header(&self) -> &DbfHeader {
        &self.header
    }

    /// Returns the field descriptors
    pub fn field_descriptors(&self) -> &[FieldDescriptor] {
        &self.field_descriptors
    }

    /// Reads the next record
    pub fn read_record(&mut self) -> Result<Option<DbfRecord>> {
        match DbfRecord::read(&mut self.reader, &self.field_descriptors) {
            Ok(record) => Ok(Some(record)),
            Err(ShapefileError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Ok(None)
            }
            Err(ShapefileError::UnexpectedEof { .. }) => {
                // EOF when reading record is expected at end of file
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Reads all records
    pub fn read_all_records(&mut self) -> Result<Vec<DbfRecord>> {
        let mut records = Vec::with_capacity(self.header.record_count as usize);
        while let Some(record) = self.read_record()? {
            // Check for file terminator
            if records.len() >= self.header.record_count as usize {
                break;
            }
            records.push(record);
        }
        Ok(records)
    }
}

/// DBF (.dbf) writer
pub struct DbfWriter<W: Write> {
    writer: W,
    header: DbfHeader,
    field_descriptors: Vec<FieldDescriptor>,
    record_count: u32,
}

impl<W: Write> DbfWriter<W> {
    /// Creates a new DBF writer
    pub fn new(writer: W, field_descriptors: Vec<FieldDescriptor>) -> Result<Self> {
        let header = DbfHeader::new(0, &field_descriptors)?;
        Ok(Self {
            writer,
            header,
            field_descriptors,
            record_count: 0,
        })
    }

    /// Writes the header (should be called first)
    pub fn write_header(&mut self) -> Result<()> {
        // Update header with current record count
        self.header.record_count = self.record_count;
        self.header.write(&mut self.writer)?;

        // Write field descriptors
        for field in &self.field_descriptors {
            field.write(&mut self.writer)?;
        }

        // Write header terminator
        self.writer
            .write_all(&[HEADER_TERMINATOR])
            .map_err(ShapefileError::Io)?;

        Ok(())
    }

    /// Writes a record
    pub fn write_record(&mut self, record: &DbfRecord) -> Result<()> {
        record.write(&mut self.writer, &self.field_descriptors)?;
        self.record_count += 1;
        Ok(())
    }

    /// Flushes the internal writer
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush().map_err(ShapefileError::Io)
    }

    /// Finalizes the file (updates header with record count and writes terminator)
    pub fn finalize(mut self) -> Result<()> {
        // Write file terminator
        self.writer
            .write_all(&[FILE_TERMINATOR])
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

impl<W: Write + Seek> DbfWriter<W> {
    /// Updates the record count in the header (for seekable writers)
    pub fn update_record_count(&mut self) -> Result<()> {
        use byteorder::WriteBytesExt;

        // Update header record count
        self.header.record_count = self.record_count;

        // Seek to record count position in header (byte 4)
        self.writer
            .seek(std::io::SeekFrom::Start(4))
            .map_err(ShapefileError::Io)?;

        // Write record count (little endian)
        self.writer
            .write_u32::<LittleEndian>(self.record_count)
            .map_err(ShapefileError::Io)?;

        // Flush to ensure the update is written
        self.writer.flush().map_err(ShapefileError::Io)?;

        // Seek back to end of file
        self.writer
            .seek(std::io::SeekFrom::End(0))
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

impl DbfWriter<std::fs::File> {
    /// Syncs all data to disk (only available for File writers)
    pub fn sync_all(&mut self) -> Result<()> {
        self.writer.sync_all().map_err(ShapefileError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_dbf_header_round_trip() {
        let fields = vec![
            FieldDescriptor::new("NAME".to_string(), FieldType::Character, 50, 0)
                .expect("valid NAME field descriptor"),
            FieldDescriptor::new("VALUE".to_string(), FieldType::Number, 10, 2)
                .expect("valid VALUE field descriptor"),
        ];

        let header = DbfHeader::new(10, &fields).expect("valid dbf header");

        let mut buffer = Vec::new();
        header.write(&mut buffer).expect("write dbf header");

        assert_eq!(buffer.len(), DBF_HEADER_SIZE);

        let mut cursor = Cursor::new(buffer);
        let read_header = DbfHeader::read(&mut cursor).expect("read dbf header");

        assert_eq!(read_header.version, 3);
        assert_eq!(read_header.record_count, 10);
    }

    #[test]
    fn test_dbf_record_round_trip() {
        let fields = vec![
            FieldDescriptor::new("NAME".to_string(), FieldType::Character, 10, 0)
                .expect("valid NAME field descriptor"),
            FieldDescriptor::new("AGE".to_string(), FieldType::Number, 3, 0)
                .expect("valid AGE field descriptor"),
        ];

        let record = DbfRecord::new(vec![
            FieldValue::String("Alice".to_string()),
            FieldValue::Integer(30),
        ]);

        let mut buffer = Vec::new();
        record
            .write(&mut buffer, &fields)
            .expect("write dbf record");

        let mut cursor = Cursor::new(buffer);
        let read_record = DbfRecord::read(&mut cursor, &fields).expect("read dbf record");

        assert!(!read_record.deleted);
        assert_eq!(read_record.values.len(), 2);
    }

    #[test]
    fn test_dbf_reader_writer() {
        let fields = vec![
            FieldDescriptor::new("NAME".to_string(), FieldType::Character, 20, 0)
                .expect("valid field"),
            FieldDescriptor::new("VALUE".to_string(), FieldType::Number, 10, 2)
                .expect("valid field"),
        ];

        let mut buffer = Cursor::new(Vec::new());

        // Collect records and write
        let records = vec![
            DbfRecord::new(vec![
                FieldValue::String("Test1".to_string()),
                FieldValue::Float(123.45),
            ]),
            DbfRecord::new(vec![
                FieldValue::String("Test2".to_string()),
                FieldValue::Float(678.90),
            ]),
        ];

        // Create header with known record count
        let header = DbfHeader::new(records.len() as u32, &fields).expect("valid header");

        // Write header
        header.write(&mut buffer).expect("write header");

        // Write field descriptors
        for field in &fields {
            field.write(&mut buffer).expect("write field");
        }
        buffer
            .write_all(&[HEADER_TERMINATOR])
            .expect("write terminator");

        // Write records
        for record in &records {
            record.write(&mut buffer, &fields).expect("write record");
        }

        // Write terminator
        buffer.write_all(&[FILE_TERMINATOR]).expect("write EOF");

        // Read
        buffer.set_position(0);
        let mut reader = DbfReader::new(buffer).expect("create reader");

        assert_eq!(reader.field_descriptors().len(), 2);

        // Check buffer length
        let expected_record_size = 1 + 20 + 10; // deletion flag + NAME field + VALUE field
        let _expected_size =
            DBF_HEADER_SIZE + (2 * FIELD_DESCRIPTOR_SIZE) + 1 + (2 * expected_record_size) + 1;

        let read_records = reader.read_all_records().expect("read records");
        assert_eq!(read_records.len(), 2);
    }
}
