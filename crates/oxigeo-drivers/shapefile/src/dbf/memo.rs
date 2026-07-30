//! DBF memo field (`.dbt` sibling) reader (dBase IV only in Slice 26).
//!
//! References:
//! - <http://www.clicketyclick.dk/databases/xbase/format/dbt.html>
//! - dBase IV: 512-byte blocks; block 0 is the header; subsequent blocks hold memo
//!   text terminated by `0x1A 0x1A`. Byte 16 of block 0 = version byte
//!   (`0x03` = dBase IV).
//! - dBase III and FoxPro variants are NOT supported in this slice.
//!
//! # File format (dBase IV)
//!
//! ## Header block (512 bytes)
//!
//! | Offset | Size | Description                                       |
//! |-------:|-----:|---------------------------------------------------|
//! |      0 |    4 | Next free block index (LE u32)                    |
//! |     16 |    1 | Version byte (`0x03` for dBase IV)                |
//! |     20 |    2 | Block size in bytes (LE u16, default 512)         |
//! |  other |    - | Reserved / file name (not parsed)                 |
//!
//! ## Data blocks
//!
//! Each memo entry begins with an 8-byte header:
//!
//! | Offset | Size | Description                                       |
//! |-------:|-----:|---------------------------------------------------|
//! |      0 |    4 | Magic `FF FF 08 00`                               |
//! |      4 |    4 | Total entry length in bytes incl. header (LE u32) |
//! |      8 |    n | UTF-8 / ASCII payload, optionally trailing `0x1A` |
//!
//! Some legacy producers omit the 8-byte header and write text directly,
//! terminated by `0x1A 0x1A`.  In that case we fall back to scanning for
//! the terminator pair.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Errors returned by [`MemoFile`].
#[derive(Debug, thiserror::Error)]
pub enum MemoError {
    /// Underlying I/O error (header read, seek, or block read).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// Memo header is structurally invalid (e.g. too short).
    #[error("Invalid memo file header: {0}")]
    InvalidHeader(String),
    /// Memo format is recognised but not supported by this slice.
    #[error("Unsupported memo version: {0}")]
    UnsupportedVersion(String),
    /// Caller requested a block beyond the file's next-free pointer.
    #[error("Memo block index {index} out of range (available={available})")]
    BlockIndexOutOfRange {
        /// Requested block index.
        index: u32,
        /// `next_block` reported by the memo header.
        available: u32,
    },
    /// Fallback terminator search ran off the end of the file or its safety bound.
    #[error("Missing memo block terminator (expected 0x1A 0x1A)")]
    MissingTerminator,
}

/// Memo file dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoVersion {
    /// dBase III (.dbt) — not supported in Slice 26.
    DBase3,
    /// dBase IV (.dbt) — the format implemented here.
    DBase4,
    /// FoxPro (.fpt) — not supported in Slice 26.
    FoxPro,
}

/// Safety bound for the legacy terminator-search path (1 MiB).
///
/// Real-world memo blocks are far smaller; the bound prevents pathological
/// inputs from consuming unbounded memory.
const TERMINATOR_SEARCH_BOUND: usize = 1_000_000;

/// Header magic at the start of a dBase IV memo entry: `FF FF 08 00`.
const DBASE4_ENTRY_MAGIC: [u8; 4] = [0xFF, 0xFF, 0x08, 0x00];

/// dBase IV memo-entry header size (magic + length).
const DBASE4_ENTRY_HEADER_LEN: usize = 8;

/// Default block size for dBase IV memo files when the header value is `0`.
const DEFAULT_BLOCK_SIZE: u32 = 512;

/// Reader for a `.dbt` memo file paired with a `.dbf` table.
///
/// The reader owns its file handle and offers random-access reads of memo
/// blocks by index.  Block 0 is the header and is never returned as a payload.
pub struct MemoFile {
    handle: File,
    block_size: u32,
    version: MemoVersion,
    next_block: u32,
    /// Encoding used to transcode memo payloads (defaults to UTF-8).
    encoding: &'static encoding_rs::Encoding,
}

impl std::fmt::Debug for MemoFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoFile")
            .field("block_size", &self.block_size)
            .field("version", &self.version)
            .field("next_block", &self.next_block)
            .finish_non_exhaustive()
    }
}

impl MemoFile {
    /// Opens a `.dbt` memo file and parses its header block.
    ///
    /// Returns [`MemoError::UnsupportedVersion`] for dBase III (`0x00`) and
    /// any other unknown version byte.  Only dBase IV (`0x03`) is supported.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, MemoError> {
        let mut handle = File::open(path.as_ref())?;
        let mut header = [0u8; 512];
        handle.read_exact(&mut header)?;

        let next_block_bytes: [u8; 4] = header[0..4]
            .try_into()
            .map_err(|_| MemoError::InvalidHeader("could not read next_block field".to_string()))?;
        let next_block = u32::from_le_bytes(next_block_bytes);

        let version_byte = header[16];
        let version = match version_byte {
            0x03 => MemoVersion::DBase4,
            0x00 => {
                return Err(MemoError::UnsupportedVersion(
                    "dBase III memo files not supported (use dBase IV format)".to_string(),
                ));
            }
            other => {
                return Err(MemoError::UnsupportedVersion(format!(
                    "Unknown memo version byte 0x{:02X}",
                    other
                )));
            }
        };

