//! I/O abstractions for geospatial data access
//!
//! This module provides traits and implementations for reading and writing
//! geospatial data from various sources.
//!
//! # Features
//!
//! - [`DataSource`] - Synchronous data source trait
//! - `AsyncDataSource` - Asynchronous data source trait (requires `async` feature)
//! - [`ByteRange`] - Byte range specification for partial reads
//! - [`RasterRead`] / [`RasterWrite`] - Raster-specific I/O traits

mod traits;

pub use traits::{
    ByteRange, CogSupport, DataSink, DataSource, OverviewSupport, RasterRead, RasterWrite,
};

#[cfg(feature = "async")]
pub use traits::{AsyncDataSource, AsyncRasterRead};

#[cfg(feature = "std")]
mod file {
    //! File-based data source implementation

    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use crate::error::{IoError, OxiGdalError, Result};
    use crate::io::{ByteRange, DataSource};

    /// A file-based data source
    pub struct FileDataSource {
        path: PathBuf,
        file: Mutex<File>,
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
                OxiGdalError::Io(IoError::Read {
                    message: format!("Failed to open file '{}': {}", path.display(), e),
                })
            })?;

            let metadata = file.metadata().map_err(|e| {
                OxiGdalError::Io(IoError::Read {
                    message: format!("Failed to get file metadata: {e}"),
                })
            })?;

            Ok(Self {
                path,
                file: Mutex::new(file),
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
            let mut file = self.file.lock().map_err(|e| OxiGdalError::Internal {
                message: format!("Failed to lock file mutex: {e}"),
            })?;

            file.seek(SeekFrom::Start(range.start)).map_err(|_| {
                OxiGdalError::Io(IoError::Seek {
                    position: range.start,
                })
            })?;

            let len = range.len() as usize;
            let mut buffer = vec![0u8; len];
            file.read_exact(&mut buffer).map_err(|e| {
                OxiGdalError::Io(IoError::Read {
                    message: format!(
                        "Failed to read {} bytes at offset {}: {}",
                        len, range.start, e
                    ),
                })
            })?;

            Ok(buffer)
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
}

#[cfg(feature = "std")]
pub use file::FileDataSource;
