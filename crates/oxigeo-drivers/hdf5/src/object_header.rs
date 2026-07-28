//! Minimal, honest HDF5 Object Header (version 1) message walker.
//!
//! `oxih5`'s decoded [`oxih5::Dataset`] view (returned by `oxih5::File::dataset`)
//! already applies filters and hands back plain element bytes, but it does not
//! expose the *metadata* a caller needs to drive [`crate::reader::Hdf5Reader::decode_chunk`]
//! directly on raw, still-filtered chunk bytes: the Data Layout message's chunk
//! dimensions (message type `0x0008`) and the Filter Pipeline message's filter
//! list (message type `0x000B`).
//!
//! This module walks a dataset's real object header — starting from the
//! object header address `oxih5::File::header_addr_of` returns — to recover
//! those two messages directly from the file bytes, using the reader's own
//! open file handle (no dependency on `oxih5` internals).
//!
//! # Scope
//!
//! Only **version 1** object headers (the classic pre-1.8 on-disk format,
//! which is also what `oxih5`'s own [`oxih5::FileWriter`] emits) are parsed.
//! Version 2 (`OHDR`-signed) object headers are recognized and skipped
//! gracefully — this walker returns an empty [`RawDatasetLayout`] rather than
//! an error, since the reader treats "no layout metadata" as "contiguous, no
//! filters", exactly as it already does today. Likewise, Data Layout message
//! versions other than 1/2/3 (i.e. the HDF5 1.10 "version 4" layout with
//! Fixed/Extensible/BTree-v2 chunk indexing) are recognized-but-unsupported
//! and skipped the same way. Nothing here ever fabricates chunk dimensions or
//! filter parameters — a message that can't be parsed simply leaves the
//! corresponding field `None`.

use crate::error::Result;
use crate::filters::FilterPipeline;
use std::io::{Read, Seek, SeekFrom};

/// Object Header message type: Data Layout.
const MSG_DATA_LAYOUT: u16 = 0x0008;
/// Object Header message type: Filter Pipeline.
const MSG_FILTER_PIPELINE: u16 = 0x000B;
/// Object Header message type: Continuation (points to more messages
/// elsewhere in the file).
const MSG_CONTINUATION: u16 = 0x0010;

/// Chunk dimensions and/or filter pipeline recovered directly from a
/// dataset's real object header.
#[derive(Debug, Default, Clone)]
pub(crate) struct RawDatasetLayout {
    /// Per-dimension chunk size (one entry per real dataset dimension, i.e.
    /// *not* including the trailing element-size slot the on-disk chunked
    /// Data Layout message itself carries), if the dataset uses a chunked
    /// layout and the message could be parsed.
    pub(crate) chunk_dims: Option<Vec<usize>>,
    /// The dataset's parsed Filter Pipeline message, if present and parsed.
    pub(crate) filter_pipeline: Option<FilterPipeline>,
}

/// Read the Data Layout and Filter Pipeline messages from the real object
/// header at `header_addr`.
///
/// `size_of_offsets` / `size_of_lengths` come from the file's superblock
/// (already parsed by [`crate::reader::Superblock`]) and size the address /
/// length fields inside Continuation and chunked Data Layout messages.
///
/// Never fails the caller's dataset open: any I/O error, unsupported object
/// header version, or malformed message simply yields a default (empty)
/// [`RawDatasetLayout`], matching the reader's existing "not chunked, no
/// filters" fallback behavior.
pub(crate) fn read_dataset_layout<R: Read + Seek>(
    reader: &mut R,
    header_addr: u64,
    size_of_offsets: u8,
    size_of_lengths: u8,
) -> Result<RawDatasetLayout> {
    let mut layout = RawDatasetLayout::default();

    if reader.seek(SeekFrom::Start(header_addr)).is_err() {
        return Ok(layout);
    }
    let mut prefix4 = [0u8; 4];
    if reader.read_exact(&mut prefix4).is_err() {
        return Ok(layout);
    }
    if &prefix4 == b"OHDR" {
        // Version 2 object header: not parsed by this focused walker.
        return Ok(layout);
    }

    let version = prefix4[0];
    if version != 1 {
        // Unknown/corrupt prefix — leave layout empty rather than guessing.
        return Ok(layout);
    }

    // Remaining V1 prefix fields: ref_count(4) + header_size(4) + padding(4).
    let mut rest = [0u8; 12];
    if reader.read_exact(&mut rest).is_err() {
        return Ok(layout);
    }
    let header_size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as u64;

    // Work queue of (start, length) message regions: the initial header-local
    // chunk, plus any Continuation-block regions discovered while walking it.
    let mut regions: Vec<(u64, u64)> = vec![(header_addr + 16, header_size)];
    // Defensive cap: a well-formed file has at most a handful of continuation
    // blocks per object; refuse to chase an unbounded/cyclic chain.
    let mut regions_visited = 0usize;
    const MAX_REGIONS: usize = 256;

    while let Some((start, len)) = regions.pop() {
        regions_visited += 1;
        if regions_visited > MAX_REGIONS {
            break;
        }
        walk_v1_message_region(
            reader,
            start,
            len,
            size_of_offsets,
            size_of_lengths,
            &mut layout,
            &mut regions,
        );
    }

    Ok(layout)
}

