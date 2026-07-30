//! Chunked reader for efficient sequential reading.

use super::buffer::ChunkedBuffer;
use super::chunked::{ChunkStrategy, ChunkedIO, FileChunkedIO};
use crate::error::{Result, StreamingError};
use bytes::Bytes;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

/// A reader that processes data in chunks.
pub struct ChunkedReader {
    /// The underlying chunked I/O
    io: Box<dyn ChunkedIO>,

    /// Chunk buffer
    buffer: ChunkedBuffer,

    /// Current chunk index
    current_index: usize,

    /// Total number of chunks
    total_chunks: usize,

    /// Total size in bytes
    total_size: u64,

    /// Prefetch semaphore
    prefetch_semaphore: Arc<Semaphore>,

    /// Number of chunks to prefetch
    prefetch_count: usize,
}

impl ChunkedReader {
    /// Create a new chunked reader from a file.
    pub async fn from_file<P: AsRef<Path>>(
        path: P,
        strategy: ChunkStrategy,
        buffer_size: usize,
        prefetch_count: usize,
    ) -> Result<Self> {
        let mut io = FileChunkedIO::new(path, strategy).await?;
        io.open_read().await?;

        let total_size = io.total_size().await?;
        let chunk_size = strategy.chunk_size_for_index(0, 0);
        let buffer = ChunkedBuffer::new(chunk_size, buffer_size);
        let total_chunks = buffer.calculate_chunks(total_size);

        info!(
            "Created chunked reader: {} chunks, {} bytes total",
            total_chunks, total_size
        );

        Ok(Self {
            io: Box::new(io),
            buffer,
            current_index: 0,
            total_chunks,
            total_size,
            prefetch_semaphore: Arc::new(Semaphore::new(prefetch_count)),
            prefetch_count,
        })
    }

    /// Read the next chunk.
    pub async fn read_chunk(&mut self) -> Result<Option<Bytes>> {
        if self.current_index >= self.total_chunks {
            return Ok(None);
        }

        // Serve from the read-ahead buffer, but only when it actually holds the
        // chunk we are about to deliver.
        //
        // This used to be an unconditional `self.buffer.pop().await?`. On a
        // fresh reader the buffer is empty and not yet write-complete, which
        // `ChunkedBuffer::pop` reports as `Other("No chunks available")` — so
        // `?` turned "nothing prefetched yet" into a hard error and the direct
        // read below was unreachable. The very first `read_chunk()` on any
        // `ChunkedReader` therefore failed, always.
        if self
            .buffer
            .peek()
            .await?
            .is_some_and(|descriptor| descriptor.index == self.current_index)
            && let Some((_, data)) = self.buffer.pop().await?
        {
            self.current_index += 1;
            self.start_prefetch().await?;
            return Ok(Some(data));
        }

        // Read directly
        let descriptor = self
            .buffer
            .descriptor_for_index(self.current_index, self.total_size);
        let data = self.io.read_chunk(&descriptor).await?;

        self.current_index += 1;
        self.start_prefetch().await?;

        Ok(Some(data))
    }

    /// Fill the read-ahead buffer with a contiguous run of chunks starting at
    /// the next index the caller will ask for.
    ///
    /// Read-ahead only runs once the buffer has drained, so the buffered chunks
    /// are always contiguous and in stream order — which is what
    /// [`ChunkedBuffer::push`] requires. The previous version pushed from
    /// `current_index` *after* it had been advanced past a chunk that had been
    /// read directly (bypassing the buffer), so the buffer's write cursor was
    /// still at 0 while the pushed index was 1 and every prefetch failed with
    /// `InvalidOperation("Expected chunk 0, got 1")`.
    async fn start_prefetch(&mut self) -> Result<()> {
        // Refill only on a drained buffer; otherwise the chunks already queued
        // are still ahead of the caller and nothing needs doing.
        if !self.buffer.is_empty().await {
            return Ok(());
        }

        let start_index = self.current_index;
        let end_index = (start_index + self.prefetch_count).min(self.total_chunks);
        if start_index >= end_index {
            return Ok(());
        }

        // Direct reads bypass the buffer, so its write cursor lags the stream.
        // Re-base it now that the buffer is drained, or `push` will reject the
        // run below.
        if !self.buffer.rebase_if_empty(start_index).await {
            return Ok(());
        }

        for index in start_index..end_index {
            if self.prefetch_semaphore.available_permits() == 0 {
                break;
            }

            let descriptor = self.buffer.descriptor_for_index(index, self.total_size);

            // Prefetch this chunk
            let _permit = self
                .prefetch_semaphore
                .try_acquire()
                .map_err(|_| StreamingError::Other("Failed to acquire permit".to_string()))?;

            let data = self.io.read_chunk(&descriptor).await?;
            self.buffer.push(descriptor, data).await?;
        }

        Ok(())
    }

    /// Get the total number of chunks.
    pub fn total_chunks(&self) -> usize {
        self.total_chunks
    }

    /// Get the current chunk index.
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Check if there are more chunks to read.
    pub fn has_more(&self) -> bool {
        self.current_index < self.total_chunks
    }

    /// Get progress percentage.
    pub fn progress(&self) -> f64 {
        if self.total_chunks == 0 {
            100.0
        } else {
            (self.current_index as f64 / self.total_chunks as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

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
            Self(env::temp_dir().join(format!(
                "oxigeo_streaming_io_reader_{}_{seq}_{name}",
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

    #[tokio::test]
    async fn test_chunked_reader() {
        let test_path = TempPath::new("chunked_read.dat");

        // Create a 10 KiB test file of known content.
        let mut f = File::create(&test_path)
            .await
            .expect("fixture file should be creatable");
        let data = vec![42u8; 10240];
        f.write_all(&data)
            .await
            .expect("fixture should be writable");
        f.flush().await.expect("fixture should flush");
        drop(f);

        // 10240 bytes at 1024 bytes per chunk is exactly 10 chunks.
        let reader = ChunkedReader::from_file(&test_path, ChunkStrategy::FixedSize(1024), 10240, 2)
            .await
            .expect("a 10 KiB readable file must open as a ChunkedReader");

        let mut reader = reader;
        assert_eq!(
            reader.total_chunks(),
            10,
            "10240 bytes at 1024 bytes/chunk is 10 chunks"
        );
        assert_eq!(reader.current_index(), 0, "nothing read yet");
        assert!(reader.has_more(), "a fresh reader has chunks pending");

        // Drain the reader and check both the framing and the bytes.
        let mut chunks = 0usize;
        let mut total = 0usize;
        while let Some(chunk) = reader
            .read_chunk()
            .await
            .expect("reading a well-formed chunk must succeed")
        {
            assert_eq!(
                chunk.len(),
                1024,
                "chunk {chunks} must be a full 1024 bytes"
            );
            assert!(
                chunk.iter().all(|&b| b == 42),
                "chunk {chunks} must return the bytes that were written"
            );
            chunks += 1;
            total += chunk.len();
        }

        assert_eq!(chunks, 10, "the reader must yield every chunk exactly once");
        assert_eq!(total, 10240, "the reader must yield every byte of the file");
        assert!(!reader.has_more(), "a drained reader has nothing pending");
        assert!(
            (reader.progress() - 100.0).abs() < 1e-9,
            "a drained reader must report 100% progress, got {}",
            reader.progress()
        );
    }
}
