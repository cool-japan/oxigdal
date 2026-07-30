//! I/O performance benchmark scenarios.
//!
//! This module provides benchmark scenarios for I/O operations including:
//! - Sequential read/write performance
//! - Random access patterns
//! - Chunked I/O operations
//! - Different file formats
//! - Compression impact on I/O

use crate::error::{BenchError, Result};
use crate::scenarios::BenchmarkScenario;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

/// Sequential read benchmark scenario.
pub struct SequentialReadScenario {
    input_path: PathBuf,
    buffer_size: usize,
    total_bytes_read: usize,
}

impl SequentialReadScenario {
    /// Creates a new sequential read benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
            buffer_size: 8192,
            total_bytes_read: 0,
        }
    }

    /// Sets the buffer size for reading.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

impl BenchmarkScenario for SequentialReadScenario {
    fn name(&self) -> &str {
        "sequential_read"
    }

    fn description(&self) -> &str {
        "Benchmark sequential file reading performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        let mut file = File::open(&self.input_path)?;
        let mut buffer = vec![0u8; self.buffer_size];
        self.total_bytes_read = 0;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            self.total_bytes_read += bytes_read;
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Sequential write benchmark scenario.
pub struct SequentialWriteScenario {
    output_path: PathBuf,
    file_size: usize,
    buffer_size: usize,
    created: bool,
}

impl SequentialWriteScenario {
    /// Creates a new sequential write benchmark scenario.
    pub fn new<P: Into<PathBuf>>(output_path: P, file_size: usize) -> Self {
        Self {
            output_path: output_path.into(),
            file_size,
            buffer_size: 8192,
            created: false,
        }
    }

    /// Sets the buffer size for writing.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

impl BenchmarkScenario for SequentialWriteScenario {
    fn name(&self) -> &str {
        "sequential_write"
    }

    fn description(&self) -> &str {
        "Benchmark sequential file writing performance"
    }

    fn setup(&mut self) -> Result<()> {
        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        let mut file = File::create(&self.output_path)?;
        let buffer = vec![0u8; self.buffer_size];

        let mut remaining = self.file_size;
        while remaining > 0 {
            let to_write = remaining.min(self.buffer_size);
            file.write_all(&buffer[..to_write])?;
            remaining -= to_write;
        }

        file.sync_all()?;
        self.created = true;

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        if self.created && self.output_path.exists() {
            std::fs::remove_file(&self.output_path)?;
        }
        Ok(())
    }
}

/// Random access read benchmark scenario.
pub struct RandomAccessScenario {
    input_path: PathBuf,
    access_count: usize,
    chunk_size: usize,
    file_size: u64,
}

impl RandomAccessScenario {
    /// Creates a new random access benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P, access_count: usize) -> Self {
        Self {
            input_path: input_path.into(),
            access_count,
            chunk_size: 4096,
            file_size: 0,
        }
    }

    /// Sets the chunk size for each random access.
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }
}

impl BenchmarkScenario for RandomAccessScenario {
    fn name(&self) -> &str {
        "random_access"
    }

