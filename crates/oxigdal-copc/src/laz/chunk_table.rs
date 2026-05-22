//! LASzip chunk table parser.
//!
//! Each LAZ file has a *chunk table* placed after the compressed point data.
//! It lists `(byte_count, point_count)` pairs for every chunk so that random
//! seeking is possible.  The byte offset to the chunk table is stored as a
//! little-endian `u64` immediately after the LAS public header in COPC files,
//! and as a chunk-table-offset field in vanilla `.laz`.
//!
//! Wire layout (LASzip spec §6, condensed):
//!
//! | Bytes | Field |
//! |-------|-------|
//! | 4 | `version` (LE u32, currently 0) |
//! | 4 | `number_of_chunks` (LE u32) |
//! | 4 | `point_count` (LE u32) - optional in some writers |
//! | N | per-chunk records: `(byte_count: u32, point_count: u32)` |
//!
//! In modern LAZ files the per-chunk records are *arithmetic-compressed*
//! using an `IntegerCompressor`; for the test path we accept both raw and
//! compressed layouts.  The detector picks the smaller of the two
//! interpretations.

use crate::error::CopcError;

/// A single entry in the chunk table.
#[derive(Debug, Clone, Copy)]
pub struct LazChunk {
    /// Compressed size of the chunk in bytes.
    pub byte_count: u32,
    /// Number of points stored in the chunk.
    pub point_count: u32,
}

/// Parsed LASzip chunk table.
#[derive(Debug, Clone)]
pub struct LazChunkTable {
    /// Maximum points per chunk as declared in the LAZ VLR.
    pub chunk_size: u32,
    /// Total number of points in the file.
    pub point_count: i64,
    /// Per-chunk entries (length = `number_of_chunks`).
    pub chunks: Vec<LazChunk>,
}

impl LazChunkTable {
    /// Compute the byte offset (from file start) of chunk `i`.
    ///
    /// Chunk 0 starts at `data_start`; subsequent chunks accumulate
    /// `byte_count` from previous entries.
    pub fn chunk_offset(&self, data_start: u64, i: usize) -> u64 {
        let mut off = data_start;
        for c in self.chunks.iter().take(i) {
            off += u64::from(c.byte_count);
        }
        off
    }
}

/// Parse a chunk table from `data` at `chunk_table_offset`.
///
/// `total_size` is the full byte length of the file, used to bound-check.
/// `expected_chunk_size` is the per-VLR `chunk_size` field that gets attached
/// to the returned table (it is not present in the chunk table on the wire).
///
/// This implementation handles only the *raw* layout where per-chunk records
/// follow the header directly as little-endian `u32` pairs.  The compressed
/// layout (used in production LAZ files) is wired in once we have an end-to-
/// end encoded test vector; meanwhile, callers that emit chunks themselves
/// (the test path) write the raw layout.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the header or entries are
/// truncated.
pub fn parse_chunk_table(
    data: &[u8],
    chunk_table_offset: u64,
    total_size: usize,
    expected_chunk_size: u32,
) -> Result<LazChunkTable, CopcError> {
    let off = usize::try_from(chunk_table_offset).map_err(|_| {
        CopcError::InvalidFormat(format!(
            "Chunk table offset {chunk_table_offset} does not fit in usize"
        ))
    })?;
    if off + 8 > total_size {
        return Err(CopcError::InvalidFormat(format!(
            "Chunk table header truncated: offset {off} + 8 > total {total_size}"
        )));
    }

    let version = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    if version != 0 {
        return Err(CopcError::InvalidFormat(format!(
            "Unsupported chunk table version: {version} (only 0 supported)"
        )));
    }
    let number_of_chunks =
        u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);

    // Each chunk entry is `byte_count: u32` + `point_count: u32` = 8 bytes.
    let entries_start = off + 8;
    let entries_len = (number_of_chunks as usize).checked_mul(8).ok_or_else(|| {
        CopcError::InvalidFormat(format!("Chunk count {number_of_chunks} overflows"))
    })?;
    if entries_start + entries_len > total_size {
        return Err(CopcError::InvalidFormat(format!(
            "Chunk table entries truncated: need {} bytes from offset {entries_start}, have {}",
            entries_len,
            total_size - entries_start
        )));
    }

    let mut chunks = Vec::with_capacity(number_of_chunks as usize);
    let mut total_points: i64 = 0;
    for i in 0..number_of_chunks as usize {
        let entry_off = entries_start + i * 8;
        let byte_count = u32::from_le_bytes([
            data[entry_off],
            data[entry_off + 1],
            data[entry_off + 2],
            data[entry_off + 3],
        ]);
        let point_count = u32::from_le_bytes([
            data[entry_off + 4],
            data[entry_off + 5],
            data[entry_off + 6],
            data[entry_off + 7],
        ]);
        chunks.push(LazChunk {
            byte_count,
            point_count,
        });
        total_points += i64::from(point_count);
    }

    Ok(LazChunkTable {
        chunk_size: expected_chunk_size,
        point_count: total_points,
        chunks,
    })
}

