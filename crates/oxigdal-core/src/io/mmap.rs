//! Memory-mapped `DataSource` implementations
//!
//! This module provides two structs that implement the [`DataSource`] trait (and
//! `std::io::{Read, Seek}` / `std::io::{Read, Write, Seek}`) using the
//! [`memmap2`] crate for zero-copy large-file access:
//!
//! * [`MmapDataSource`] — read-only mapping.
//! * [`MmapDataSourceRw`] — read-write mapping with optional file creation.
//!
//! # Safety contract
//!
//! Memory-mapped I/O is inherently unsafe because the OS may alias the mapped
//! region with the underlying file.  Both structs document the invariants at
//! each `unsafe` site.  The primary responsibility placed on the *caller* is:
//!
//! > **Do not modify the mapped file through any other handle while a mapping
//! > is live.**  Doing so triggers undefined behaviour in Rust because the
//! > memory contents may change underneath an immutable reference.
//!
//! All internal operations are `Result`-returning; there are no `unwrap` calls
//! in production code.

// The entire module is `std`-only (memmap2 requires std).
#![allow(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use memmap2::{Mmap, MmapMut};

use crate::error::{IoError, OxiGdalError, Result};
use crate::io::{ByteRange, DataSource};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build an `OxiGdalError::Io(IoError::Read { … })` from a `std::io::Error`.
#[inline]
fn io_read_err(e: io::Error, context: &str) -> OxiGdalError {
    OxiGdalError::Io(IoError::Read {
        message: format!("{context}: {e}"),
    })
}

/// Build an out-of-bounds error for an attempted range read.
#[inline]
fn out_of_bounds_err(offset: usize, len: usize, mapped_len: usize) -> OxiGdalError {
    OxiGdalError::OutOfBounds {
        message: format!(
            "read_at: offset ({offset}) + length ({len}) = {} exceeds mapping length ({mapped_len})",
            offset.saturating_add(len)
        ),
    }
}

// ---------------------------------------------------------------------------
// MmapDataSource (read-only)
// ---------------------------------------------------------------------------

/// A read-only [`DataSource`] backed by a memory-mapped file.
///
/// The mapping is created once at construction time using [`memmap2::Mmap`].
/// Random-access reads through [`DataSource::read_range`] copy the requested
/// bytes; zero-copy access is available via [`MmapDataSource::as_bytes`] and
/// [`MmapDataSource::read_at`].
///
/// The struct also implements [`std::io::Read`] and [`std::io::Seek`] so that
/// it can be passed to any reader that accepts `R: Read + Seek`.  The internal
/// cursor used by those trait impls is independent of the `DataSource` API.
///
/// # Safety
///
/// Internally this calls `unsafe { memmap2::Mmap::map(&file) }`.  The
/// invariant is: **the file must not be modified through any other handle while
/// the mapping is live**.  Violating this invariant causes undefined behaviour.
pub struct MmapDataSource {
    /// The memory-mapped region.  `None` for zero-length files.
    mmap: Option<Mmap>,
    /// Total byte length of the mapped file (0 for empty files).
    len: usize,
    /// Path of the file, kept for `Debug` / error messages.
    path: PathBuf,
    /// Cursor position for `std::io::Read + Seek`.
    cursor: usize,
}

impl MmapDataSource {
    /// Opens `path` for read-only memory-mapped access.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, its metadata cannot be
    /// read, or `mmap` fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file =
            File::open(&path).map_err(|e| io_read_err(e, &format!("open '{}'", path.display())))?;

        let metadata = file
            .metadata()
            .map_err(|e| io_read_err(e, "get file metadata"))?;

        let file_len = metadata.len() as usize;

        // SAFETY: We have just opened the file and hold the only handle to it
        // within this struct.  We do not mutate the file elsewhere.  The file
        // remains open (kept alive by `_file` inside `Mmap`) for the lifetime
        // of the mapping.  The caller is responsible for not modifying the file
        // externally while the mapping is live.
        let mmap = if file_len == 0 {
            // memmap2 returns EINVAL for zero-length files on Linux; treat as
            // an empty mapping without calling `map`.
            None
        } else {
            Some(unsafe { Mmap::map(&file) }.map_err(|e| io_read_err(e, "mmap read-only"))?)
        };

