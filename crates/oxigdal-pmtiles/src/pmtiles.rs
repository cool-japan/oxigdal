//! High-level PMTiles reader.

use crate::directory::{DirectoryEntry, decode_directory};
use crate::error::PmTilesError;
use crate::header::PmTilesHeader;

/// A PMTiles v3 archive reader backed by an in-memory byte buffer.
pub struct PmTilesReader {
    /// Parsed header.
    pub header: PmTilesHeader,
    data: Vec<u8>,
}

impl PmTilesReader {
    /// Construct a reader from the raw bytes of a PMTiles file.
    ///
    /// # Errors
    /// Propagates any error from [`PmTilesHeader::parse`].
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PmTilesError> {
        let header = PmTilesHeader::parse(&data)?;
        Ok(Self { header, data })
    }

    /// Return the raw (possibly compressed) bytes of the root directory.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] when the directory region falls
    /// outside the file.
    pub fn raw_root_directory(&self) -> Result<&[u8], PmTilesError> {
        let start = self.header.root_dir_offset as usize;
        let end = start + self.header.root_dir_length as usize;
        if end > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "Root directory [{start}..{end}) out of bounds (file is {} bytes)",
                self.data.len()
            )));
        }
        Ok(&self.data[start..end])
    }

    /// Decode and return the entries of the root directory.
    ///
    /// Note: if `internal_compression` is not `None` the root directory must
    /// be decompressed before calling this method.  This implementation
    /// assumes the directory is uncompressed (as in test/synthetic files).
    ///
    /// # Errors
    /// Propagates errors from [`Self::raw_root_directory`] or [`decode_directory`].
    pub fn root_directory(&self) -> Result<Vec<DirectoryEntry>, PmTilesError> {
        let raw = self.raw_root_directory()?;
        decode_directory(raw)
    }
}
