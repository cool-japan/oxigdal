//! GRIB2 Section 7: Data Section

use crate::error::Result;

/// GRIB2 Section 7: Data Section
///
/// Contains the packed binary data values.
#[derive(Debug, Clone)]
pub struct DataSection {
    /// Raw packed binary data
    pub packed_data: Vec<u8>,
}

impl DataSection {
    /// Parses the data section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(Self {
            packed_data: data.to_vec(),
        })
    }
}
