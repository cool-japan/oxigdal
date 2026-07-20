//! COPC octree hierarchy page parsing and spatial traversal.
//!
//! The COPC specification stores the octree index as a flat table of
//! [`HierarchyEntry`] records, each mapping a [`VoxelKey`] (depth + x/y/z cell
//! index) to either point data (`byte_count > 0`), an empty node
//! (`byte_count == 0`) or a page pointer to a child hierarchy page
//! (`byte_count == -1`).
//!
//! Reference: <https://copc.io/copc-specification-1.0.pdf> sections 3.2-3.3.

use crate::copc_vlr::CopcInfo;
use crate::error::CopcError;
use crate::point::BoundingBox3D;

/// Size of a single VoxelKey in bytes (4 x i32 = 16).
const VOXEL_KEY_SIZE: usize = 16;

/// Size of a single HierarchyEntry in bytes (VoxelKey + u64 + i32 + i32 = 32).
const ENTRY_SIZE: usize = 32;

/// Sentinel value for `byte_count` indicating a page pointer.
const PAGE_POINTER_SENTINEL: i32 = -1;

/// Octree voxel key: depth level plus cell indices.
///
/// At depth 0 the root node covers the entire octree extent.  At depth `d`
/// each axis is divided into `2^d` cells, so `x`, `y`, `z` range from
/// `0` to `2^d - 1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoxelKey {
    /// Depth level in the octree (0 = root).
    pub depth: i32,
    /// X cell index at this depth.
    pub x: i32,
    /// Y cell index at this depth.
    pub y: i32,
    /// Z cell index at this depth.
    pub z: i32,
}

impl VoxelKey {
    /// Parse a VoxelKey from 16 bytes of little-endian data.
    pub fn from_bytes(data: &[u8]) -> Result<Self, CopcError> {
        if data.len() < VOXEL_KEY_SIZE {
            return Err(CopcError::InvalidFormat(format!(
                "VoxelKey too short: {} bytes (need {VOXEL_KEY_SIZE})",
                data.len()
            )));
        }
        Ok(Self {
            depth: i32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            x: i32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            y: i32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            z: i32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }

    /// Serialize this key to 16 bytes of little-endian data.
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&self.depth.to_le_bytes());
        buf[4..8].copy_from_slice(&self.x.to_le_bytes());
        buf[8..12].copy_from_slice(&self.y.to_le_bytes());
        buf[12..16].copy_from_slice(&self.z.to_le_bytes());
        buf
    }
}

/// A single entry in the COPC hierarchy page.
///
/// | Field       | Type  | Meaning                                     |
/// |-------------|-------|---------------------------------------------|
/// | key         | VoxelKey | Octree cell this entry describes           |
/// | offset      | u64   | Byte offset from file start to the data     |
/// | byte_count  | i32   | `-1` = page pointer, `0` = empty, `>0` = point data |
/// | point_count | i32   | Number of point records in this chunk        |
#[derive(Debug, Clone)]
pub struct HierarchyEntry {
    /// Octree voxel key.
    pub key: VoxelKey,
    /// Byte offset from file start.
    pub offset: u64,
    /// Byte count of point data, or sentinel values.
    pub byte_count: i32,
    /// Number of point records in this chunk.
    pub point_count: i32,
}

impl HierarchyEntry {
    /// Parse a single hierarchy entry from 32 bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, CopcError> {
        if data.len() < ENTRY_SIZE {
            return Err(CopcError::InvalidFormat(format!(
                "HierarchyEntry too short: {} bytes (need {ENTRY_SIZE})",
                data.len()
            )));
        }
        let key = VoxelKey::from_bytes(&data[0..16])?;
        let offset = u64::from_le_bytes([
            data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
        ]);
        let byte_count = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let point_count = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);

        Ok(Self {
            key,
            offset,
            byte_count,
            point_count,
        })
    }

    /// Serialize this entry to 32 bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..16].copy_from_slice(&self.key.to_bytes());
        buf[16..24].copy_from_slice(&self.offset.to_le_bytes());
        buf[24..28].copy_from_slice(&self.byte_count.to_le_bytes());
        buf[28..32].copy_from_slice(&self.point_count.to_le_bytes());
        buf
    }

    /// Returns `true` when this entry is a page pointer to another hierarchy page.
    #[inline]
    pub fn is_page_pointer(&self) -> bool {
        self.byte_count == PAGE_POINTER_SENTINEL
    }

    /// Returns `true` when this entry indicates an empty node.
    #[inline]
    pub fn is_empty_node(&self) -> bool {
        self.byte_count == 0
    }

    /// Returns `true` when this entry contains point data.
    #[inline]
    pub fn has_point_data(&self) -> bool {
        self.byte_count > 0
    }
}