/// Walk one contiguous region of V1 object header messages (either the
/// header's own message area, or a Continuation block), updating `layout`
/// and pushing any further Continuation regions onto `regions`.
///
/// Best-effort: any I/O error or malformed message header simply stops the
/// walk over this region early (bytes already recovered stay in `layout`).
#[allow(clippy::too_many_arguments)]
fn walk_v1_message_region<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    len: u64,
    size_of_offsets: u8,
    size_of_lengths: u8,
    layout: &mut RawDatasetLayout,
    regions: &mut Vec<(u64, u64)>,
) {
    let end = match start.checked_add(len) {
        Some(e) => e,
        None => return,
    };
    let mut pos = start;

    while pos + 8 <= end {
        if reader.seek(SeekFrom::Start(pos)).is_err() {
            return;
        }
        let mut hdr = [0u8; 8];
        if reader.read_exact(&mut hdr).is_err() {
            return;
        }
        let msg_type = u16::from_le_bytes([hdr[0], hdr[1]]);
        let body_size = u16::from_le_bytes([hdr[2], hdr[3]]) as u64;
        let body_start = pos + 8;
        let body_end = match body_start.checked_add(body_size) {
            Some(e) => e,
            None => return,
        };
        if body_end > end {
            return; // truncated / malformed — stop, keep what we already found
        }

        match msg_type {
            MSG_DATA_LAYOUT => {
                if let Some(body) = read_body(reader, body_start, body_size)
                    && let Some(dims) = parse_layout_message(&body, size_of_offsets)
                {
                    layout.chunk_dims = Some(dims);
                }
            }
            MSG_FILTER_PIPELINE => {
                if let Some(body) = read_body(reader, body_start, body_size)
                    && let Ok(pipeline) = FilterPipeline::from_message_bytes(&body)
                {
                    layout.filter_pipeline = Some(pipeline);
                }
            }
            MSG_CONTINUATION => {
                if let Some(body) = read_body(reader, body_start, body_size)
                    && let Some((addr, cont_len)) =
                        parse_continuation(&body, size_of_offsets, size_of_lengths)
                {
                    regions.push((addr, cont_len));
                }
            }
            _ => {}
        }

        pos = body_end;
    }
}

