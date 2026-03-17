//! Zero-copy Arrow IPC framing for inter-process communication.
//!
//! Arrow IPC format (<https://arrow.apache.org/docs/format/IPC.html>):
//! - **File format**: magic + schema + record batches + footer
//! - **Stream format**: schema + record batches (no footer)
//!
//! This module implements the framing layer (message headers) to allow
//! zero-copy deserialization.  Arrow arrays are described by offset+length
//! pairs into the backing buffer, which enables zero-copy reads when the
//! buffer is memory-mapped.

use crate::error::StreamingError;

/// Arrow IPC file magic bytes.
pub const ARROW_MAGIC: &[u8] = b"ARROW1";
/// Length of the magic sequence.
pub const ARROW_MAGIC_LEN: usize = 6;
/// Required alignment for Arrow IPC message bodies (bytes).
pub const ARROW_ALIGNMENT: usize = 8;

// ── Message types ─────────────────────────────────────────────────────────────

/// Discriminant for Arrow IPC message payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcMessageType {
    /// Schema message.
    Schema,
    /// Dictionary replacement batch.
    DictionaryBatch,
    /// Record batch.
    RecordBatch,
    /// Dense tensor.
    Tensor,
    /// Sparse tensor.
    SparseTensor,
}

// ── Header & metadata structures ─────────────────────────────────────────────

/// Parsed Arrow IPC message header (metadata only; body data is not copied).
#[derive(Debug, Clone)]
pub struct IpcMessageHeader {
    /// Payload type discriminant.
    pub message_type: IpcMessageType,
    /// Length of the flatbuffer metadata section, in bytes.
    pub metadata_length: i32,
    /// Length of the binary body section, in bytes.
    pub body_length: i64,
    /// Absolute byte offset in the source buffer where the body starts.
    pub body_offset: u64,
}

/// Arrow IPC buffer descriptor (offset + length within the message body).
#[derive(Debug, Clone)]
pub struct IpcBuffer {
    /// Byte offset from the start of the message body.
    pub offset: i64,
    /// Number of bytes in this buffer.
    pub length: i64,
}

/// Arrow IPC record batch metadata (no heap copies of array data).
#[derive(Debug, Clone)]
pub struct IpcRecordBatch {
    /// Number of rows in this batch.
    pub length: i64,
    /// Per-column field node metadata.
    pub nodes: Vec<IpcFieldNode>,
    /// Buffer descriptors for all column buffers.
    pub buffers: Vec<IpcBuffer>,
}

/// Metadata for one Arrow column node.
#[derive(Debug, Clone)]
pub struct IpcFieldNode {
    /// Number of logical values in the column.
    pub length: i64,
    /// Number of null values.
    pub null_count: i64,
}

// ── Reader ────────────────────────────────────────────────────────────────────

/// Cursor-based Arrow IPC message reader.
pub struct ArrowIpcReader {
    data: Vec<u8>,
    offset: usize,
}