        Ok(Self {
            mmap,
            len: file_len,
            path,
            cursor: 0,
        })
    }

    /// Returns the total mapped length in bytes (0 for empty files).
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the mapped file is empty.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a byte slice of the entire mapping.
    ///
    /// For empty files this returns an empty slice.
    #[must_use]
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match &self.mmap {
            Some(m) => m.as_ref(),
            None => &[],
        }
    }

    /// Returns a byte slice for `offset..offset+len` without moving the
    /// internal cursor.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::OutOfBounds`] if `offset + len > self.len()`.
    pub fn read_at(&self, offset: usize, len: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!("read_at: offset ({offset}) + length ({len}) overflows usize"),
            })?;
        if end > self.len {
            return Err(out_of_bounds_err(offset, len, self.len));
        }
        Ok(&self.as_bytes()[offset..end])
    }

    /// Returns the path of the underlying file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// --- DataSource impl --------------------------------------------------------

impl DataSource for MmapDataSource {
    fn size(&self) -> Result<u64> {
        Ok(self.len as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let offset = range.start as usize;
        let len = range.len() as usize;
        let data = self.read_at(offset, len)?;
        Ok(data.to_vec())
    }

    fn supports_range_requests(&self) -> bool {
        true
    }
}

// --- std::io::Read + Seek impl ----------------------------------------------

impl Read for MmapDataSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes = self.as_bytes();
        if self.cursor >= self.len {
            return Ok(0); // EOF
        }
        let available = self.len - self.cursor;
        let to_copy = buf.len().min(available);
        buf[..to_copy].copy_from_slice(&bytes[self.cursor..self.cursor + to_copy]);
        self.cursor += to_copy;
        Ok(to_copy)
    }
}

impl Seek for MmapDataSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_cursor: i64 = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => self.len as i64 + n,
            SeekFrom::Current(n) => self.cursor as i64 + n,
        };
        // Per the `Seek` contract, seeking to a negative position is an error,
        // but seeking past the end is permitted (just sets cursor past end).
        if new_cursor < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek to a negative position",
            ));
        }
        self.cursor = new_cursor as usize;
        Ok(self.cursor as u64)
    }
}

impl std::fmt::Debug for MmapDataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MmapDataSource")
            .field("path", &self.path)
            .field("len", &self.len)
            .field("cursor", &self.cursor)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// MmapDataSourceRw (read-write)
// ---------------------------------------------------------------------------

/// A read-write [`DataSource`] backed by a memory-mapped file.
///
/// Supports reading, writing, flushing, and seeking via the standard traits.
/// Use [`MmapDataSourceRw::open`] to map an existing file, or
/// [`MmapDataSourceRw::create`] to create and immediately map a new
/// zero-filled file of a given byte length.
///
/// # Safety
///
/// Internally this calls `unsafe { MmapMut::map_mut(&file) }`.  The same
/// invariant applies as for [`MmapDataSource`]: **the file must not be
/// accessed through any other handle while the mapping is live**.
pub struct MmapDataSourceRw {
    /// The mutable memory-mapped region.
    mmap: MmapMut,
    /// Total byte length of the mapped file.
    len: usize,
    /// Path of the file, kept for `Debug` / error messages.
    path: PathBuf,
    /// Cursor position for `std::io::{Read, Write, Seek}`.
    cursor: usize,
}

impl MmapDataSourceRw {
    /// Opens `path` for read-write memory-mapped access.
    ///
    /// The file must already exist and be non-empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, its metadata cannot be
    /// read, the file is empty, or `mmap_mut` fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| io_read_err(e, &format!("open rw '{}'", path.display())))?;

        let metadata = file
            .metadata()
            .map_err(|e| io_read_err(e, "get file metadata"))?;