        // dBase IV stores the block size as a little-endian u16 at offset 20.
        // Some files leave it as zero, in which case the default 512 applies.
        let block_size_bytes: [u8; 2] = header[20..22]
            .try_into()
            .map_err(|_| MemoError::InvalidHeader("could not read block_size field".to_string()))?;
        let block_size_raw = u16::from_le_bytes(block_size_bytes) as u32;
        let block_size = if block_size_raw == 0 {
            DEFAULT_BLOCK_SIZE
        } else {
            block_size_raw
        };

        Ok(Self {
            handle,
            block_size,
            version,
            next_block,
            encoding: encoding_rs::UTF_8,
        })
    }

    /// Overrides the encoding used to transcode memo payloads.
    pub fn set_encoding(&mut self, encoding: &'static encoding_rs::Encoding) {
        self.encoding = encoding;
    }

    /// Returns the block size declared in the header (default 512).
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Returns the detected memo file dialect.
    pub fn version(&self) -> MemoVersion {
        self.version
    }

    /// Returns the next-free block index, which is also one past the last
    /// valid block.
    pub fn next_block(&self) -> u32 {
        self.next_block
    }

    /// Reads the memo payload stored in block `index`.
    ///
    /// Returns [`MemoError::BlockIndexOutOfRange`] when `index >= next_block`.
    pub fn read_block(&mut self, index: u32) -> Result<String, MemoError> {
        if index >= self.next_block {
            return Err(MemoError::BlockIndexOutOfRange {
                index,
                available: self.next_block,
            });
        }

        let offset = u64::from(index) * u64::from(self.block_size);
        self.handle.seek(SeekFrom::Start(offset))?;

        // dBase IV: each memo entry begins with `FF FF 08 00 <length-LE-u32>`.
        // `length` includes the 8-byte header.  Some legacy files store text
        // directly, in which case the magic test fails and we fall back to
        // scanning for `0x1A 0x1A`.
        let mut hdr = [0u8; DBASE4_ENTRY_HEADER_LEN];
        self.handle.read_exact(&mut hdr)?;

        if hdr[0..4] != DBASE4_ENTRY_MAGIC {
            self.handle.seek(SeekFrom::Start(offset))?;
            return self.read_block_terminator_search();
        }

        let length_bytes: [u8; 4] = hdr[4..8].try_into().map_err(|_| {
            MemoError::InvalidHeader("could not read entry length field".to_string())
        })?;
        let length = u32::from_le_bytes(length_bytes) as usize;

        // `length` includes the 8-byte header; the actual payload is the
        // remainder.  Use saturating subtraction so a malformed length does
        // not panic.
        let payload_len = length.saturating_sub(DBASE4_ENTRY_HEADER_LEN);
        let mut payload = vec![0u8; payload_len];
        self.handle.read_exact(&mut payload)?;

        // Strip any trailing 0x1A terminator bytes the writer may have included
        // (single or double-byte EOR markers).
        while payload.ends_with(&[0x1A]) {
            payload.pop();
        }

        Ok(self
            .encoding
            .decode_without_bom_handling(&payload)
            .0
            .into_owned())
    }

    /// Legacy fallback: scan forward from the current file position for the
    /// `0x1A 0x1A` end-of-record marker and return the bytes that precede it.
    fn read_block_terminator_search(&mut self) -> Result<String, MemoError> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let n = self.handle.read(&mut chunk)?;
            if n == 0 {
                return Err(MemoError::MissingTerminator);
            }
            buf.extend_from_slice(&chunk[..n]);

            if let Some(pos) = buf.windows(2).position(|w| w == [0x1A, 0x1A]) {
                buf.truncate(pos);
                return Ok(self
                    .encoding
                    .decode_without_bom_handling(&buf)
                    .0
                    .into_owned());
            }

            if buf.len() > TERMINATOR_SEARCH_BOUND {
                return Err(MemoError::MissingTerminator);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_shapefile_memo_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Builds a minimal valid dBase IV `.dbt` fixture with one block per text.
    fn build_dbase4_fixture(path: &Path, blocks: &[&str]) -> io::Result<()> {
        let mut file = File::create(path)?;

        // Header block (512 bytes).  next_block = blocks.len() + 1 (1-based,
        // because block 0 is the header).
        let mut header = [0u8; 512];
        let next_block = (blocks.len() as u32) + 1;
        header[0..4].copy_from_slice(&next_block.to_le_bytes());
        header[16] = 0x03; // dBase IV version byte
        header[20..22].copy_from_slice(&512u16.to_le_bytes());
        file.write_all(&header)?;

        for text in blocks {
            let mut block = vec![0u8; 512];
            block[0..4].copy_from_slice(&DBASE4_ENTRY_MAGIC);
            let length = (DBASE4_ENTRY_HEADER_LEN + text.len() + 2) as u32;
            block[4..8].copy_from_slice(&length.to_le_bytes());
            let text_bytes = text.as_bytes();
            block[8..8 + text_bytes.len()].copy_from_slice(text_bytes);
            block[8 + text_bytes.len()] = 0x1A;
            block[8 + text_bytes.len() + 1] = 0x1A;
            file.write_all(&block)?;
        }

        Ok(())
    }

    #[test]
    fn test_memo_internal_open_and_read() {
        let path = TempPath::new("internal_open.dbt");
        build_dbase4_fixture(&path, &["Hello", "World"]).expect("build fixture");
        let mut memo = MemoFile::open(&path).expect("open memo");
        assert_eq!(memo.version(), MemoVersion::DBase4);
        assert_eq!(memo.block_size(), 512);
        assert_eq!(memo.read_block(1).expect("read block 1"), "Hello");
        assert_eq!(memo.read_block(2).expect("read block 2"), "World");
    }
}