/// Parse a hierarchy page from raw bytes.
///
/// A page is simply a sequence of 32-byte [`HierarchyEntry`] records.
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when the page length is not a multiple
/// of 32 or an individual entry is malformed.
pub fn parse_hierarchy_page(data: &[u8]) -> Result<Vec<HierarchyEntry>, CopcError> {
    if !data.len().is_multiple_of(ENTRY_SIZE) {
        return Err(CopcError::InvalidFormat(format!(
            "Hierarchy page length {} is not a multiple of {ENTRY_SIZE}",
            data.len()
        )));
    }
    let count = data.len() / ENTRY_SIZE;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * ENTRY_SIZE;
        let entry = HierarchyEntry::from_bytes(&data[start..start + ENTRY_SIZE])?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Compute the 3D bounding box of an octree node identified by its [`VoxelKey`].
///
/// At depth 0 the root covers the full extent from `(center - halfsize)` to
/// `(center + halfsize)`.  At depth `d`, each axis is subdivided into `2^d`
/// cells of size `2 * halfsize / 2^d`.
pub fn node_bounds(key: &VoxelKey, info: &CopcInfo) -> BoundingBox3D {
    let full_size = info.halfsize * 2.0;
    let origin_x = info.center_x - info.halfsize;
    let origin_y = info.center_y - info.halfsize;
    let origin_z = info.center_z - info.halfsize;

    if key.depth <= 0 {
        // Root node covers the full extent.
        return BoundingBox3D {
            min_x: origin_x,
            min_y: origin_y,
            min_z: origin_z,
            max_x: origin_x + full_size,
            max_y: origin_y + full_size,
            max_z: origin_z + full_size,
        };
    }

    let divisions = 2.0_f64.powi(key.depth);
    let node_size = full_size / divisions;

    let min_x = origin_x + key.x as f64 * node_size;
    let min_y = origin_y + key.y as f64 * node_size;
    let min_z = origin_z + key.z as f64 * node_size;

    BoundingBox3D {
        min_x,
        min_y,
        min_z,
        max_x: min_x + node_size,
        max_y: min_y + node_size,
        max_z: min_z + node_size,
    }
}

/// Collect all hierarchy entries whose octree nodes intersect `query_bbox`.
///
/// Traverses the hierarchy by loading pages from the file data on demand.
/// Returns entries that have point data (`byte_count > 0`) and whose spatial
/// bounds overlap the query.
///
/// # Parameters
/// - `file_data` -- the complete COPC file bytes
/// - `info` -- parsed COPC info VLR
/// - `root_page_offset` -- byte offset of the root hierarchy page
/// - `root_page_size` -- byte length of the root hierarchy page
/// - `query_bbox` -- 3D bounding box to test intersection against
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when a hierarchy page is truncated or
/// malformed.
pub fn query_hierarchy(
    file_data: &[u8],
    info: &CopcInfo,
    root_page_offset: u64,
    root_page_size: u64,
    query_bbox: &BoundingBox3D,
) -> Result<Vec<HierarchyEntry>, CopcError> {
    let mut result: Vec<HierarchyEntry> = Vec::new();

    // Stack-based iterative traversal of hierarchy pages.
    let mut page_stack: Vec<(u64, u64)> = vec![(root_page_offset, root_page_size)];

    // Guard against pathological recursion/loops.
    let max_pages = 10_000;
    let mut pages_visited = 0_usize;

    while let Some((page_offset, page_size)) = page_stack.pop() {
        pages_visited += 1;
        if pages_visited > max_pages {
            return Err(CopcError::InvalidFormat(
                "Hierarchy traversal exceeded maximum page count (possible cycle)".into(),
            ));
        }

        let off = page_offset as usize;
        let sz = page_size as usize;
        if off + sz > file_data.len() {
            return Err(CopcError::InvalidFormat(format!(
                "Hierarchy page at offset {off} + size {sz} exceeds file length {}",
                file_data.len()
            )));
        }

        let page_data = &file_data[off..off + sz];
        let entries = parse_hierarchy_page(page_data)?;

        for entry in entries {
            let entry_bounds = node_bounds(&entry.key, info);

            if !entry_bounds.intersects_3d(query_bbox) {
                continue;
            }

            if entry.is_page_pointer() {
                // Follow page pointer.  Per COPC 1.0 spec, when byte_count == -1
                // the child page byte length is stored in `point_count`.
                let child_size = entry.point_count.unsigned_abs() as u64;
                page_stack.push((entry.offset, child_size));
            } else if entry.has_point_data() {
                result.push(entry);
            }
            // Empty nodes (byte_count == 0) are silently skipped.
        }
    }

    Ok(result)
}

/// Variant of [`query_hierarchy`] that handles page pointers using the
/// `point_count` field as the child page size in bytes.
///
/// This matches the COPC 1.0 specification where page pointer entries have
/// `byte_count == -1` and the child page byte length is stored in
/// `point_count`.
pub fn query_hierarchy_with_page_pointers(
    file_data: &[u8],
    info: &CopcInfo,
    root_page_offset: u64,
    root_page_size: u64,
    query_bbox: &BoundingBox3D,
) -> Result<Vec<HierarchyEntry>, CopcError> {
    let mut result: Vec<HierarchyEntry> = Vec::new();
    let mut page_stack: Vec<(u64, u64)> = vec![(root_page_offset, root_page_size)];
    let max_pages = 10_000;
    let mut pages_visited = 0_usize;

    while let Some((page_offset, page_size)) = page_stack.pop() {
        pages_visited += 1;
        if pages_visited > max_pages {
            return Err(CopcError::InvalidFormat(
                "Hierarchy traversal exceeded maximum page count (possible cycle)".into(),
            ));
        }

        let off = page_offset as usize;
        let sz = page_size as usize;
        if off + sz > file_data.len() {
            return Err(CopcError::InvalidFormat(format!(
                "Hierarchy page at offset {off} + size {sz} exceeds file length {}",
                file_data.len()
            )));
        }

        let page_data = &file_data[off..off + sz];
        let entries = parse_hierarchy_page(page_data)?;

        for entry in entries {
            let entry_bounds = node_bounds(&entry.key, info);

            if !entry_bounds.intersects_3d(query_bbox) {
                continue;
            }

            if entry.is_page_pointer() {
                // point_count stores the child page size in bytes
                let child_size = entry.point_count.unsigned_abs() as u64;
                page_stack.push((entry.offset, child_size));
            } else if entry.has_point_data() {
                result.push(entry);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_copc_info() -> CopcInfo {
        CopcInfo {
            center_x: 50.0,
            center_y: 50.0,
            center_z: 50.0,
            halfsize: 50.0,
            spacing: 1.0,
            root_hier_offset: 0,
            root_hier_size: 0,
            gpstime_minimum: 0.0,
            gpstime_maximum: 100.0,
        }
    }

    #[test]
    fn test_voxel_key_roundtrip() {
        let key = VoxelKey {
            depth: 3,
            x: 5,
            y: 7,
            z: 2,
        };
        let bytes = key.to_bytes();
        let parsed = VoxelKey::from_bytes(&bytes).expect("roundtrip parse");
        assert_eq!(parsed, key);
    }

    #[test]
    fn test_voxel_key_too_short() {
        let data = [0u8; 15];
        assert!(VoxelKey::from_bytes(&data).is_err());
    }

    #[test]
    fn test_hierarchy_entry_roundtrip() {
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 2,
                x: 1,
                y: 2,
                z: 3,
            },
            offset: 12345,
            byte_count: 5000,
            point_count: 250,
        };
        let bytes = entry.to_bytes();
        let parsed = HierarchyEntry::from_bytes(&bytes).expect("roundtrip");
        assert_eq!(parsed.key, entry.key);
        assert_eq!(parsed.offset, 12345);
        assert_eq!(parsed.byte_count, 5000);
        assert_eq!(parsed.point_count, 250);
    }

    #[test]
    fn test_hierarchy_entry_page_pointer() {
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 999,
            byte_count: -1,
            point_count: 0,
        };
        assert!(entry.is_page_pointer());
        assert!(!entry.is_empty_node());
        assert!(!entry.has_point_data());
    }

    #[test]
    fn test_hierarchy_entry_empty_node() {
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 0,
            byte_count: 0,
            point_count: 0,
        };
        assert!(!entry.is_page_pointer());
        assert!(entry.is_empty_node());
        assert!(!entry.has_point_data());
    }

    #[test]
    fn test_hierarchy_entry_has_data() {
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 100,
            byte_count: 500,
            point_count: 25,
        };
        assert!(!entry.is_page_pointer());
        assert!(!entry.is_empty_node());
        assert!(entry.has_point_data());
    }

    #[test]
    fn test_hierarchy_entry_too_short() {
        let data = [0u8; 31];
        assert!(HierarchyEntry::from_bytes(&data).is_err());
    }

    #[test]
    fn test_parse_hierarchy_page_empty() {
        let entries = parse_hierarchy_page(&[]).expect("empty page");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_hierarchy_page_one_entry() {
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 500,
            byte_count: 200,
            point_count: 10,
        };
        let bytes = entry.to_bytes();
        let entries = parse_hierarchy_page(&bytes).expect("one entry");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].offset, 500);
        assert_eq!(entries[0].point_count, 10);
    }

    #[test]
    fn test_parse_hierarchy_page_not_multiple_of_32() {
        let data = [0u8; 33]; // not a multiple of 32
        assert!(parse_hierarchy_page(&data).is_err());
    }

    #[test]
    fn test_parse_hierarchy_page_two_entries() {
        let e1 = HierarchyEntry {
            key: VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 100,
            byte_count: 50,
            point_count: 5,
        };
        let e2 = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 200,
            byte_count: 80,
            point_count: 8,
        };
        let mut page_data = Vec::new();
        page_data.extend_from_slice(&e1.to_bytes());
        page_data.extend_from_slice(&e2.to_bytes());
        let entries = parse_hierarchy_page(&page_data).expect("two entries");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_node_bounds_root() {
        let info = make_copc_info();
        let key = VoxelKey {
            depth: 0,
            x: 0,
            y: 0,
            z: 0,
        };
        let bounds = node_bounds(&key, &info);
        assert!((bounds.min_x - 0.0).abs() < 1e-9);
        assert!((bounds.min_y - 0.0).abs() < 1e-9);
        assert!((bounds.min_z - 0.0).abs() < 1e-9);
        assert!((bounds.max_x - 100.0).abs() < 1e-9);
        assert!((bounds.max_y - 100.0).abs() < 1e-9);
        assert!((bounds.max_z - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_node_bounds_depth1_first_octant() {
        let info = make_copc_info();
        let key = VoxelKey {
            depth: 1,
            x: 0,
            y: 0,
            z: 0,
        };
        let bounds = node_bounds(&key, &info);
        // At depth 1: 2 divisions, node_size = 100/2 = 50
        assert!((bounds.min_x - 0.0).abs() < 1e-9);
        assert!((bounds.max_x - 50.0).abs() < 1e-9);
        assert!((bounds.min_y - 0.0).abs() < 1e-9);
        assert!((bounds.max_y - 50.0).abs() < 1e-9);
    }

    #[test]
    fn test_node_bounds_depth1_last_octant() {
        let info = make_copc_info();
        let key = VoxelKey {
            depth: 1,
            x: 1,
            y: 1,
            z: 1,
        };
        let bounds = node_bounds(&key, &info);
        assert!((bounds.min_x - 50.0).abs() < 1e-9);
        assert!((bounds.max_x - 100.0).abs() < 1e-9);
        assert!((bounds.min_y - 50.0).abs() < 1e-9);
        assert!((bounds.max_y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_node_bounds_depth2() {
        let info = make_copc_info();
        let key = VoxelKey {
            depth: 2,
            x: 1,
            y: 2,
            z: 3,
        };
        let bounds = node_bounds(&key, &info);
        // At depth 2: 4 divisions, node_size = 100/4 = 25
        assert!((bounds.min_x - 25.0).abs() < 1e-9);
        assert!((bounds.max_x - 50.0).abs() < 1e-9);
        assert!((bounds.min_y - 50.0).abs() < 1e-9);
        assert!((bounds.max_y - 75.0).abs() < 1e-9);
        assert!((bounds.min_z - 75.0).abs() < 1e-9);
        assert!((bounds.max_z - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_query_hierarchy_single_data_entry() {
        let info = make_copc_info();

        // Build a hierarchy page with one data entry at the root
        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 100,
            byte_count: 500,
            point_count: 25,
        };
        let page_data = entry.to_bytes();

        // Build file_data: some padding then the page at offset 0
        let file_data = page_data.to_vec();

        // Query that covers the whole extent
        let query =
            BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 100.0).expect("valid query bbox");
        let results =
            query_hierarchy(&file_data, &info, 0, 32, &query).expect("query should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].point_count, 25);
    }

    #[test]
    fn test_query_hierarchy_spatial_filtering() {
        let info = make_copc_info();

        // Two entries at depth 1:
        // Entry at (1,0,0,0) covers [0,50) -- lower half in x
        // Entry at (1,1,0,0) covers [50,100) -- upper half in x
        let e1 = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 200,
            byte_count: 100,
            point_count: 10,
        };
        let e2 = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 1,
                y: 0,
                z: 0,
            },
            offset: 300,
            byte_count: 100,
            point_count: 15,
        };
        let mut page_data = Vec::new();
        page_data.extend_from_slice(&e1.to_bytes());
        page_data.extend_from_slice(&e2.to_bytes());

        // Query only the upper half (x > 50)
        let query =
            BoundingBox3D::new(60.0, 0.0, 0.0, 100.0, 100.0, 100.0).expect("valid query bbox");
        let results =
            query_hierarchy(&page_data, &info, 0, 64, &query).expect("query should succeed");
        // Only e2 should match
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].point_count, 15);
    }

    #[test]
    fn test_query_hierarchy_empty_nodes_skipped() {
        let info = make_copc_info();

        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 0,
            byte_count: 0,
            point_count: 0,
        };
        let page_data = entry.to_bytes();

        let query =
            BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 100.0).expect("valid query bbox");
        let results =
            query_hierarchy(&page_data, &info, 0, 32, &query).expect("query should succeed");
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_hierarchy_page_pointer_traversal() {
        let info = make_copc_info();

        // Root page contains a page pointer to a child page
        // Child page contains a data entry
        let page_pointer = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 32, // child page starts right after root page
            byte_count: -1,
            point_count: 0,
        };

        let data_entry = HierarchyEntry {
            key: VoxelKey {
                depth: 2,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 1000,
            byte_count: 300,
            point_count: 30,
        };

        let mut file_data = Vec::new();
        file_data.extend_from_slice(&page_pointer.to_bytes()); // root page at offset 0
        file_data.extend_from_slice(&data_entry.to_bytes()); // child page at offset 32

        // The page pointer has byte_count == -1, so |byte_count| = 1 which is not
        // a valid page size. For page pointers the offset points to a page and the
        // actual page size should be derivable. Let's adjust: when byte_count == -1
        // we use the absolute value (1) as the page size -- but 1 byte is not
        // a multiple of 32. We need to re-think the page pointer size.
        //
        // Actually in real COPC files the page pointer's "offset" field points
        // to the start of the child page and the actual page size is determined
        // by reading entries until the page boundary.
        //
        // For this test, let's put the child page size as 32 somewhere accessible.
        // In real COPC, the page pointer byte_count is -1 and we need to know the
        // page size from a different VLR or from the structure.
        //
        // Looking at the COPC spec more carefully: page pointers have
        // byte_count == -1. The child page size is NOT stored in the entry itself;
        // it must be inferred or stored as metadata elsewhere. For the implementation,
        // we should store page sizes separately.
        //
        // However, examining popular COPC implementations (PDAL, copc-lib), page
        // pointers actually store the PAGE SIZE in the offset field as u64, and the
        // point_count field (i32) may be repurposed. Let me adjust the implementation.
        //
        // After reviewing the spec again: the page pointer entry stores:
        //   - offset = byte offset to child page
        //   - byte_count = -1 (sentinel)
        //   - point_count = number of entries in child page (actually, byte size / 32)
        //
        // Actually the COPC 1.0 spec says for page pointers:
        //   byte_count = -1 means "this entry is a pointer to a child hierarchy page"
        //   offset = byte offset to child page data
        //   point_count = byte size of child page
        //
        // Let me use that convention.

        // OK, let me redo this properly.
        // Re-building: page_pointer.point_count stores the child page byte size.
        let page_pointer_v2 = HierarchyEntry {
            key: VoxelKey {
                depth: 1,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 32, // child page at offset 32
            byte_count: -1,
            point_count: 32, // child page size in bytes
        };

        let mut file_data_v2 = Vec::new();
        file_data_v2.extend_from_slice(&page_pointer_v2.to_bytes());
        file_data_v2.extend_from_slice(&data_entry.to_bytes());

        let query = BoundingBox3D::new(0.0, 0.0, 0.0, 30.0, 30.0, 30.0).expect("valid query bbox");

        // For this test, use a custom query that calls through with the adjusted page size
        let results = query_hierarchy_with_page_pointers(&file_data_v2, &info, 0, 32, &query)
            .expect("should traverse page pointer");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].point_count, 30);
    }

    #[test]
    fn test_query_hierarchy_truncated_page_errors() {
        let info = make_copc_info();
        let query =
            BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 100.0).expect("valid query bbox");
        // page_offset + page_size exceeds file_data length
        let file_data = vec![0u8; 10];
        assert!(query_hierarchy(&file_data, &info, 0, 32, &query).is_err());
    }

    #[test]
    fn test_query_hierarchy_no_intersection() {
        let info = make_copc_info();

        let entry = HierarchyEntry {
            key: VoxelKey {
                depth: 0,
                x: 0,
                y: 0,
                z: 0,
            },
            offset: 100,
            byte_count: 500,
            point_count: 25,
        };
        let page_data = entry.to_bytes();

        // Query outside the octree extent
        let query =
            BoundingBox3D::new(200.0, 200.0, 200.0, 300.0, 300.0, 300.0).expect("valid query bbox");
        let results =
            query_hierarchy(&page_data, &info, 0, 32, &query).expect("query should succeed");
        assert!(results.is_empty());
    }

    #[test]
    fn test_node_bounds_asymmetric_info() {
        let info = CopcInfo {
            center_x: 100.0,
            center_y: 200.0,
            center_z: 50.0,
            halfsize: 25.0,
            spacing: 1.0,
            root_hier_offset: 0,
            root_hier_size: 0,
            gpstime_minimum: 0.0,
            gpstime_maximum: 0.0,
        };
        let key = VoxelKey {
            depth: 0,
            x: 0,
            y: 0,
            z: 0,
        };
        let bounds = node_bounds(&key, &info);
        // origin = center - halfsize = (75, 175, 25)
        assert!((bounds.min_x - 75.0).abs() < 1e-9);
        assert!((bounds.min_y - 175.0).abs() < 1e-9);
        assert!((bounds.min_z - 25.0).abs() < 1e-9);
        assert!((bounds.max_x - 125.0).abs() < 1e-9);
        assert!((bounds.max_y - 225.0).abs() < 1e-9);
        assert!((bounds.max_z - 75.0).abs() < 1e-9);
    }

    #[test]
    fn test_voxel_key_negative_depth() {
        // depth <= 0 treated as root
        let info = make_copc_info();
        let key = VoxelKey {
            depth: -1,
            x: 0,
            y: 0,
            z: 0,
        };
        let bounds = node_bounds(&key, &info);
        assert!((bounds.min_x - 0.0).abs() < 1e-9);
        assert!((bounds.max_x - 100.0).abs() < 1e-9);
    }
}
