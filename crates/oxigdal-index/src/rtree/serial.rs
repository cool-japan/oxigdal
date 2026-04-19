//! Binary serialization / deserialization for the R-tree.
//!
//! # Wire format
//!
//! ```text
//! [4 bytes] magic "RTIX"
//! [1 byte ] version (currently 1)
//! [4 bytes] LE entry count (u32)
//! [1 byte ] max_entries (M, u8)
//! ... recursive node dump ...
//! ```
//!
//! Each node is prefixed by a tag byte:
//! - `0x01` = leaf node, followed by `u16 LE entry_count`, then each entry:
//!   `[8*4 bytes] bbox (min_x, min_y, max_x, max_y as f64 LE)`,
//!   `[4 bytes] value_len (u32 LE)`, `[value_len bytes] value`.
//! - `0x02` = internal node, followed by `u16 LE child_count`, then each
//!   child: `[8*4 bytes] bbox`, recursively-serialized child node.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::bbox::Bbox2D;
use crate::error::IndexError;

use super::node::{InternalEntry, InternalNode, LeafEntry, LeafNode, Node};

/// Magic bytes identifying an R-tree binary stream.
const MAGIC: [u8; 4] = *b"RTIX";
/// Current serialization format version.
const VERSION: u8 = 1;
/// Tag byte for a leaf node.
const TAG_LEAF: u8 = 0x01;
/// Tag byte for an internal node.
const TAG_INTERNAL: u8 = 0x02;

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize an R-tree to bytes.
///
/// `T` must implement `AsRef<[u8]>` so each value can be written as a
/// length-prefixed blob.
pub(crate) fn serialize<T: AsRef<[u8]>>(
    root: &Option<Node<T>>,
    entry_count: usize,
    max_entries: usize,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header.
    buf.extend_from_slice(&MAGIC);
    buf.push(VERSION);
    let count32 = entry_count.min(u32::MAX as usize) as u32;
    buf.extend_from_slice(&count32.to_le_bytes());
    let m = max_entries.min(u8::MAX as usize) as u8;
    buf.push(m);

    // Recursive node dump.
    if let Some(node) = root {
        serialize_node(node, &mut buf);
    }

    buf
}

fn serialize_node<T: AsRef<[u8]>>(node: &Node<T>, buf: &mut Vec<u8>) {
    match node {
        Node::Leaf(leaf) => {
            buf.push(TAG_LEAF);
            let count = leaf.entries.len().min(u16::MAX as usize) as u16;
            buf.extend_from_slice(&count.to_le_bytes());
            for e in &leaf.entries {
                write_bbox(&e.bbox, buf);
                let bytes = e.value.as_ref();
                let vlen = bytes.len().min(u32::MAX as usize) as u32;
                buf.extend_from_slice(&vlen.to_le_bytes());
                buf.extend_from_slice(bytes);
            }
        }
        Node::Internal(internal) => {
            buf.push(TAG_INTERNAL);
            let count = internal.entries.len().min(u16::MAX as usize) as u16;
            buf.extend_from_slice(&count.to_le_bytes());
            for e in &internal.entries {
                write_bbox(&e.bbox, buf);
                serialize_node(&e.child, buf);
            }
        }
    }
}

fn write_bbox(bbox: &Bbox2D, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&bbox.min_x.to_le_bytes());
    buf.extend_from_slice(&bbox.min_y.to_le_bytes());
    buf.extend_from_slice(&bbox.max_x.to_le_bytes());
    buf.extend_from_slice(&bbox.max_y.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Deserialization
// ---------------------------------------------------------------------------

/// Deserialize an R-tree from bytes.
///
/// Returns `(root, entry_count, max_entries)`.
pub(crate) fn deserialize<T: From<Vec<u8>>>(
    data: &[u8],
) -> Result<(Option<Node<T>>, usize, usize), IndexError> {
    let mut cursor = 0usize;

    // Read magic.
    if data.len() < 10 {
        return Err(IndexError::TruncatedData(0));
    }
    if data[0..4] != MAGIC {
        return Err(IndexError::InvalidMagic);
    }
    cursor += 4;

    // Version.
    let version = data[cursor];
    if version != VERSION {
        return Err(IndexError::UnsupportedVersion(version));
    }
    cursor += 1;

    // Entry count.
    let count = read_u32_le(data, &mut cursor)? as usize;

    // Max entries.
    if cursor >= data.len() {
        return Err(IndexError::TruncatedData(cursor));
    }
    let max_entries = data[cursor] as usize;
    cursor += 1;

    // If count is 0, no node follows.
    if count == 0 {
        return Ok((None, 0, max_entries.max(2)));
    }

    let node = deserialize_node(data, &mut cursor)?;
    Ok((Some(node), count, max_entries.max(2)))
}

fn deserialize_node<T: From<Vec<u8>>>(
    data: &[u8],
    cursor: &mut usize,
) -> Result<Node<T>, IndexError> {
    if *cursor >= data.len() {
        return Err(IndexError::TruncatedData(*cursor));
    }
    let tag = data[*cursor];
    *cursor += 1;

    match tag {
        TAG_LEAF => {
            let entry_count = read_u16_le(data, cursor)? as usize;
            let mut entries = Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let bbox = read_bbox(data, cursor)?;
                let vlen = read_u32_le(data, cursor)? as usize;
                if *cursor + vlen > data.len() {
                    return Err(IndexError::TruncatedData(*cursor));
                }
                let value_bytes = data[*cursor..*cursor + vlen].to_vec();
                *cursor += vlen;
                entries.push(LeafEntry {
                    bbox,
                    value: T::from(value_bytes),
                });
            }
            Ok(Node::Leaf(LeafNode { entries }))
        }
        TAG_INTERNAL => {
            let child_count = read_u16_le(data, cursor)? as usize;
            let mut entries = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                let bbox = read_bbox(data, cursor)?;
                let child = deserialize_node(data, cursor)?;
                entries.push(InternalEntry {
                    bbox,
                    child: Box::new(child),
                });
            }
            Ok(Node::Internal(InternalNode { entries }))
        }
        _ => Err(IndexError::TruncatedData(*cursor - 1)),
    }
}

