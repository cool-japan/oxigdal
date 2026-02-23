//! GRIB file reader with high-level API.
//!
//! This module provides a high-level reader for GRIB files with support for
//! filtering by parameter, level, and time.

use crate::error::{GribError, Result};
use crate::grib1::Grib1Message;
use crate::grib2::Grib2Message;
use crate::message::{GribEdition, GribMessage, MessageIterator};
use crate::parameter::{LevelType, Parameter};
use chrono::NaiveDateTime;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// GRIB file reader
pub struct GribReader<R: Read> {
    iter: MessageIterator<R>,
}

impl GribReader<BufReader<File>> {
    /// Open a GRIB file from a path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

impl<R: Read> GribReader<R> {
    /// Create a new GRIB reader from any Read implementation
    pub fn new(reader: R) -> Self {
        Self {
            iter: MessageIterator::new(reader),
        }
    }

    /// Read the next message
    pub fn next_message(&mut self) -> Result<Option<GribRecord>> {
        match self.iter.next() {
            Some(Ok(msg)) => Ok(Some(GribRecord::from_message(msg)?)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Read all messages
    pub fn read_all(&mut self) -> Result<Vec<GribRecord>> {
        let mut records = Vec::new();
        while let Some(record) = self.next_message()? {
            records.push(record);
        }
        Ok(records)
    }

    /// Get the number of messages read so far
    pub fn message_count(&self) -> usize {
        self.iter.message_count()
    }
}

/// Iterator over GRIB records
impl<R: Read> Iterator for GribReader<R> {
    type Item = Result<GribRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_message().transpose()
    }
}

/// High-level GRIB record
#[derive(Debug, Clone)]
pub enum GribRecord {
    /// GRIB Edition 1 record
    Grib1(Grib1Record),
    /// GRIB Edition 2 record
    Grib2(Grib2Record),
}

impl GribRecord {
    /// Create from raw GRIB message
    fn from_message(msg: GribMessage) -> Result<Self> {
        match msg.edition() {
            GribEdition::Grib1 => {
                let grib1_msg = Grib1Message::from_bytes(msg.data())?;
                Ok(Self::Grib1(Grib1Record { message: grib1_msg }))
            }
            GribEdition::Grib2 => {
                let discipline = msg.discipline().ok_or_else(|| {
                    GribError::ParseError("GRIB2 message missing discipline".to_string())
                })?;
                let grib2_msg = Grib2Message::from_bytes(msg.data(), discipline)?;
                Ok(Self::Grib2(Grib2Record { message: grib2_msg }))
            }
        }
    }

    /// Get the parameter
    pub fn parameter(&self) -> Result<Parameter> {
        match self {
            Self::Grib1(r) => r.message.parameter(),
            Self::Grib2(r) => r.message.parameter(),
        }
    }

    /// Get the level type
    pub fn level_type(&self) -> LevelType {
        match self {
            Self::Grib1(r) => r.message.level_type(),
            Self::Grib2(r) => r.message.level_type(),
        }
    }

    /// Get the level value
    pub fn level_value(&self) -> f64 {
        match self {
            Self::Grib1(r) => r.message.level_value(),
            Self::Grib2(r) => r.message.level_value(),
        }
    }

    /// Get the reference time
    pub fn reference_time(&self) -> Option<NaiveDateTime> {
        match self {
            Self::Grib1(r) => r.message.reference_time(),
            Self::Grib2(r) => r.message.reference_time(),
        }
    }

    /// Get the forecast offset in hours
    pub fn forecast_offset_hours(&self) -> u32 {
        match self {
            Self::Grib1(r) => r.message.forecast_offset_hours() as u32,
            Self::Grib2(r) => r.message.forecast_offset_hours(),
        }
    }

    /// Get the valid time
    pub fn valid_time(&self) -> Option<NaiveDateTime> {
        match self {
            Self::Grib1(r) => r.message.valid_time(),
            Self::Grib2(r) => r.message.valid_time(),
        }
    }

    /// Decode the data values
    pub fn decode_data(&self) -> Result<Vec<f32>> {
        match self {
            Self::Grib1(r) => r.message.decode_data(),
            Self::Grib2(r) => r.message.decode_data(),
        }
    }

    /// Get the number of grid points
    pub fn num_points(&self) -> usize {
        match self {
            Self::Grib1(r) => r.message.num_points(),
            Self::Grib2(r) => r.message.num_points(),
        }
    }

    /// Get grid dimensions (ni/nx, nj/ny)
    pub fn grid_dimensions(&self) -> Option<(u32, u32)> {
        match self {
            Self::Grib1(r) => r.message.grid().map(|g| g.dimensions()),
            Self::Grib2(r) => Some(r.message.grid().dimensions()),
        }
    }
}

/// GRIB1 record
#[derive(Debug, Clone)]
pub struct Grib1Record {
    message: Grib1Message,
}

impl Grib1Record {
    /// Get the underlying GRIB1 message
    pub fn message(&self) -> &Grib1Message {
        &self.message
    }
}

/// GRIB2 record
#[derive(Debug, Clone)]
pub struct Grib2Record {
    message: Grib2Message,
}

impl Grib2Record {
    /// Get the underlying GRIB2 message
    pub fn message(&self) -> &Grib2Message {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grib_reader_empty() {
        let data: &[u8] = &[];
        let mut reader = GribReader::new(data);
        assert!(
            reader
                .next_message()
                .expect("Failed to read empty GRIB message")
                .is_none()
        );
        assert_eq!(reader.message_count(), 0);
    }

    #[test]
    fn test_grib_reader_invalid() {
        let data = b"NOT GRIB DATA";
        let mut reader = GribReader::new(&data[..]);
        let result = reader.next_message();
        assert!(result.is_err());
    }
}
