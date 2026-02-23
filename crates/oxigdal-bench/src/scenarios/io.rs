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
        // Note: This is a placeholder for memory-mapped I/O
        // Actual implementation would use memmap2 crate or similar

        // For now, we'll just read the file normally
        let mut file = File::open(&self.input_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

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

impl BenchmarkScenario for DirectIoScenario {
    fn name(&self) -> &str {
        "direct_io"
    }

    fn description(&self) -> &str {
        "Benchmark direct I/O (O_DIRECT) performance"
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
        // Note: Direct I/O requires platform-specific APIs
        // This is a simplified version that just reads with aligned buffers

        let mut file = File::open(&self.input_path)?;
        let mut buffer = vec![0u8; self.alignment];

        loop {
            let bytes_read = file.read(&mut buffer)?;
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

    fn create_test_file(path: &PathBuf, size: usize) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        let data = vec![0u8; size];
        file.write_all(&data)?;
        file.sync_all()?;
        Ok(())
    }

    #[test]
    fn test_sequential_read_scenario_creation() {
        let scenario = SequentialReadScenario::new("/tmp/test.bin").with_buffer_size(16384);

        assert_eq!(scenario.name(), "sequential_read");
        assert_eq!(scenario.buffer_size, 16384);
    }

    #[test]
    fn test_sequential_write_scenario_creation() {
        let scenario =
            SequentialWriteScenario::new("/tmp/output.bin", 1024 * 1024).with_buffer_size(32768);

        assert_eq!(scenario.name(), "sequential_write");
        assert_eq!(scenario.buffer_size, 32768);
    }

    #[test]
    fn test_random_access_scenario_creation() {
        let scenario = RandomAccessScenario::new("/tmp/test.bin", 100).with_chunk_size(8192);

        assert_eq!(scenario.name(), "random_access");
        assert_eq!(scenario.chunk_size, 8192);
    }

    #[test]
    fn test_chunked_io_scenario() {
        let temp_dir = std::env::temp_dir();
        let input_path = temp_dir.join("test_chunked_input.bin");
        let output_path = temp_dir.join("test_chunked_output.bin");

        // Create test file
        create_test_file(&input_path, 10240).expect("Failed to create test file");

        let scenario = ChunkedIoScenario::new(&input_path, &output_path)
            .with_chunk_sizes(vec![512, 1024, 4096]);

        assert_eq!(scenario.name(), "chunked_io");
        assert_eq!(scenario.chunk_sizes.len(), 3);

        // Cleanup
        let _ = std::fs::remove_file(&input_path);
    }

    #[test]
    fn test_buffered_io_scenario_creation() {
        let scenario = BufferedIoScenario::new("/tmp/test.bin", true);
        assert_eq!(scenario.name(), "buffered_io");

        let scenario = BufferedIoScenario::new("/tmp/test.bin", false);
        assert_eq!(scenario.name(), "unbuffered_io");
    }
}