impl ArrowIpcReader {
    /// Creates a new reader wrapping `data`.
    #[must_use]
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, offset: 0 }
    }

    /// Returns `true` if `data` starts with the Arrow IPC magic bytes.
    #[must_use]
    pub fn is_arrow_file(&self) -> bool {
        self.data.len() >= ARROW_MAGIC_LEN && self.data.starts_with(ARROW_MAGIC)
    }

    fn read_i32(&self, offset: usize) -> Option<i32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_i64(&self, offset: usize) -> Option<i64> {
        let bytes = self.data.get(offset..offset + 8)?;
        Some(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_u32(&self, offset: usize) -> Option<u32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Validates and advances past the file header (magic + 2-byte padding).
    ///
    /// # Errors
    /// Returns an error if the buffer does not start with the Arrow magic bytes.
    pub fn parse_file_header(&mut self) -> Result<(), StreamingError> {
        if !self.is_arrow_file() {
            return Err(StreamingError::Other("Not an Arrow IPC file".into()));
        }
        // 6-byte magic + 2-byte padding
        self.offset = ARROW_MAGIC_LEN + 2;
        Ok(())
    }

    /// Reads and returns the next IPC message header, or `Ok(None)` at EOS.
    ///
    /// # Errors
    /// Returns an error if the buffer is truncated mid-header.
    pub fn next_message(&mut self) -> Result<Option<IpcMessageHeader>, StreamingError> {
        if self.offset + 4 > self.data.len() {
            return Ok(None);
        }

        // Optional continuation marker (0xFFFFFFFF).
        if let Some(cont) = self.read_u32(self.offset) {
            if cont == 0xFFFF_FFFF {
                self.offset += 4;
            }
        }

        // Metadata length (i32, LE).  Zero means EOS.
        let metadata_length = self
            .read_i32(self.offset)
            .ok_or_else(|| StreamingError::Other("Truncated metadata length".into()))?;

        if metadata_length <= 0 {
            return Ok(None);
        }
        self.offset += 4;

        // Infer the message type from the flatbuffer union tag at byte offset 4
        // inside the flatbuffer (the union type field).
        let meta_end = self.offset + metadata_length as usize;
        let msg_type = if meta_end <= self.data.len() && metadata_length >= 8 {
            match self.data.get(self.offset + 4).copied().unwrap_or(0) {
                1 => IpcMessageType::Schema,
                2 => IpcMessageType::DictionaryBatch,
                3 => IpcMessageType::RecordBatch,
                4 => IpcMessageType::Tensor,
                5 => IpcMessageType::SparseTensor,
                _ => IpcMessageType::RecordBatch,
            }
        } else {
            IpcMessageType::RecordBatch
        };

        // Advance past metadata (aligned).
        let aligned_meta = align_to(metadata_length as usize, ARROW_ALIGNMENT);
        self.offset += aligned_meta;

        // Body length (i64, LE) follows aligned metadata.
        let body_length = self.read_i64(self.offset).unwrap_or(0);
        self.offset += 8;

        let body_offset = self.offset as u64;

        // Advance past body (aligned).
        let aligned_body = align_to(body_length as usize, ARROW_ALIGNMENT);
        self.offset += aligned_body;

        Ok(Some(IpcMessageHeader {
            message_type: msg_type,
            metadata_length,
            body_length,
            body_offset,
        }))
    }

    /// Returns the slice for an [`IpcBuffer`] relative to `body_offset`.
    ///
    /// Returns `None` if the range falls outside the backing buffer.
    #[must_use]
    pub fn read_buffer<'a>(&'a self, body_offset: u64, buf: &IpcBuffer) -> Option<&'a [u8]> {
        let start = (body_offset as usize).checked_add(buf.offset as usize)?;
        let end = start.checked_add(buf.length as usize)?;
        self.data.get(start..end)
    }

    /// Returns the total length of the backing buffer.
    #[must_use]
    pub fn data_len(&self) -> usize {
        self.data.len()
    }

    /// Returns the current read cursor position.
    #[must_use]
    pub fn current_offset(&self) -> usize {
        self.offset
    }
}

// ── Writer ────────────────────────────────────────────────────────────────────

/// Framing-layer Arrow IPC writer.  Serialises message headers and bodies
/// without depending on the full Arrow crate serialiser.
pub struct ArrowIpcWriter {
    buf: Vec<u8>,
}

impl ArrowIpcWriter {
    /// Creates a new writer and writes the Arrow file magic header.
    #[must_use]
    pub fn new() -> Self {
        let mut w = Self { buf: Vec::new() };
        w.buf.extend_from_slice(ARROW_MAGIC);
        w.buf.extend_from_slice(&[0u8; 2]); // 2-byte padding
        w
    }

    /// Appends a framed IPC message (metadata + body) to the internal buffer.
    pub fn write_message(&mut self, metadata: &[u8], body: &[u8]) {
        // Continuation token.
        self.buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        // Metadata length.
        self.buf
            .extend_from_slice(&(metadata.len() as i32).to_le_bytes());
        // Metadata + alignment padding.
        self.buf.extend_from_slice(metadata);
        let meta_pad = align_to(metadata.len(), ARROW_ALIGNMENT) - metadata.len();
        self.buf.resize(self.buf.len() + meta_pad, 0u8);
        // Body length.
        self.buf
            .extend_from_slice(&(body.len() as i64).to_le_bytes());
        // Body + alignment padding.
        self.buf.extend_from_slice(body);
        let body_pad = align_to(body.len(), ARROW_ALIGNMENT) - body.len();
        self.buf.resize(self.buf.len() + body_pad, 0u8);
    }

    /// Writes the EOS marker and trailing magic, then returns the finished buffer.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        // EOS: continuation + zero metadata length.
        self.buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        self.buf.extend_from_slice(&0i32.to_le_bytes());
        // Trailing magic.
        self.buf.extend_from_slice(ARROW_MAGIC);
        self.buf
    }
}

impl Default for ArrowIpcWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Rounds `size` up to the next multiple of `alignment`.
#[must_use]
pub fn align_to(size: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return size;
    }
    (size + alignment - 1) & !(alignment - 1)
}