// ---------------------------------------------------------------------------
// Primitive readers
// ---------------------------------------------------------------------------

fn read_u16_le(data: &[u8], cursor: &mut usize) -> Result<u16, IndexError> {
    if *cursor + 2 > data.len() {
        return Err(IndexError::TruncatedData(*cursor));
    }
    let bytes: [u8; 2] = [data[*cursor], data[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32_le(data: &[u8], cursor: &mut usize) -> Result<u32, IndexError> {
    if *cursor + 4 > data.len() {
        return Err(IndexError::TruncatedData(*cursor));
    }
    let bytes: [u8; 4] = [
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ];
    *cursor += 4;
    Ok(u32::from_le_bytes(bytes))
}

fn read_f64_le(data: &[u8], cursor: &mut usize) -> Result<f64, IndexError> {
    if *cursor + 8 > data.len() {
        return Err(IndexError::TruncatedData(*cursor));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[*cursor..*cursor + 8]);
    *cursor += 8;
    Ok(f64::from_le_bytes(bytes))
}

fn read_bbox(data: &[u8], cursor: &mut usize) -> Result<Bbox2D, IndexError> {
    let min_x = read_f64_le(data, cursor)?;
    let min_y = read_f64_le(data, cursor)?;
    let max_x = read_f64_le(data, cursor)?;
    let max_y = read_f64_le(data, cursor)?;
    // The bbox should have been valid when serialized; trust the data.
    Ok(Bbox2D {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::node::search_node;
    use super::*;

    /// Helper: build a small tree manually.
    fn build_small_tree() -> (Option<Node<Vec<u8>>>, usize, usize) {
        let max_entries = 9;
        let mut entries = Vec::new();
        for i in 0u32..5 {
            let f = i as f64;
            let bbox = Bbox2D::new(f, f, f + 1.0, f + 1.0).expect("valid");
            entries.push(LeafEntry {
                bbox,
                value: i.to_le_bytes().to_vec(),
            });
        }
        let root = Node::Leaf(LeafNode { entries });
        (Some(root), 5, max_entries)
    }

    #[test]
    fn roundtrip_empty_tree() {
        let data = serialize::<Vec<u8>>(&None, 0, 9);
        let (root, count, max_entries) = deserialize::<Vec<u8>>(&data).expect("valid");
        assert!(root.is_none());
        assert_eq!(count, 0);
        assert_eq!(max_entries, 9);
    }

    #[test]
    fn roundtrip_small_tree() {
        let (root, count, max_entries) = build_small_tree();
        let data = serialize(&root, count, max_entries);
        let (root2, count2, m2) = deserialize::<Vec<u8>>(&data).expect("valid");
        assert_eq!(count2, 5);
        assert_eq!(m2, 9);
        // Search should find entry at (2, 2).
        let query = Bbox2D::new(2.0, 2.0, 3.0, 3.0).expect("valid");
        let root2 = root2.expect("non-empty");
        let mut results: Vec<&Vec<u8>> = Vec::new();
        search_node(&root2, &query, &mut results);
        assert!(
            !results.is_empty(),
            "deserialized tree should find entry at (2,2)"
        );
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut data = serialize::<Vec<u8>>(&None, 0, 9);
        data[0] = b'X';
        assert!(matches!(
            deserialize::<Vec<u8>>(&data),
            Err(IndexError::InvalidMagic)
        ));
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut data = serialize::<Vec<u8>>(&None, 0, 9);
        data[4] = 99;
        assert!(matches!(
            deserialize::<Vec<u8>>(&data),
            Err(IndexError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn truncated_data_rejected() {
        let data: &[u8] = b"RTIX";
        assert!(matches!(
            deserialize::<Vec<u8>>(data),
            Err(IndexError::TruncatedData(_))
        ));
    }
}
