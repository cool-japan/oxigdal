//! Chunked writer for efficient sequential writing.

use super::buffer::{ChunkDescriptor, ChunkedBuffer};
use super::chunked::{ChunkStrategy, ChunkedIO, FileChunkedIO};
use crate::error::{Result, StreamingError};
use bytes::Bytes;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// A writer that processes data in chunks.
pub struct ChunkedWriter {
    /// The underlying chunked I/O
    io: Box<dyn ChunkedIO>,

    /// Chunk buffer (retained for future buffered-write support)
    #[allow(dead_code)]
    buffer: ChunkedBuffer,

    /// Current chunk index (equals the number of chunks written so far)
    current_index: usize,

    /// Total size written
    bytes_written: u64,

    /// Write semaphore
    write_semaphore: Arc<Semaphore>,

    /// Chunk strategy (retained for future adaptive-size support)
    #[allow(dead_code)]
    strategy: ChunkStrategy,

    /// Optional pre-declared total chunk count.
    ///
    /// When set, [`finalize`] checks that exactly this many chunks were written
    /// and returns [`StreamingError::IncompleteFinalize`] on mismatch.
    preset_total_chunks: Option<usize>,
}

impl ChunkedWriter {
    /// Create a new chunked writer to a file.
    pub async fn from_file<P: AsRef<Path>>(
        path: P,
        strategy: ChunkStrategy,
        buffer_size: usize,
        max_concurrent_writes: usize,
    ) -> Result<Self> {
        let mut io = FileChunkedIO::new(path, strategy).await?;
        io.open_write().await?;

        let chunk_size = strategy.chunk_size_for_index(0, 0);
        let buffer = ChunkedBuffer::new(chunk_size, buffer_size);

        info!("Created chunked writer with chunk size {}", chunk_size);

        Ok(Self {
            io: Box::new(io),
            buffer,
            current_index: 0,
            bytes_written: 0,
            write_semaphore: Arc::new(Semaphore::new(max_concurrent_writes)),
            strategy,
            preset_total_chunks: None,
        })
    }

    /// Declare the expected total number of chunks up front.
    ///
    /// When `finalize` is called it verifies that exactly `n` chunks were
    /// written. A mismatch returns [`StreamingError::IncompleteFinalize`].
    pub fn set_total_chunks(mut self, n: usize) -> Self {
        self.preset_total_chunks = Some(n);
        self
    }

    /// Write a chunk of data.
    pub async fn write_chunk(&mut self, data: Bytes) -> Result<()> {
        let _permit = self
            .write_semaphore
            .acquire()
            .await
            .map_err(|e| StreamingError::Other(e.to_string()))?;

        let offset = self.bytes_written;
        let length = data.len();

        // `total_chunks` in the descriptor is set to 0 here because we may not
        // know the final count yet; `finalize` will write a footer or patch the
        // header with the real value.
        let descriptor = ChunkDescriptor::new(
            offset,
            length,
            self.current_index,
            0, // Updated when finalized (see finalize())
        );

        self.io.write_chunk(&descriptor, data).await?;

        self.current_index += 1;
        self.bytes_written += length as u64;

        debug!(
            "Wrote chunk {} ({} bytes), total: {} bytes",
            descriptor.index, length, self.bytes_written
        );

        Ok(())
    }

    /// Write multiple chunks sequentially.
    pub async fn write_chunks(&mut self, chunks: Vec<Bytes>) -> Result<()> {
        for chunk in chunks {
            self.write_chunk(chunk).await?;
        }
        Ok(())
    }

    /// Flush all pending writes.
    pub async fn flush(&mut self) -> Result<()> {
        self.io.flush().await?;
        info!(
            "Flushed {} bytes in {} chunks",
            self.bytes_written, self.current_index
        );
        Ok(())
    }