/// Read exactly `size` bytes starting at `start`, or `None` on any I/O error.
///
/// `size` originates from an untrusted object-header message length, so it is
/// bounded against the bytes actually remaining in the file before the buffer
/// is allocated: a malformed header claiming a multi-gigabyte body yields
/// `None` here rather than a huge speculative allocation (OOM / DoS).
fn read_body<R: Read + Seek>(reader: &mut R, start: u64, size: u64) -> Option<Vec<u8>> {
    reader.seek(SeekFrom::Start(start)).ok()?;
    let end = reader.seek(SeekFrom::End(0)).ok()?;
    reader.seek(SeekFrom::Start(start)).ok()?;
    if size > end.saturating_sub(start) {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    reader.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Parse a Data Layout message body (message type `0x0008`) of version 1, 2,
/// or 3 into per-dimension chunk sizes, when the message describes a
/// **chunked** layout (layout class 2). Contiguous/compact layouts, and
/// message versions this walker doesn't support (namely version 4's
/// Fixed/Extensible/BTree-v2 chunk indexing), return `None`.
fn parse_layout_message(body: &[u8], size_of_offsets: u8) -> Option<Vec<usize>> {
    let version = *body.first()?;
    match version {
        1 | 2 => parse_layout_v1_v2(body, size_of_offsets),
        3 => parse_layout_v3(body, size_of_offsets),
        _ => None,
    }
}

/// Version 1/2 Data Layout message body:
/// `version(1) dimensionality(1) layout_class(1) reserved(5) [address](size_of_offsets, only when class != compact) dims[dimensionality](u32 each)`.
/// For a chunked layout the last "dimension" entry is the element size, not a
/// real dataset dimension.
fn parse_layout_v1_v2(body: &[u8], size_of_offsets: u8) -> Option<Vec<usize>> {
    if body.len() < 8 {
        return None;
    }
    let dimensionality = body[1] as usize;
    let layout_class = body[2];
    if layout_class != 2 {
        return None; // only chunked (class 2) carries useful chunk dims here
    }
    if dimensionality == 0 {
        return None;
    }
    let addr_size = size_of_offsets as usize;
    let dims_start = 8usize.checked_add(addr_size)?;
    // The last of `dimensionality` u32 entries is the element size, not a
    // real dataset dimension.
    let ndims = dimensionality.checked_sub(1)?;
    read_u32_dims(body, dims_start, ndims)
}

/// Version 3 Data Layout message body:
/// `version(1) class(1) dimensionality(1) address(size_of_offsets) dims[dimensionality](u32 each)`
/// (chunked, class 2) — the last of the `dimensionality` u32 entries is the
/// element size, not a real dataset dimension.
fn parse_layout_v3(body: &[u8], size_of_offsets: u8) -> Option<Vec<usize>> {
    if body.len() < 3 {
        return None;
    }
    let class = body[1];
    if class != 2 {
        return None; // contiguous (1) / compact (0): no chunk dims to recover
    }
    let dimensionality = body[2] as usize;
    if dimensionality == 0 {
        return None;
    }
    let addr_size = size_of_offsets as usize;
    let dims_start = 3usize.checked_add(addr_size)?;
    let ndims = dimensionality.checked_sub(1)?;
    read_u32_dims(body, dims_start, ndims)
}

/// Read `ndims` consecutive little-endian `u32` values starting at
/// `dims_start` in `body`, returning `None` if the body is too short.
fn read_u32_dims(body: &[u8], dims_start: usize, ndims: usize) -> Option<Vec<usize>> {
    let mut dims = Vec::with_capacity(ndims);
    for i in 0..ndims {
        let off = dims_start.checked_add(i.checked_mul(4)?)?;
        let word = body.get(off..off.checked_add(4)?)?;
        dims.push(u32::from_le_bytes([word[0], word[1], word[2], word[3]]) as usize);
    }
    Some(dims)
}

/// Parse a Continuation message body (`offset(size_of_offsets)` +
/// `length(size_of_lengths)`) into an absolute file address and byte length.
fn parse_continuation(body: &[u8], size_of_offsets: u8, size_of_lengths: u8) -> Option<(u64, u64)> {
    let off_size = size_of_offsets as usize;
    let len_size = size_of_lengths as usize;
    let addr = read_uint_le(body.get(0..off_size)?);
    let len = read_uint_le(body.get(off_size..off_size.checked_add(len_size)?)?);
    Some((addr, len))
}

/// Decode a little-endian unsigned integer of 1-8 bytes.
fn read_uint_le(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// `read_body` must refuse a message length that exceeds the bytes left in
    /// the file rather than speculatively allocating from an untrusted header.
    #[test]
    fn read_body_rejects_size_beyond_stream() {
        let mut cur = Cursor::new(vec![0u8; 32]);
        // Within bounds: 16 bytes from offset 8 (24 <= 32) succeeds.
        assert_eq!(read_body(&mut cur, 8, 16).map(|b| b.len()), Some(16));
        // Out of bounds: a header claiming ~4 GB from offset 8 yields None
        // without attempting the allocation.
        assert!(read_body(&mut cur, 8, 4_000_000_000).is_none());
        // Exactly at the end is fine; one past is not.
        assert!(read_body(&mut cur, 0, 32).is_some());
        assert!(read_body(&mut cur, 0, 33).is_none());
    }

    /// Hand-build a minimal V1 object header containing a single version-3
    /// chunked Data Layout message (2 real dims + element-size slot, 8-byte
    /// offsets) and confirm the chunk dims round-trip.
    #[test]
    fn parses_v1_header_with_v3_chunked_layout() {
        // Data Layout v3 body: version(1) class(1) dim(1) btree_addr(8) dims(u32 x dim)
        let dimensionality = 3u8; // 2 real dims + element-size slot
        let mut lo_body = vec![3u8, 2u8, dimensionality];
        lo_body.extend_from_slice(&0xABCDu64.to_le_bytes()); // btree addr
        lo_body.extend_from_slice(&10u32.to_le_bytes()); // dim 0 chunk size
        lo_body.extend_from_slice(&20u32.to_le_bytes()); // dim 1 chunk size
        lo_body.extend_from_slice(&8u32.to_le_bytes()); // element size
        while lo_body.len() % 8 != 0 {
            lo_body.push(0);
        }

        let mut msgs = Vec::new();
        msgs.extend_from_slice(&MSG_DATA_LAYOUT.to_le_bytes());
        msgs.extend_from_slice(&(lo_body.len() as u16).to_le_bytes());
        msgs.push(0); // flags
        msgs.extend_from_slice(&[0, 0, 0]); // reserved
        msgs.extend_from_slice(&lo_body);

        let header_size = msgs.len() as u32;
        let mut file_bytes = vec![0u8; 16];
        file_bytes[0] = 1; // version
        file_bytes[2..4].copy_from_slice(&1u16.to_le_bytes()); // num messages
        file_bytes[8..12].copy_from_slice(&header_size.to_le_bytes());
        file_bytes.extend_from_slice(&msgs);

        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert_eq!(layout.chunk_dims, Some(vec![10, 20]));
        assert!(layout.filter_pipeline.is_none());
    }

    /// A Filter Pipeline message alongside a chunked Data Layout message must
    /// populate both fields.
    #[test]
    fn parses_filter_pipeline_alongside_layout() {
        let dimensionality = 2u8; // 1 real dim + element-size slot
        let mut lo_body = vec![3u8, 2u8, dimensionality];
        lo_body.extend_from_slice(&0u64.to_le_bytes());
        lo_body.extend_from_slice(&4u32.to_le_bytes()); // chunk size
        lo_body.extend_from_slice(&4u32.to_le_bytes()); // element size
        while lo_body.len() % 8 != 0 {
            lo_body.push(0);
        }

        // Filter Pipeline v2: 1 filter, Deflate (id 1), 1 client value.
        let mut fp_body = vec![2u8, 1u8];
        fp_body.extend_from_slice(&1u16.to_le_bytes()); // Deflate
        fp_body.extend_from_slice(&0u16.to_le_bytes()); // flags
        fp_body.extend_from_slice(&1u16.to_le_bytes()); // 1 client value
        fp_body.extend_from_slice(&6u32.to_le_bytes()); // level 6
        while fp_body.len() % 8 != 0 {
            fp_body.push(0);
        }

        let mut msgs = Vec::new();
        msgs.extend_from_slice(&MSG_DATA_LAYOUT.to_le_bytes());
        msgs.extend_from_slice(&(lo_body.len() as u16).to_le_bytes());
        msgs.extend_from_slice(&[0, 0, 0, 0]);
        msgs.extend_from_slice(&lo_body);

        msgs.extend_from_slice(&MSG_FILTER_PIPELINE.to_le_bytes());
        msgs.extend_from_slice(&(fp_body.len() as u16).to_le_bytes());
        msgs.extend_from_slice(&[0, 0, 0, 0]);
        msgs.extend_from_slice(&fp_body);

        let header_size = msgs.len() as u32;
        let mut file_bytes = vec![0u8; 16];
        file_bytes[0] = 1;
        file_bytes[8..12].copy_from_slice(&header_size.to_le_bytes());
        file_bytes.extend_from_slice(&msgs);

        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert_eq!(layout.chunk_dims, Some(vec![4]));
        let pipeline = layout.filter_pipeline.expect("filter pipeline");
        assert_eq!(pipeline.len(), 1);
    }

    /// A continuation message must be followed to its target region and any
    /// Data Layout message found there must be recovered.
    #[test]
    fn follows_continuation_to_find_layout_message() {
        // Continuation block placed after the initial (empty) message area.
        let cont_addr: u64 = 64;
        let mut lo_body = vec![3u8, 2u8, 2u8];
        lo_body.extend_from_slice(&0u64.to_le_bytes());
        lo_body.extend_from_slice(&5u32.to_le_bytes());
        lo_body.extend_from_slice(&8u32.to_le_bytes());
        while lo_body.len() % 8 != 0 {
            lo_body.push(0);
        }
        let mut cont_msgs = Vec::new();
        cont_msgs.extend_from_slice(&MSG_DATA_LAYOUT.to_le_bytes());
        cont_msgs.extend_from_slice(&(lo_body.len() as u16).to_le_bytes());
        cont_msgs.extend_from_slice(&[0, 0, 0, 0]);
        cont_msgs.extend_from_slice(&lo_body);
        let cont_len = cont_msgs.len() as u64;

        // Initial header area: just one Continuation message.
        let mut cont_body = Vec::new();
        cont_body.extend_from_slice(&cont_addr.to_le_bytes()); // offset
        cont_body.extend_from_slice(&cont_len.to_le_bytes()); // length
        let mut msgs = Vec::new();
        msgs.extend_from_slice(&MSG_CONTINUATION.to_le_bytes());
        msgs.extend_from_slice(&(cont_body.len() as u16).to_le_bytes());
        msgs.extend_from_slice(&[0, 0, 0, 0]);
        msgs.extend_from_slice(&cont_body);

        let header_size = msgs.len() as u32;
        let mut file_bytes = vec![0u8; 16];
        file_bytes[0] = 1;
        file_bytes[8..12].copy_from_slice(&header_size.to_le_bytes());
        file_bytes.extend_from_slice(&msgs);
        // Pad up to cont_addr, then append the continuation block's messages.
        file_bytes.resize(cont_addr as usize, 0);
        file_bytes.extend_from_slice(&cont_msgs);

        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert_eq!(layout.chunk_dims, Some(vec![5]));
    }

    /// A version-2 (`OHDR`-signed) object header is recognized and skipped
    /// gracefully rather than misparsed as V1.
    #[test]
    fn v2_object_header_is_skipped_gracefully() {
        let mut file_bytes = b"OHDR".to_vec();
        file_bytes.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0]);
        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert!(layout.chunk_dims.is_none());
        assert!(layout.filter_pipeline.is_none());
    }

    /// A contiguous (non-chunked) Data Layout message must not populate
    /// chunk_dims.
    #[test]
    fn contiguous_layout_leaves_chunk_dims_none() {
        // v3 contiguous body: version(1) class(1)=1 address(8) size(8)
        let mut lo_body = vec![3u8, 1u8];
        lo_body.extend_from_slice(&0u64.to_le_bytes());
        lo_body.extend_from_slice(&64u64.to_le_bytes());
        while lo_body.len() % 8 != 0 {
            lo_body.push(0);
        }
        let mut msgs = Vec::new();
        msgs.extend_from_slice(&MSG_DATA_LAYOUT.to_le_bytes());
        msgs.extend_from_slice(&(lo_body.len() as u16).to_le_bytes());
        msgs.extend_from_slice(&[0, 0, 0, 0]);
        msgs.extend_from_slice(&lo_body);

        let header_size = msgs.len() as u32;
        let mut file_bytes = vec![0u8; 16];
        file_bytes[0] = 1;
        file_bytes[8..12].copy_from_slice(&header_size.to_le_bytes());
        file_bytes.extend_from_slice(&msgs);

        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert!(layout.chunk_dims.is_none());
    }

    /// Truncated / malformed input must never panic — it simply yields an
    /// empty layout.
    #[test]
    fn truncated_header_does_not_panic() {
        let file_bytes = vec![1u8, 0, 1, 0]; // way too short
        let mut cursor = Cursor::new(file_bytes);
        let layout = read_dataset_layout(&mut cursor, 0, 8, 8).expect("parse");
        assert!(layout.chunk_dims.is_none());
        assert!(layout.filter_pipeline.is_none());
    }
}
