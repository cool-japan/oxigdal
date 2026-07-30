//! File-based [`DataSource`] implementation.
//!
//! # Why the reads are positional
//!
//! A block-oriented reader (a GeoTIFF band walk, say) issues one read per
//! tile/strip — thousands per band. The obvious implementation, `seek` followed
//! by `read_exact`, has to serialise those reads behind a mutex because the file
//! cursor is shared state, and pays two syscalls per block instead of one.
//!
//! On Unix and Windows the OS offers a *positioned* read (`pread(2)`,
//! `ReadFile` with an explicit `OVERLAPPED` offset) that takes the offset as an
//! argument and does not disturb the cursor other readers rely on. That removes
//! both the seek syscall and the lock, so any number of threads can read
//! different blocks of the same file concurrently. Everywhere else — notably
//! `wasm32-unknown-unknown` and WASI, where `std::os::{unix, windows}` do not
//! exist — the only portable primitive is seek-then-read, which is kept behind a
//! `Mutex` exactly as before. See [`FileHandle`].

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{IoError, OxiGeoError, Result};
use crate::io::traits::{dst_too_small, range_len_usize};
use crate::io::{ByteRange, DataSource};

/// Highest offset a `SeekFrom::Start` seek can express.
///
/// `std` converts the `u64` to the platform's signed 64-bit displacement, so an
/// offset above `i64::MAX` fails the seek on both Unix (`EOVERFLOW`) and Windows
/// (`ERROR_NEGATIVE_SEEK`). The positional read paths would happily accept such
/// an offset, so it is rejected explicitly to keep the observable error identical
/// to the seek-based implementation this replaced.
const MAX_SEEK_OFFSET: u64 = i64::MAX as u64;

// ---------------------------------------------------------------------------
// FileHandle — the platform-specific positioned read
// ---------------------------------------------------------------------------

/// An open file that can serve a positioned read through `&self`.
///
/// The representation is chosen per platform: a bare [`File`] where the OS has a
/// positional read, a `Mutex<File>` where the shared cursor has to be protected.
#[cfg(any(unix, windows))]
#[derive(Debug)]
struct FileHandle(File);

/// An open file that can serve a positioned read through `&self`.
///
/// Fallback representation for targets without `pread`-style APIs: the shared
/// file cursor makes seek-then-read a critical section.
#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
struct FileHandle(std::sync::Mutex<File>);

#[cfg(unix)]
impl FileHandle {
    /// Wraps an open file.
    fn new(file: File) -> Self {
        Self(file)
    }

    /// Fills `buf` from `offset` using `pread(2)`, which neither uses nor moves
    /// the shared file cursor.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        use std::os::unix::fs::FileExt;
        self.0
            .read_exact_at(buf, offset)
            .map_err(|e| read_err(buf.len(), offset, &e))
    }
}

#[cfg(windows)]
impl FileHandle {
    /// Wraps an open file.
    fn new(file: File) -> Self {
        Self(file)
    }

    /// Fills `buf` from `offset` using `ReadFile` with an explicit `OVERLAPPED`
    /// offset.
    ///
    /// `seek_read` is allowed to return a short read, so this loops the same way
    /// `std`'s own `read_exact_at` does on Unix, and reports the same
    /// `UnexpectedEof` when the file ends first. Each call carries its own
    /// offset, so concurrent readers cannot steal each other's position.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        use std::os::windows::fs::FileExt;

        let total = buf.len();
        let mut remaining = buf;
        let mut position = offset;
        while !remaining.is_empty() {
            match self.0.seek_read(remaining, position) {
                Ok(0) => break,
                Ok(n) => {
                    let rest = remaining;
                    remaining = &mut rest[n..];
                    position = position.saturating_add(n as u64);
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(read_err(total, offset, &e)),
            }
        }
        if remaining.is_empty() {
            Ok(())
        } else {
            Err(read_err(
                total,
                offset,
                &io::Error::new(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer"),
            ))
        }
    }
}

#[cfg(not(any(unix, windows)))]
impl FileHandle {
    /// Wraps an open file behind the cursor lock.
    fn new(file: File) -> Self {
        Self(std::sync::Mutex::new(file))
    }

    /// Fills `buf` from `offset` by seeking the shared cursor under the lock.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = self.0.lock().map_err(|e| OxiGeoError::Internal {
            message: format!("Failed to lock file mutex: {e}"),
        })?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| OxiGeoError::Io(IoError::Seek { position: offset }))?;
        file.read_exact(buf)
            .map_err(|e| read_err(buf.len(), offset, &e))
    }
}