    /// Finalize the writer, validate chunk counts, and close the stream.
    ///
    /// If [`set_total_chunks`] was called previously and the actual number of
    /// chunks written does not match the preset value,
    /// [`StreamingError::IncompleteFinalize`] is returned *before* the
    /// underlying I/O is closed so callers can inspect the partial output.
    ///
    /// After a successful finalize a footer descriptor containing the real
    /// `total_chunks` value is appended so readers can discover the count
    /// without having to re-scan the file.
    ///
    /// [`set_total_chunks`]: ChunkedWriter::set_total_chunks
    pub async fn finalize(mut self) -> Result<()> {
        self.flush().await?;

        let total_chunks = self.current_index;

        // Validate against the preset (if any)
        if let Some(expected) = self.preset_total_chunks
            && total_chunks != expected
        {
            warn!(
                "Finalize mismatch: expected {} chunks, wrote {}",
                expected, total_chunks
            );
            return Err(StreamingError::IncompleteFinalize {
                expected,
                actual: total_chunks,
            });
        }

        // Write a footer: a small sentinel chunk whose ChunkDescriptor carries
        // the definitive `total_chunks` value.  This allows readers that cannot
        // seek (e.g. network streams) to discover the count at the end.
        //
        // The footer payload is a little-endian u64 encoding of `total_chunks`
        // preceded by the magic bytes `b"CHNK"` (4 bytes) so it is identifiable.
        let mut footer = Vec::with_capacity(12);
        footer.extend_from_slice(b"CHNK");
        footer.extend_from_slice(&(total_chunks as u64).to_le_bytes());

        let footer_descriptor = ChunkDescriptor::new(
            self.bytes_written,
            footer.len(),
            total_chunks, // footer index = total_chunks (one past the last data chunk)
            total_chunks + 1, // total including the footer itself
        );

        self.io
            .write_chunk(&footer_descriptor, Bytes::from(footer))
            .await?;
        self.io.flush().await?;

        info!(
            "Finalized chunked writer: {} data chunks, {} total bytes",
            total_chunks, self.bytes_written
        );

        Ok(())
    }

    /// Get the number of chunks written so far.
    pub fn chunks_written(&self) -> usize {
        self.current_index
    }

    /// Get the total bytes written so far.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_chunked_writer() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_chunked_write.dat");

        let result =
            ChunkedWriter::from_file(&test_path, ChunkStrategy::FixedSize(1024), 10240, 4).await;

        if let Ok(mut writer) = result {
            let data = Bytes::from(vec![42u8; 1024]);
            writer.write_chunk(data).await.ok();
            writer.finalize().await.ok();
        }

        // Clean up
        tokio::fs::remove_file(&test_path).await.ok();
    }

    /// Verify that `finalize` writes the footer and reports the correct total.
    #[tokio::test]
    async fn test_chunked_writer_finalize_patches_total_chunks() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_cw_finalize_total.dat");
        // Clean up before test to avoid stale state
        tokio::fs::remove_file(&test_path).await.ok();

        let mut writer =
            ChunkedWriter::from_file(&test_path, ChunkStrategy::FixedSize(512), 10240, 2)
                .await
                .expect("writer should be created successfully");

        for i in 0u8..5 {
            let data = Bytes::from(vec![i; 512]);
            writer
                .write_chunk(data)
                .await
                .expect("write_chunk should succeed");
        }

        assert_eq!(writer.chunks_written(), 5);

        writer.finalize().await.expect("finalize should succeed");

        // The footer should have been written; verify the file is larger than 5 * 512
        let meta = tokio::fs::metadata(&test_path)
            .await
            .expect("file metadata should be readable");
        assert!(meta.len() > 5 * 512, "footer was not written");

        tokio::fs::remove_file(&test_path).await.ok();
    }

    /// When `set_total_chunks` matches the actual count, `finalize` succeeds.
    #[tokio::test]
    async fn test_chunked_writer_with_preset_chunks_matches_on_finalize() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_cw_preset_match.dat");
        tokio::fs::remove_file(&test_path).await.ok();

        let mut writer =
            ChunkedWriter::from_file(&test_path, ChunkStrategy::FixedSize(256), 10240, 2)
                .await
                .expect("writer should be created successfully");
        // Pre-declare 3 chunks
        writer = writer.set_total_chunks(3);

        for i in 0u8..3 {
            writer
                .write_chunk(Bytes::from(vec![i; 256]))
                .await
                .expect("write_chunk should succeed");
        }

        writer
            .finalize()
            .await
            .expect("finalize should succeed when preset matches actual");

        tokio::fs::remove_file(&test_path).await.ok();
    }

    /// When `set_total_chunks` does NOT match the actual count, `finalize` errors.
    #[tokio::test]
    async fn test_chunked_writer_preset_mismatch_returns_error() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_cw_preset_mismatch.dat");
        tokio::fs::remove_file(&test_path).await.ok();

        let mut writer =
            ChunkedWriter::from_file(&test_path, ChunkStrategy::FixedSize(256), 10240, 2)
                .await
                .expect("writer should be created successfully");
        // Pre-declare 5 chunks but only write 3
        writer = writer.set_total_chunks(5);

        for i in 0u8..3 {
            writer
                .write_chunk(Bytes::from(vec![i; 256]))
                .await
                .expect("write_chunk should succeed");
        }

        let result = writer.finalize().await;
        match result {
            Err(StreamingError::IncompleteFinalize { expected, actual }) => {
                assert_eq!(expected, 5, "expected preset value");
                assert_eq!(actual, 3, "actual written count");
            }
            other => panic!("expected IncompleteFinalize, got {:?}", other),
        }

        tokio::fs::remove_file(&test_path).await.ok();
    }
}