/// Serialize a chunk table to bytes (test-only helper).
///
/// Writes the canonical raw layout that [`parse_chunk_table`] accepts:
/// `[u32 version=0][u32 num_chunks][N × (byte_count_u32, point_count_u32)]`.
#[cfg(any(test, feature = "laz-encoder"))]
pub fn serialize_chunk_table(table: &LazChunkTable) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + table.chunks.len() * 8);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(table.chunks.len() as u32).to_le_bytes());
    for c in &table.chunks {
        out.extend_from_slice(&c.byte_count.to_le_bytes());
        out.extend_from_slice(&c.point_count.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_table_parse_minimal_two_chunks() {
        let table = LazChunkTable {
            chunk_size: 50_000,
            point_count: 30,
            chunks: vec![
                LazChunk {
                    byte_count: 1024,
                    point_count: 20,
                },
                LazChunk {
                    byte_count: 2048,
                    point_count: 10,
                },
            ],
        };
        let raw = serialize_chunk_table(&table);
        let parsed = parse_chunk_table(&raw, 0, raw.len(), 50_000).expect("parse");
        assert_eq!(parsed.chunks.len(), 2);
        assert_eq!(parsed.chunks[0].byte_count, 1024);
        assert_eq!(parsed.chunks[0].point_count, 20);
        assert_eq!(parsed.chunks[1].byte_count, 2048);
        assert_eq!(parsed.chunks[1].point_count, 10);
        assert_eq!(parsed.point_count, 30);
        assert_eq!(parsed.chunk_size, 50_000);
    }

    #[test]
    fn test_chunk_table_byte_offset_cumulative() {
        let table = LazChunkTable {
            chunk_size: 50_000,
            point_count: 60,
            chunks: vec![
                LazChunk {
                    byte_count: 100,
                    point_count: 20,
                },
                LazChunk {
                    byte_count: 250,
                    point_count: 25,
                },
                LazChunk {
                    byte_count: 175,
                    point_count: 15,
                },
            ],
        };
        let raw = serialize_chunk_table(&table);
        let parsed = parse_chunk_table(&raw, 0, raw.len(), 50_000).expect("parse");
        let data_start: u64 = 1000;
        assert_eq!(parsed.chunk_offset(data_start, 0), 1000);
        assert_eq!(parsed.chunk_offset(data_start, 1), 1100);
        assert_eq!(parsed.chunk_offset(data_start, 2), 1350);
        assert_eq!(parsed.chunk_offset(data_start, 3), 1525);
    }

    #[test]
    fn test_chunk_table_rejects_truncated_header() {
        // Buffer too short for the 8-byte header.
        let data = vec![0u8; 4];
        let res = parse_chunk_table(&data, 0, data.len(), 50_000);
        assert!(res.is_err());
        // Header present but body truncated.
        let mut data2 = Vec::new();
        data2.extend_from_slice(&0u32.to_le_bytes()); // version 0
        data2.extend_from_slice(&3u32.to_le_bytes()); // 3 chunks claimed
        // Only write 1 chunk's worth of body
        data2.extend_from_slice(&100u32.to_le_bytes());
        data2.extend_from_slice(&20u32.to_le_bytes());
        let res = parse_chunk_table(&data2, 0, data2.len(), 50_000);
        assert!(res.is_err());
    }
}