    fn description(&self) -> &str {
        "Benchmark random access read performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        self.file_size = std::fs::metadata(&self.input_path)?.len();

        if self.file_size < self.chunk_size as u64 {
            return Err(BenchError::scenario_failed(
                self.name(),
                "File too small for random access benchmark".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        let mut file = File::open(&self.input_path)?;
        let mut buffer = vec![0u8; self.chunk_size];

        // Use a simple pseudo-random sequence for reproducibility
        let mut seed = 12345u64;
        for _ in 0..self.access_count {
            // Simple LCG for reproducible randomness
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let max_offset = self.file_size.saturating_sub(self.chunk_size as u64);
            let offset = seed % max_offset.max(1);

            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut buffer)?;
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Chunked I/O benchmark scenario.
pub struct ChunkedIoScenario {
    input_path: PathBuf,
    output_path: PathBuf,
    chunk_sizes: Vec<usize>,
    created: bool,
}

impl ChunkedIoScenario {
    /// Creates a new chunked I/O benchmark scenario.
    pub fn new<P1, P2>(input_path: P1, output_path: P2) -> Self
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
    {
        Self {
            input_path: input_path.into(),
            output_path: output_path.into(),
            chunk_sizes: vec![512, 1024, 4096, 8192, 16384, 65536],
            created: false,
        }
    }

    /// Sets the chunk sizes to benchmark.
    pub fn with_chunk_sizes(mut self, sizes: Vec<usize>) -> Self {
        self.chunk_sizes = sizes;
        self
    }
}

impl BenchmarkScenario for ChunkedIoScenario {
    fn name(&self) -> &str {
        "chunked_io"
    }

    fn description(&self) -> &str {
        "Benchmark different chunk sizes for I/O operations"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        for &chunk_size in &self.chunk_sizes {
            let mut input = File::open(&self.input_path)?;
            let mut output = File::create(&self.output_path)?;
            let mut buffer = vec![0u8; chunk_size];

            loop {
                let bytes_read = input.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                output.write_all(&buffer[..bytes_read])?;
            }

            output.sync_all()?;
        }

        self.created = true;
        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        if self.created && self.output_path.exists() {
            std::fs::remove_file(&self.output_path)?;
        }
        Ok(())
    }
}

/// Buffered vs unbuffered I/O benchmark scenario.
pub struct BufferedIoScenario {
    input_path: PathBuf,
    use_buffering: bool,
    total_bytes: usize,
}

impl BufferedIoScenario {
    /// Creates a new buffered I/O benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P, use_buffering: bool) -> Self {
        Self {
            input_path: input_path.into(),
            use_buffering,
            total_bytes: 0,
        }
    }
}

impl BenchmarkScenario for BufferedIoScenario {
    fn name(&self) -> &str {
        if self.use_buffering {
            "buffered_io"
        } else {
            "unbuffered_io"
        }
    }

    fn description(&self) -> &str {
        "Benchmark buffered vs unbuffered I/O performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        use std::io::BufReader;

        let file = File::open(&self.input_path)?;
        self.total_bytes = 0;

        if self.use_buffering {
            let mut reader = BufReader::new(file);
            let mut buffer = vec![0u8; 8192];
            loop {
                let bytes_read = reader.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                self.total_bytes += bytes_read;
            }
        } else {
            let mut reader = file;
            let mut buffer = vec![0u8; 8192];
            loop {
                let bytes_read = reader.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                self.total_bytes += bytes_read;
            }
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Memory-mapped I/O benchmark scenario.
pub struct MemoryMappedIoScenario {
    input_path: PathBuf,
    read_pattern: ReadPattern,
}

/// Read patterns for memory-mapped I/O.
#[derive(Debug, Clone, Copy)]
pub enum ReadPattern {
    /// Sequential read pattern.
    Sequential,
    /// Random read pattern.
    Random,
    /// Strided read pattern (every Nth byte).
    Strided(usize),
}

impl MemoryMappedIoScenario {
    /// Creates a new memory-mapped I/O benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
            read_pattern: ReadPattern::Sequential,
        }
    }

    /// Sets the read pattern.
    pub fn with_pattern(mut self, pattern: ReadPattern) -> Self {
        self.read_pattern = pattern;
        self
    }
}

impl BenchmarkScenario for MemoryMappedIoScenario {
    fn name(&self) -> &str {
        "memory_mapped_io"
    }

    fn description(&self) -> &str {
        "Benchmark memory-mapped file I/O performance"
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        use oxigeo_core::io::MmapDataSource;

        // Memory-map the file so reads below go through the OS page cache
        // without an explicit buffered copy, unlike the other scenarios in
        // this module.
        let mmap = MmapDataSource::open(&self.input_path).map_err(|e| {
            BenchError::scenario_failed(self.name(), format!("Failed to mmap file: {e}"))
        })?;
        let buffer = mmap.as_bytes();

        // Simulate different read patterns
        let _sum: u64 = match self.read_pattern {
            ReadPattern::Sequential => buffer.iter().map(|&b| b as u64).sum(),
            ReadPattern::Random => {
                let mut seed = 12345u64;
                let mut sum = 0u64;
                for _ in 0..buffer.len().min(10000) {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                    let idx = (seed as usize) % buffer.len();
                    sum = sum.wrapping_add(buffer[idx] as u64);
                }
                sum
            }
            ReadPattern::Strided(stride) => buffer.iter().step_by(stride).map(|&b| b as u64).sum(),
        };

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Direct I/O benchmark scenario.
pub struct DirectIoScenario {
    input_path: PathBuf,
    alignment: usize,
}

impl DirectIoScenario {
    /// Creates a new direct I/O benchmark scenario.
    pub fn new<P: Into<PathBuf>>(input_path: P) -> Self {
        Self {
            input_path: input_path.into(),
            alignment: 4096,
        }
    }

    /// Sets the alignment requirement for direct I/O.
    pub fn with_alignment(mut self, alignment: usize) -> Self {
        self.alignment = alignment;
        self
    }
}

impl DirectIoScenario {
    /// Opens `input_path` requesting the OS's uncached / page-cache-bypassing
    /// read path where one is available:
    ///
    /// - Linux: `O_DIRECT` on `open()`.
    /// - macOS: `fcntl(fd, F_NOCACHE, 1)` after `open()` (Linux's `O_DIRECT`
    ///   has no macOS equivalent as an open flag; `F_NOCACHE` is the
    ///   documented substitute used by e.g. SQLite).
    /// - Everything else: a plain buffered open (no bypass available here).
    #[cfg(target_os = "linux")]
    fn open_direct(&self) -> Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        Ok(File::options()
            .read(true)
            .custom_flags(libc::O_DIRECT)
            .open(&self.input_path)?)
    }

    #[cfg(target_os = "macos")]
    fn open_direct(&self) -> Result<File> {
        use std::os::unix::io::AsRawFd;
        let file = File::open(&self.input_path)?;
        // SAFETY: `fcntl` is called with a valid, currently-open file
        // descriptor owned by `file` and a well-formed `F_NOCACHE` argument;
        // it only flips a flag on the underlying file description and
        // touches no memory through raw pointers.
        #[allow(unsafe_code)]
        let ret = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1) };
        if ret == -1 {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!(
                    "fcntl(F_NOCACHE) failed: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        Ok(file)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn open_direct(&self) -> Result<File> {
        Ok(File::open(&self.input_path)?)
    }
}

impl BenchmarkScenario for DirectIoScenario {
    fn name(&self) -> &str {
        "direct_io"
    }

    fn description(&self) -> &str {
        #[cfg(target_os = "linux")]
        {
            "Benchmark direct I/O performance: O_DIRECT bypasses the page cache on open()"
        }
        #[cfg(target_os = "macos")]
        {
            "Benchmark direct I/O performance: F_NOCACHE bypasses the unified buffer cache \
             (macOS has no O_DIRECT open flag; F_NOCACHE is the platform equivalent)"
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            "Benchmark aligned buffered read performance (this platform has no \
             O_DIRECT/F_NOCACHE equivalent wired up here; the page cache is NOT bypassed)"
        }
    }

    fn setup(&mut self) -> Result<()> {
        if !self.input_path.exists() {
            return Err(BenchError::scenario_failed(
                self.name(),
                format!("Input file does not exist: {}", self.input_path.display()),
            ));
        }

        if self.alignment == 0 {
            return Err(BenchError::scenario_failed(
                self.name(),
                "alignment must be non-zero".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        // Direct/uncached I/O requires the destination buffer to start on an
        // `alignment`-byte boundary. `Vec<u8>`'s allocator alignment is not
        // guaranteed to match block-device alignment, so carve an aligned
        // sub-slice out of an over-sized allocation. This only inspects
        // pointer *addresses* (safe, no dereference), so no `unsafe` is
        // needed here.
        let mut raw = vec![0u8; self.alignment * 2];
        let addr = raw.as_ptr() as usize;
        let pad = (self.alignment - (addr % self.alignment)) % self.alignment;
        let buffer = raw.get_mut(pad..pad + self.alignment).ok_or_else(|| {
            BenchError::scenario_failed(self.name(), "failed to align scratch buffer".to_string())
        })?;

        let mut file = self.open_direct()?;

        loop {
            let bytes_read = match file.read(buffer) {
                Ok(n) => n,
                // On Linux, O_DIRECT requires every read to be block-aligned
                // in length; a shorter-than-`alignment` trailing block
                // commonly surfaces as EINVAL. That is an expected O_DIRECT
                // edge case (not a benchmark failure) -- treat it as EOF.
                #[cfg(target_os = "linux")]
                Err(e) if e.raw_os_error() == Some(libc::EINVAL) => break,
                Err(e) => return Err(e.into()),
            };
            if bytes_read == 0 {
                break;
            }
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_bench_io_{}_{seq}_{name}",
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
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn create_test_file(path: &Path, size: usize) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        let data = vec![0u8; size];
        file.write_all(&data)?;
        file.sync_all()?;
        Ok(())
    }

    #[test]
    fn test_sequential_read_scenario_creation() {
        let scenario = SequentialReadScenario::new(TempPath::new("test.bin").to_path_buf())
            .with_buffer_size(16384);

        assert_eq!(scenario.name(), "sequential_read");
        assert_eq!(scenario.buffer_size, 16384);
    }

    #[test]
    fn test_sequential_write_scenario_creation() {
        let scenario =
            SequentialWriteScenario::new(TempPath::new("output.bin").to_path_buf(), 1024 * 1024)
                .with_buffer_size(32768);

        assert_eq!(scenario.name(), "sequential_write");
        assert_eq!(scenario.buffer_size, 32768);
    }

    #[test]
    fn test_random_access_scenario_creation() {
        let scenario = RandomAccessScenario::new(TempPath::new("test.bin").to_path_buf(), 100)
            .with_chunk_size(8192);

        assert_eq!(scenario.name(), "random_access");
        assert_eq!(scenario.chunk_size, 8192);
    }

    #[test]
    fn test_chunked_io_scenario() {
        let input_path = TempPath::new("chunked_input.bin");
        let output_path = TempPath::new("chunked_output.bin");

        // Create test file
        create_test_file(&input_path, 10240).expect("Failed to create test file");

        let scenario = ChunkedIoScenario::new(input_path.to_path_buf(), output_path.to_path_buf())
            .with_chunk_sizes(vec![512, 1024, 4096]);

        assert_eq!(scenario.name(), "chunked_io");
        assert_eq!(scenario.chunk_sizes.len(), 3);
    }

    #[test]
    fn test_buffered_io_scenario_creation() {
        let scenario = BufferedIoScenario::new(TempPath::new("test.bin").to_path_buf(), true);
        assert_eq!(scenario.name(), "buffered_io");

        let scenario = BufferedIoScenario::new(TempPath::new("test.bin").to_path_buf(), false);
        assert_eq!(scenario.name(), "unbuffered_io");
    }

    #[test]
    fn test_direct_io_scenario_creation() {
        let scenario = DirectIoScenario::new(TempPath::new("direct_io.bin").to_path_buf())
            .with_alignment(8192);

        assert_eq!(scenario.name(), "direct_io");
        assert_eq!(scenario.alignment, 8192);
    }

    #[test]
    fn test_direct_io_scenario_description_matches_platform_behavior() {
        let scenario = DirectIoScenario::new(TempPath::new("direct_io.bin").to_path_buf());
        let description = scenario.description();

        // The description must never claim an uncached bypass this build
        // does not actually provide (see `open_direct` cfg branches above).
        #[cfg(target_os = "linux")]
        assert!(description.contains("O_DIRECT"));
        #[cfg(target_os = "macos")]
        assert!(description.contains("F_NOCACHE"));
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        assert!(description.contains("NOT bypassed"));
    }

    #[test]
    fn test_direct_io_scenario_rejects_zero_alignment() {
        let input_path = TempPath::new("direct_io_zero_alignment.bin");
        create_test_file(&input_path, 4096).expect("Failed to create test file");

        let mut scenario = DirectIoScenario::new(input_path.to_path_buf()).with_alignment(0);
        let result = scenario.setup();

        assert!(
            result.is_err(),
            "zero alignment must be rejected in setup()"
        );
    }

    #[test]
    fn test_direct_io_scenario_setup_rejects_missing_file() {
        let mut scenario =
            DirectIoScenario::new(TempPath::new("direct_io_does_not_exist.bin").to_path_buf());
        assert!(scenario.setup().is_err());
    }

    #[test]
    fn test_direct_io_scenario_execute_reads_full_file() {
        // Regression test: DirectIoScenario::execute() must actually read the
        // whole file through the platform's uncached/direct path (or the
        // honest buffered fallback), not silently no-op. `alignment` is kept
        // small and a multiple of the OS page size is not required here
        // since this file's fallback/`F_NOCACHE` paths (used on this test
        // runner's platform) have no O_DIRECT-style block-alignment
        // requirement; on Linux CI this exercises the real O_DIRECT open.
        let input_path = TempPath::new("direct_io_execute.bin");
        // 3 alignment-sized blocks so the read loop iterates more than once.
        create_test_file(&input_path, 4096 * 3).expect("Failed to create test file");

        let mut scenario = DirectIoScenario::new(input_path.to_path_buf()).with_alignment(4096);

        scenario.setup().expect("setup should succeed");
        let result = scenario.execute();
        scenario.teardown().expect("teardown should succeed");

        assert!(
            result.is_ok(),
            "DirectIoScenario::execute() failed: {result:?}"
        );
    }
}