        let file_len = metadata.len() as usize;
        if file_len == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "path",
                message: "cannot open a read-write mmap on an empty file; use create() instead"
                    .to_string(),
            });
        }

        // SAFETY: We've opened the file with read+write access and hold the
        // sole File handle within this struct.  The caller must not access the
        // file externally while the mapping is live.
        let mmap =
            unsafe { MmapMut::map_mut(&file) }.map_err(|e| io_read_err(e, "mmap read-write"))?;

        Ok(Self {
            mmap,
            len: file_len,
            path,
            cursor: 0,
        })
    }

    /// Creates a new file at `path`, extends it to `len` bytes, and maps it.
    ///
    /// If `path` already exists it is truncated.  The new file is zero-filled
    /// by the OS.
    ///
    /// # Errors
    ///
    /// Returns an error if `len == 0`, the file cannot be created, `set_len`
    /// fails, or `mmap_mut` fails.
    pub fn create(path: impl AsRef<Path>, len: usize) -> Result<Self> {
        if len == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "len",
                message: "cannot create a zero-length memory-mapped file".to_string(),
            });
        }

        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| io_read_err(e, &format!("create '{}'", path.display())))?;

        // Extend the file to `len` bytes BEFORE mapping; otherwise the mapping
        // would be zero-length.
        file.set_len(len as u64)
            .map_err(|e| io_read_err(e, "set file length"))?;

        // SAFETY: We just created the file, extended it to `len` bytes, and
        // hold the only File handle.  The mapping covers the full file.
        let mmap = unsafe { MmapMut::map_mut(&file) }.map_err(|e| io_read_err(e, "mmap create"))?;

        Ok(Self {
            mmap,
            len,
            path,
            cursor: 0,
        })
    }

    /// Returns the total mapped length in bytes.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the mapped region is empty.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Flushes outstanding changes to disk synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if `msync` / `FlushViewOfFile` fails.
    pub fn flush(&self) -> Result<()> {
        self.mmap.flush().map_err(|e| io_read_err(e, "mmap flush"))
    }

    /// Returns an immutable byte slice of the entire mapping.
    #[must_use]
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Returns a mutable byte slice of the entire mapping.
    #[must_use]
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.mmap
    }

    /// Returns a byte slice for `offset..offset+len` without moving the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::OutOfBounds`] if `offset + len > self.len()`.
    pub fn read_at(&self, offset: usize, len: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!("read_at: offset ({offset}) + length ({len}) overflows usize"),
            })?;
        if end > self.len {
            return Err(out_of_bounds_err(offset, len, self.len));
        }
        Ok(&self.mmap[offset..end])
    }

    /// Overwrites `data.len()` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::OutOfBounds`] if `offset + data.len() > self.len()`.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        let len = data.len();
        let end = offset
            .checked_add(len)
            .ok_or_else(|| OxiGdalError::OutOfBounds {
                message: format!(
                    "write_at: offset ({offset}) + data length ({len}) overflows usize"
                ),
            })?;
        if end > self.len {
            return Err(out_of_bounds_err(offset, len, self.len));
        }
        self.mmap[offset..end].copy_from_slice(data);
        Ok(())
    }

    /// Returns the path of the underlying file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// --- DataSource impl (immutable reads) -------------------------------------

impl DataSource for MmapDataSourceRw {
    fn size(&self) -> Result<u64> {
        Ok(self.len as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let offset = range.start as usize;
        let len = range.len() as usize;
        let data = self.read_at(offset, len)?;
        Ok(data.to_vec())
    }

    fn supports_range_requests(&self) -> bool {
        true
    }
}

// --- std::io::{Read, Write, Seek} impl -------------------------------------

impl Read for MmapDataSourceRw {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cursor >= self.len {
            return Ok(0); // EOF
        }
        let available = self.len - self.cursor;
        let to_copy = buf.len().min(available);
        buf[..to_copy].copy_from_slice(&self.mmap[self.cursor..self.cursor + to_copy]);
        self.cursor += to_copy;
        Ok(to_copy)
    }
}

impl Write for MmapDataSourceRw {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.cursor >= self.len {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write past end of memory-mapped region",
            ));
        }
        let available = self.len - self.cursor;
        let to_copy = buf.len().min(available);
        self.mmap[self.cursor..self.cursor + to_copy].copy_from_slice(&buf[..to_copy]);
        self.cursor += to_copy;
        Ok(to_copy)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.mmap
            .flush()
            .map_err(|e| io::Error::other(format!("mmap flush failed: {e}")))
    }
}

impl Seek for MmapDataSourceRw {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_cursor: i64 = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => self.len as i64 + n,
            SeekFrom::Current(n) => self.cursor as i64 + n,
        };
        if new_cursor < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek to a negative position",
            ));
        }
        self.cursor = new_cursor as usize;
        Ok(self.cursor as u64)
    }
}