/// Builds the read error, worded exactly as the seek-and-read implementation
/// this module replaced worded it.
fn read_err(len: usize, offset: u64, e: &io::Error) -> OxiGeoError {
    OxiGeoError::Io(IoError::Read {
        message: format!("Failed to read {len} bytes at offset {offset}: {e}"),
    })
}

// ---------------------------------------------------------------------------
// FileDataSource
// ---------------------------------------------------------------------------

/// A file-based data source
pub struct FileDataSource {
    path: PathBuf,
    handle: FileHandle,
    size: u64,
}

impl FileDataSource {
    /// Opens a file as a data source
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|e| {
            OxiGeoError::Io(IoError::Read {
                message: format!("Failed to open file '{}': {}", path.display(), e),
            })
        })?;

        let metadata = file.metadata().map_err(|e| {
            OxiGeoError::Io(IoError::Read {
                message: format!("Failed to get file metadata: {e}"),
            })
        })?;

        Ok(Self {
            path,
            handle: FileHandle::new(file),
            size: metadata.len(),
        })
    }

    /// Returns the file path
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl DataSource for FileDataSource {
    fn size(&self) -> Result<u64> {
        Ok(self.size)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let len = range_len_usize(range)?;
        let mut buffer = vec![0u8; len];
        // `read_range_into` is overridden below, so this cannot recurse.
        self.read_range_into(range, &mut buffer)?;
        Ok(buffer)
    }

    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        let len = range_len_usize(range)?;
        if dst.len() < len {
            return Err(dst_too_small(len, dst.len()));
        }
        if range.start > MAX_SEEK_OFFSET {
            return Err(OxiGeoError::Io(IoError::Seek {
                position: range.start,
            }));
        }
        // An empty range performs no syscall at all: `read_exact_at` on an empty
        // buffer returns immediately, matching the old `seek` + `read_exact(&mut [])`.
        self.handle.read_exact_at(&mut dst[..len], range.start)?;
        Ok(len)
    }
}

impl std::fmt::Debug for FileDataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileDataSource")
            .field("path", &self.path)
            .field("size", &self.size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::env::temp_dir;
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two test
    /// binaries — nor two concurrent runs of this one — can ever land on the same
    /// file.  Dropping the guard removes the fixture, so a panicking test leaks
    /// nothing.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(temp_dir().join(format!(
                "oxigeo_core_file_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for TempPath {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn write_temp_file(name: &str, data: &[u8]) -> TempPath {
        let path = TempPath::new(name);
        let mut f = fs::File::create(&path).expect("create temp file");
        f.write_all(data).expect("write temp data");
        f.flush().expect("flush temp file");
        path
    }

    #[test]
    fn test_file_read_range_and_into_agree() {
        let data: Vec<u8> = (0u8..=255u8).collect();
        let path = write_temp_file("ds_agree.bin", &data);
        let src = FileDataSource::open(&path).expect("open");

        assert_eq!(src.size().expect("size"), 256);

        let range = ByteRange::new(10, 60);
        let owned = src.read_range(range).expect("read_range");
        let mut buf = vec![0u8; 50];
        let n = src
            .read_range_into(range, &mut buf)
            .expect("read_range_into");
        assert_eq!(n, 50);
        assert_eq!(owned, buf);
        assert_eq!(buf, data[10..60]);
    }

    #[test]
    fn test_file_read_range_into_empty_range() {
        let path = write_temp_file("ds_empty.bin", &[1, 2, 3, 4]);
        let src = FileDataSource::open(&path).expect("open");

        let mut empty: [u8; 0] = [];
        let n = src
            .read_range_into(ByteRange::new(2, 2), &mut empty)
            .expect("empty range");
        assert_eq!(n, 0);
    }

    #[test]
    fn test_file_read_range_into_rejects_short_dst() {
        let path = write_temp_file("ds_short.bin", &[1, 2, 3, 4, 5, 6, 7, 8]);
        let src = FileDataSource::open(&path).expect("open");

        let mut buf = [0xAAu8; 3];
        let err = src
            .read_range_into(ByteRange::new(0, 4), &mut buf)
            .expect_err("dst too short must be rejected");
        assert!(matches!(err, OxiGeoError::InvalidParameter { .. }));
        // No I/O happened: the buffer is untouched.
        assert_eq!(buf, [0xAAu8; 3]);
    }

    #[test]
    fn test_file_read_range_into_past_eof_errors() {
        let path = write_temp_file("ds_eof.bin", &[1, 2, 3, 4]);
        let src = FileDataSource::open(&path).expect("open");

        let range = ByteRange::new(2, 16);
        let owned_err = src.read_range(range).expect_err("past EOF");
        let mut buf = vec![0u8; 14];
        let into_err = src.read_range_into(range, &mut buf).expect_err("past EOF");
        assert_eq!(format!("{owned_err}"), format!("{into_err}"));
    }
}
