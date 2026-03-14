//! Pure Rust SQLite binary format reader.
//!
//! Parses the 100-byte SQLite file header and provides page-level access.
//! Reference: <https://www.sqlite.org/fileformat.html>

use crate::error::GpkgError;

/// SQLite text encoding identifier.
#[derive(Debug, Clone, PartialEq)]
pub enum TextEncoding {
    /// UTF-8 encoding (value 1 in the header).
    Utf8,
    /// UTF-16 little-endian encoding (value 2).
    Utf16Le,
    /// UTF-16 big-endian encoding (value 3).
    Utf16Be,
}

/// Parsed 100-byte SQLite file header.
#[derive(Debug, Clone)]
pub struct SqliteHeader {
    /// Actual page size in bytes (raw value 1 maps to 65536).
    pub page_size: u32,
    /// Database size in pages (may be 0 for older files; use `SqliteReader::page_count`).
    pub db_size_pages: u32,
    /// Page number of the first trunk page of the freelist (0 if no freelist).
    pub first_freelist_page: u32,
    /// Total number of free pages in the freelist.
    pub freelist_page_count: u32,
    /// Schema cookie (incremented on each schema change).
    pub schema_version: u32,
    /// Schema format number (1–4).
    pub schema_format: u8,
    /// Suggested default cache size in pages (signed).
    pub default_cache_size: i32,
    /// Text encoding used by the database.
    pub text_encoding: TextEncoding,
    /// User-defined version number (offset 60).
    pub user_version: u32,
    /// Application ID written by `PRAGMA application_id` (offset 68).
    /// Value `0x47504B47` ("GPKG") identifies a GeoPackage file.
    pub application_id: u32,
}

impl SqliteHeader {
    /// Returns `true` if the application_id marks this as a GeoPackage.
    pub fn is_geopackage(&self) -> bool {
        self.application_id == 0x4750_4B47
    }
}

/// Minimal SQLite binary file parser providing header access and page slicing.
pub struct SqliteReader {
    /// Raw file bytes.
    data: Vec<u8>,
    /// Parsed file header.
    pub header: SqliteHeader,
}

impl SqliteReader {
    /// Parse a SQLite file from its raw bytes.
    ///
    /// # Errors
    /// Returns [`GpkgError::InvalidFormat`] when the data is too short or does
    /// not begin with the SQLite magic string.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, GpkgError> {
        const SQLITE_MAGIC: &[u8] = b"SQLite format 3\x00";

        if data.len() < 100 {
            return Err(GpkgError::InvalidFormat(
                "Data too short for SQLite header (need ≥ 100 bytes)".into(),
            ));
        }
        if !data.starts_with(SQLITE_MAGIC) {
            return Err(GpkgError::InvalidFormat("Not a SQLite file".into()));
        }

        // Offset 16: page size (2 bytes, big-endian). Value 1 means 65536.
        let page_size_raw = u16::from_be_bytes([data[16], data[17]]) as u32;
        let page_size = if page_size_raw == 1 {
            65536
        } else {
            page_size_raw
        };

        // Offset 56: text encoding (4 bytes, big-endian).
        let text_encoding = match u32::from_be_bytes([data[56], data[57], data[58], data[59]]) {
            2 => TextEncoding::Utf16Le,
            3 => TextEncoding::Utf16Be,
            _ => TextEncoding::Utf8,
        };

        let header = SqliteHeader {
            page_size,
            // Offset 28: database size in pages.
            db_size_pages: u32::from_be_bytes([data[28], data[29], data[30], data[31]]),
            // Offset 32: first trunk freelist page.
            first_freelist_page: u32::from_be_bytes([data[32], data[33], data[34], data[35]]),
            // Offset 36: total freelist pages.
            freelist_page_count: u32::from_be_bytes([data[36], data[37], data[38], data[39]]),
            // Offset 40: schema cookie.
            schema_version: u32::from_be_bytes([data[40], data[41], data[42], data[43]]),
            // Offset 44: schema format number.
            schema_format: data[44],
            // Offset 48: default cache size (signed).
            default_cache_size: i32::from_be_bytes([data[48], data[49], data[50], data[51]]),
            text_encoding,
            // Offset 60: user version.
            user_version: u32::from_be_bytes([data[60], data[61], data[62], data[63]]),
            // Offset 68: application id (SQLite ≥ 3.8.6).
            // The header is 100 bytes so offset 68+3=71 is always in range.
            application_id: u32::from_be_bytes([data[68], data[69], data[70], data[71]]),
        };

        Ok(Self { data, header })
    }

    /// Return the byte slice for the given page (1-indexed, as per SQLite spec).
    ///
    /// # Errors
    /// Returns [`GpkgError::InvalidFormat`] if `page_num` is 0 or out of range.
    pub fn page(&self, page_num: u32) -> Result<&[u8], GpkgError> {
        if page_num == 0 {
            return Err(GpkgError::InvalidFormat(
                "Page numbers are 1-indexed; 0 is invalid".into(),
            ));
        }
        let page_size = self.header.page_size as usize;
        let offset = (page_num as usize - 1) * page_size;
        let end = offset + page_size;
        if end > self.data.len() {
            return Err(GpkgError::InvalidFormat(format!(
                "Page {page_num} out of range (file has {} bytes, need {end})",
                self.data.len()
            )));
        }
        Ok(&self.data[offset..end])
    }

    /// Return the number of pages, preferring the header value when non-zero.
    pub fn page_count(&self) -> u32 {
        if self.header.db_size_pages > 0 {
            self.header.db_size_pages
        } else {
            (self.data.len() / self.header.page_size as usize) as u32
        }
    }

    /// Return `true` when the file contains at least one complete page.
    pub fn is_valid(&self) -> bool {
        self.data.len() >= self.header.page_size as usize
    }
}