impl std::fmt::Debug for MmapDataSourceRw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MmapDataSourceRw")
            .field("path", &self.path)
            .field("len", &self.len)
            .field("cursor", &self.cursor)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs;
    use std::io::{Read, Seek, SeekFrom, Write};

    // Helper: write `data` to a temp file and return the path.
    fn write_temp_file(name: &str, data: &[u8]) -> PathBuf {
        let path = temp_dir().join(name);
        let mut f = fs::File::create(&path).expect("test helper: failed to create temp file");
        f.write_all(data)
            .expect("test helper: failed to write temp data");
        f.flush().expect("test helper: failed to flush temp file");
        path
    }

    // Helper: create a uniquely named temp path for RW tests.
    fn temp_rw_path(name: &str) -> PathBuf {
        temp_dir().join(name)
    }

    // -----------------------------------------------------------------------
    // MmapDataSource tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mmap_read_small_file() {
        let data: Vec<u8> = (0u8..=127u8).collect();
        let path = write_temp_file("mmap_test_small.bin", &data);

        let src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");
        assert_eq!(src.len(), 128);
        assert!(!src.is_empty());
        assert_eq!(src.as_bytes(), &data[..]);
    }

    #[test]
    fn test_mmap_read_at() {
        let data: Vec<u8> = (0u8..200u8).collect();
        let path = write_temp_file("mmap_test_read_at.bin", &data);

        let src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");

        // Read 10 bytes at offset 50
        let slice = src
            .read_at(50, 10)
            .expect("read_at should succeed within bounds");
        assert_eq!(slice, &data[50..60]);

        // Read last byte
        let last = src
            .read_at(199, 1)
            .expect("read_at last byte should succeed");
        assert_eq!(last, &[199u8]);
    }

    #[test]
    fn test_mmap_seek_and_read() {
        let data: Vec<u8> = (0u8..100u8).collect();
        let path = write_temp_file("mmap_test_seek.bin", &data);

        let mut src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");

        // Seek to offset 40 and read 10 bytes
        src.seek(SeekFrom::Start(40))
            .expect("seek to 40 should succeed");
        let mut buf = vec![0u8; 10];
        src.read_exact(&mut buf)
            .expect("read_exact after seek should succeed");
        assert_eq!(&buf, &data[40..50]);
    }

    #[test]
    fn test_mmap_out_of_bounds_err() {
        let data = vec![0u8; 100];
        let path = write_temp_file("mmap_test_oob.bin", &data);

        let src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");

        // Exact boundary — should succeed
        let ok = src.read_at(0, 100);
        assert!(ok.is_ok());

        // One byte over — should fail
        let err = src.read_at(1, 100);
        assert!(err.is_err());
        assert!(matches!(err, Err(OxiGdalError::OutOfBounds { .. })));

        // offset + len overflows for gigantic values
        let overflow = src.read_at(usize::MAX, 1);
        assert!(overflow.is_err());
    }

    #[test]
    fn test_mmap_empty_file_ok() {
        let path = write_temp_file("mmap_test_empty.bin", &[]);

        let src =
            MmapDataSource::open(&path).expect("MmapDataSource::open on empty file should succeed");
        assert_eq!(src.len(), 0);
        assert!(src.is_empty());
        assert_eq!(src.as_bytes(), &[] as &[u8]);

        // read_at 0 bytes at offset 0 is valid on an empty file
        let ok = src.read_at(0, 0);
        assert!(ok.is_ok());

        // Any non-zero read must fail
        let err = src.read_at(0, 1);
        assert!(err.is_err());
    }

    #[test]
    fn test_mmap_large_offset_seek() {
        let data = vec![0u8; 64];
        let path = write_temp_file("mmap_test_large_seek.bin", &data);

        let mut src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");

        // Seeking past end is valid per Seek contract; the cursor is simply set
        // past the end.  Subsequent reads return 0 bytes (EOF).
        let pos = src
            .seek(SeekFrom::Start(1_000_000))
            .expect("seek past end should not error");
        assert_eq!(pos, 1_000_000);

        let mut buf = vec![0u8; 16];
        let n = src
            .read(&mut buf)
            .expect("read after seek past end should not error");
        assert_eq!(n, 0, "read after seek past end returns 0 bytes (EOF)");
    }

    #[test]
    fn test_mmap_datasource_trait_read_range() {
        let data: Vec<u8> = (0u8..=255u8).collect();
        let path = write_temp_file("mmap_test_range.bin", &data);

        let src = MmapDataSource::open(&path).expect("MmapDataSource::open should succeed");

        let range = ByteRange::new(10, 30);
        let bytes = src
            .read_range(range)
            .expect("DataSource::read_range should succeed");
        assert_eq!(bytes, &data[10..30]);

        let size = src.size().expect("DataSource::size should succeed");
        assert_eq!(size, 256);
        assert!(src.supports_range_requests());
    }

    // -----------------------------------------------------------------------
    // MmapDataSourceRw tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mmap_rw_create_and_write() {
        let path = temp_rw_path("mmap_rw_create.bin");
        // Remove if exists from a previous run
        let _ = fs::remove_file(&path);

        {
            let mut rw = MmapDataSourceRw::create(&path, 1024)
                .expect("MmapDataSourceRw::create should succeed");
            assert_eq!(rw.len(), 1024);

            // Write a recognisable pattern at the start
            let pattern: Vec<u8> = (0u8..=255u8).collect();
            rw.write_at(0, &pattern)
                .expect("write_at start should succeed");

            // Write another pattern near the end
            let tail = b"END!";
            rw.write_at(1020, tail)
                .expect("write_at tail should succeed");

            rw.flush().expect("flush should succeed");
        }

        // Reopen read-only and verify
        let ro = MmapDataSource::open(&path)
            .expect("re-opening created file as read-only should succeed");
        assert_eq!(ro.len(), 1024);

        let head = ro.read_at(0, 256).expect("read_at head should succeed");
        let expected: Vec<u8> = (0u8..=255u8).collect();
        assert_eq!(head, &expected[..]);

        let tail = ro.read_at(1020, 4).expect("read_at tail should succeed");
        assert_eq!(tail, b"END!");
    }

    #[test]
    fn test_mmap_rw_write_at() {
        let path = temp_rw_path("mmap_rw_write_at.bin");
        let _ = fs::remove_file(&path);

        let mut rw =
            MmapDataSourceRw::create(&path, 256).expect("MmapDataSourceRw::create should succeed");

        // Write at offset 100
        let data = b"HELLO_WORLD";
        rw.write_at(100, data).expect("write_at should succeed");

        // Verify with read_at
        let read_back = rw
            .read_at(100, data.len())
            .expect("read_at after write_at should succeed");
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_mmap_rw_out_of_bounds() {
        let path = temp_rw_path("mmap_rw_oob.bin");
        let _ = fs::remove_file(&path);

        let mut rw =
            MmapDataSourceRw::create(&path, 128).expect("MmapDataSourceRw::create should succeed");

        // write_at that extends past end should fail
        let data = vec![1u8; 10];
        let err = rw.write_at(120, &data);
        assert!(err.is_err());
        assert!(matches!(err, Err(OxiGdalError::OutOfBounds { .. })));

        // read_at past end should also fail
        let err = rw.read_at(120, 10);
        assert!(err.is_err());
        assert!(matches!(err, Err(OxiGdalError::OutOfBounds { .. })));
    }

    #[test]
    fn test_mmap_rw_std_io_traits() {
        let path = temp_rw_path("mmap_rw_io.bin");
        let _ = fs::remove_file(&path);

        let mut rw =
            MmapDataSourceRw::create(&path, 64).expect("MmapDataSourceRw::create should succeed");

        // Write via std::io::Write
        let payload = b"abcdefghij";
        let written = rw.write(payload).expect("write should succeed");
        assert_eq!(written, payload.len());

        // Seek back to start via std::io::Seek
        rw.seek(SeekFrom::Start(0))
            .expect("seek to start should succeed");

        // Read via std::io::Read
        let mut buf = vec![0u8; payload.len()];
        rw.read_exact(&mut buf).expect("read_exact should succeed");
        assert_eq!(&buf, payload);
    }

    #[test]
    fn test_mmap_rw_datasource_trait() {
        let path = temp_rw_path("mmap_rw_ds.bin");
        let _ = fs::remove_file(&path);

        let mut rw =
            MmapDataSourceRw::create(&path, 512).expect("MmapDataSourceRw::create should succeed");

        let fill: Vec<u8> = (0u8..=255u8).cycle().take(512).collect();
        rw.write_at(0, &fill).expect("write_at fill should succeed");

        // DataSource::read_range
        let range = ByteRange::new(64, 128);
        let bytes = rw.read_range(range).expect("read_range should succeed");
        assert_eq!(bytes, &fill[64..128]);

        assert_eq!(rw.size().expect("size should succeed"), 512);
        assert!(rw.supports_range_requests());
    }

    #[test]
    fn test_mmap_rw_create_zero_len_err() {
        let path = temp_rw_path("mmap_rw_zero_len.bin");
        let _ = fs::remove_file(&path);

        let err = MmapDataSourceRw::create(&path, 0);
        assert!(err.is_err());
        assert!(matches!(
            err,
            Err(OxiGdalError::InvalidParameter {
                parameter: "len",
                ..
            })
        ));
    }
}
