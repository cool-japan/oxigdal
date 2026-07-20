//! Packed Hilbert R-tree spatial index for `FlatGeobuf`
//!
//! `FlatGeobuf` uses a packed Hilbert R-tree for spatial indexing, which enables
//! efficient spatial queries without loading the entire dataset into memory.
//!
//! The index is stored as a sequence of nodes, where each node contains:
//! - Bounding box (`min_x`, `min_y`, `max_x`, `max_y`)
//! - Offset to the feature data
//!
//! The tree is built bottom-up using Hilbert curve ordering for optimal spatial locality.

use crate::error::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Bounding box for spatial index node
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
}

impl BoundingBox {
    /// Creates a new bounding box
    #[must_use]
    pub const fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Creates an empty/invalid bounding box
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    /// Returns true if the bounding box is valid
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y
    }

    /// Returns true if this bounding box intersects with another
    #[must_use]
    pub const fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Returns true if this bounding box contains another
    #[must_use]
    pub const fn contains(&self, other: &Self) -> bool {
        self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }

    /// Expands this bounding box to include another
    pub fn expand(&mut self, other: &Self) {
        if !other.is_valid() {
            return;
        }

        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
    }

    /// Computes the center point
    #[must_use]
    pub const fn center(&self) -> (f64, f64) {
        (
            f64::midpoint(self.min_x, self.max_x),
            f64::midpoint(self.min_y, self.max_y),
        )
    }

    /// Computes the width
    #[must_use]
    pub const fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Computes the height
    #[must_use]
    pub const fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Computes the area
    #[must_use]
    pub const fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Reads a bounding box from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        Ok(Self {
            min_x: reader.read_f64::<LittleEndian>()?,
            min_y: reader.read_f64::<LittleEndian>()?,
            max_x: reader.read_f64::<LittleEndian>()?,
            max_y: reader.read_f64::<LittleEndian>()?,
        })
    }

    /// Writes a bounding box to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_f64::<LittleEndian>(self.min_x)?;
        writer.write_f64::<LittleEndian>(self.min_y)?;
        writer.write_f64::<LittleEndian>(self.max_x)?;
        writer.write_f64::<LittleEndian>(self.max_y)?;
        Ok(())
    }
}

/// R-tree node in the packed index
#[derive(Debug, Clone)]
pub struct Node {
    /// Bounding box of the node
    pub bbox: BoundingBox,
    /// Offset to feature data (for leaf nodes) or child nodes (for internal nodes)
    pub offset: u64,
}

impl Node {
    /// Creates a new node
    #[must_use]
    pub const fn new(bbox: BoundingBox, offset: u64) -> Self {
        Self { bbox, offset }
    }

    /// Size of a node in bytes (bbox + offset)
    pub const NODE_SIZE: usize = 32 + 8; // 4 * f64 + u64

    /// Reads a node from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let bbox = BoundingBox::read(reader)?;
        let offset = reader.read_u64::<LittleEndian>()?;
        Ok(Self::new(bbox, offset))
    }

    /// Writes a node to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        self.bbox.write(writer)?;
        writer.write_u64::<LittleEndian>(self.offset)?;
        Ok(())
    }
}

/// Packed Hilbert R-tree index
#[derive(Debug, Clone)]
pub struct PackedRTree {
    /// All nodes in the tree (stored in breadth-first order)
    pub nodes: Vec<Node>,
    /// Number of nodes per level
    pub level_sizes: Vec<usize>,
    /// Node size parameter (default: 16)
    pub node_size: usize,
}

impl PackedRTree {
    /// Default node size for the R-tree
    pub const DEFAULT_NODE_SIZE: usize = 16;

    /// Creates a new empty R-tree
    #[must_use]
    pub fn new(node_size: usize) -> Self {
        Self {
            nodes: Vec::new(),
            level_sizes: Vec::new(),
            node_size,
        }
    }

    /// Creates an R-tree from a list of feature bounding boxes
    pub fn build(feature_boxes: Vec<BoundingBox>, node_size: usize) -> Result<Self> {
        if feature_boxes.is_empty() {
            return Ok(Self::new(node_size));
        }

        // Calculate Hilbert values for sorting
        let mut items: Vec<(u64, BoundingBox)> = feature_boxes
            .iter()
            .map(|bbox| {
                let (cx, cy) = bbox.center();
                let hilbert = hilbert_index(cx, cy, 16); // 16-bit resolution
                (hilbert, *bbox)
            })
            .collect();

        // Sort by Hilbert value for spatial locality
        items.sort_by_key(|(h, _)| *h);

        // Build the tree bottom-up
        let mut current_level: Vec<Node> = items
            .iter()
            .enumerate()
            .map(|(i, (_, bbox))| Node::new(*bbox, i as u64))
            .collect();

        let mut all_nodes = Vec::new();
        let mut level_sizes = Vec::new();

        // Build levels until we reach the root
        while current_level.len() > 1 {
            level_sizes.push(current_level.len());
            let level_start = all_nodes.len();
            all_nodes.extend(current_level.clone());

            // Build parent level. Each parent's `offset` must be the ABSOLUTE
            // index of its first child within `all_nodes`, matching the
            // semantics `search()` relies on (and mirroring
            // `writer::build_rtree_with_offsets`). Storing a constant `0` here
            // would make `search()` treat every internal node's children as
            // starting at the front of the array.
            let mut parent_level = Vec::new();
            let mut child_idx = level_start;
            for chunk in current_level.chunks(node_size) {
                let mut parent_bbox = BoundingBox::empty();
                for node in chunk {
                    parent_bbox.expand(&node.bbox);
                }
                parent_level.push(Node::new(parent_bbox, child_idx as u64));
                child_idx += chunk.len();
            }

            current_level = parent_level;
        }

        // Add root
        if !current_level.is_empty() {
            level_sizes.push(1);
            all_nodes.extend(current_level);
        }

        Ok(Self {
            nodes: all_nodes,
            level_sizes,
            node_size,
        })
    }

    /// Searches the index for features intersecting the query box
    #[must_use]
    pub fn search(&self, query: &BoundingBox) -> Vec<u64> {
        let mut results = Vec::new();

        if self.nodes.is_empty() {
            return results;
        }

        // Start from root and traverse down
        let mut stack = Vec::new();
        let root_level = self.level_sizes.len() - 1;
        let root_offset = self.get_level_offset(root_level);
        stack.push((root_level, root_offset));

        while let Some((level, offset)) = stack.pop() {
            let node = &self.nodes[offset];

            if !node.bbox.intersects(query) {
                continue;
            }

            if level == 0 {
                // Leaf node - return its *node-array index* (which, since leaf
                // nodes occupy the front of `self.nodes`, equals the feature's
                // ordinal position). Consumers (`FlatGeobufReader::seek_feature`,
                // `HttpReader::read_feature_by_index`) dereference this index to
                // `self.nodes[idx].offset` to obtain the actual byte offset.
                results.push(offset as u64);
            } else {
                // Internal node. An internal node's `offset` field already holds
                // the ABSOLUTE index of its first child within `self.nodes`
                // (that is how both `build_rtree_with_offsets` and `build` write
                // it), so children are simply the contiguous slots
                // `first_child .. first_child + node_size`, bounded by the end
                // of the child level.
                let child_level = level - 1;
                let child_start = self.get_level_offset(child_level);
                let children_per_node = self.node_size;
                let first_child = node.offset as usize;
                let child_level_end = child_start + self.level_sizes[child_level];

                for i in 0..children_per_node {
                    let child_offset = first_child + i;
                    if child_offset < child_level_end {
                        stack.push((child_level, child_offset));
                    }
                }
            }
        }

        results
    }

    /// Gets the offset of the first node at the given level
    fn get_level_offset(&self, level: usize) -> usize {
        self.level_sizes[..level].iter().sum()
    }

    /// Reads the index from a reader
    pub fn read<R: Read>(reader: &mut R, num_features: u64) -> Result<Self> {
        if num_features == 0 {
            return Ok(Self::new(Self::DEFAULT_NODE_SIZE));
        }

        // Calculate total number of nodes
        let node_size = Self::DEFAULT_NODE_SIZE;
        let num_nodes = Self::calculate_node_count(num_features as usize, node_size);

        let mut nodes = Vec::with_capacity(num_nodes);
        for _ in 0..num_nodes {
            nodes.push(Node::read(reader)?);
        }

        // Calculate level sizes
        let level_sizes = Self::calculate_level_sizes(num_features as usize, node_size);

        Ok(Self {
            nodes,
            level_sizes,
            node_size,
        })
    }

    /// Writes the index to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        for node in &self.nodes {
            node.write(writer)?;
        }
        Ok(())
    }

    /// Calculates the total number of nodes in the tree
    pub(crate) fn calculate_node_count(num_features: usize, node_size: usize) -> usize {
        let mut count = num_features;
        let mut total = 0;
        while count > 1 {
            total += count;
            count = count.div_ceil(node_size);
        }
        total + count
    }

    /// Calculates the size of each level
    fn calculate_level_sizes(num_features: usize, node_size: usize) -> Vec<usize> {
        let mut sizes = Vec::new();
        let mut count = num_features;
        while count > 1 {
            sizes.push(count);
            count = count.div_ceil(node_size);
        }
        sizes.push(1); // Root
        sizes
    }

    /// Returns the size of the index in bytes
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        self.nodes.len() * Node::NODE_SIZE
    }
}

/// Computes Hilbert curve index for 2D point using geographic normalization.
///
/// This uses the iterative algorithm to compute the Hilbert index
/// for a point at the given (x, y) coordinates with the specified bit resolution.
/// Coordinates are assumed to be in geographic space (lon -180..180, lat -90..90).
fn hilbert_index(x: f64, y: f64, bits: u32) -> u64 {
    // Normalize to [0, 2^bits)
    let max_val = (1u64 << bits) - 1;
    let x_norm = ((x + 180.0) / 360.0 * max_val as f64).clamp(0.0, max_val as f64) as u64;
    let y_norm = ((y + 90.0) / 180.0 * max_val as f64).clamp(0.0, max_val as f64) as u64;

    xy_to_hilbert(x_norm, y_norm, bits)
}

/// Computes Hilbert curve index for a 2D point normalized over a provided bounding box.
///
/// This normalizes coordinates relative to the given bounding box, then maps
/// them onto a 16-bit Hilbert curve. Used by the writer to sort features by
/// their spatial position relative to the global extent.
pub fn hilbert_index_for_bbox(x: f64, y: f64, bbox: &BoundingBox) -> u64 {
    const BITS: u32 = 16;
    let max_val = (1u64 << BITS) - 1;

    let width = bbox.max_x - bbox.min_x;
    let height = bbox.max_y - bbox.min_y;

    // If the bbox has zero extent in a dimension, map everything to 0
    let x_norm = if width > 0.0 {
        ((x - bbox.min_x) / width * max_val as f64).clamp(0.0, max_val as f64) as u64
    } else {
        0
    };
    let y_norm = if height > 0.0 {
        ((y - bbox.min_y) / height * max_val as f64).clamp(0.0, max_val as f64) as u64
    } else {
        0
    };

    xy_to_hilbert(x_norm, y_norm, BITS)
}

/// Converts 2D coordinates to Hilbert curve index
fn xy_to_hilbert(mut x: u64, mut y: u64, bits: u32) -> u64 {
    let mut d = 0u64;
    let n = 1u64 << bits;

    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from((x & s) > 0);
        let ry = u64::from((y & s) > 0);
        d += s * s * ((3 * rx) ^ ry);

        // Rotate
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }

        s /= 2;
    }

    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        assert!(bbox.is_valid());
        assert_eq!(bbox.width(), 10.0);
        assert_eq!(bbox.height(), 10.0);
        assert_eq!(bbox.area(), 100.0);
        assert_eq!(bbox.center(), (5.0, 5.0));
    }

    #[test]
    fn test_bbox_intersection() {
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 15.0, 15.0);
        let bbox3 = BoundingBox::new(20.0, 20.0, 30.0, 30.0);

        assert!(bbox1.intersects(&bbox2));
        assert!(bbox2.intersects(&bbox1));
        assert!(!bbox1.intersects(&bbox3));
    }

    #[test]
    fn test_bbox_contains() {
        let outer = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let inner = BoundingBox::new(2.0, 2.0, 8.0, 8.0);

        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
    }

    #[test]
    fn test_bbox_expand() {
        let mut bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 15.0, 15.0);

        bbox1.expand(&bbox2);
        assert_eq!(bbox1.min_x, 0.0);
        assert_eq!(bbox1.min_y, 0.0);
        assert_eq!(bbox1.max_x, 15.0);
        assert_eq!(bbox1.max_y, 15.0);
    }

    #[test]
    fn test_hilbert_index() {
        // Test some known Hilbert values
        let h1 = hilbert_index(-180.0, -90.0, 8);
        let h2 = hilbert_index(180.0, 90.0, 8);
        let h3 = hilbert_index(0.0, 0.0, 8);

        // These should be different
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }

    #[test]
    fn test_rtree_build() {
        let boxes = vec![
            BoundingBox::new(0.0, 0.0, 1.0, 1.0),
            BoundingBox::new(1.0, 1.0, 2.0, 2.0),
            BoundingBox::new(2.0, 2.0, 3.0, 3.0),
        ];

        let rtree = PackedRTree::build(boxes, 2).ok();
        assert!(rtree.is_some());
        let rtree = rtree.expect("rtree build failed");
        assert!(!rtree.nodes.is_empty());
    }

    #[test]
    fn test_rtree_search() {
        let boxes = vec![
            BoundingBox::new(0.0, 0.0, 1.0, 1.0),
            BoundingBox::new(5.0, 5.0, 6.0, 6.0),
            BoundingBox::new(10.0, 10.0, 11.0, 11.0),
        ];

        let rtree = PackedRTree::build(boxes, 2).ok();
        assert!(rtree.is_some());
        let rtree = rtree.expect("rtree build failed");

        let query = BoundingBox::new(0.0, 0.0, 6.0, 6.0);
        let results = rtree.search(&query);

        // Should find first two boxes
        assert!(!results.is_empty());
    }

    #[test]
    fn test_node_size() {
        assert_eq!(Node::NODE_SIZE, 40);
    }

    /// Regression: a multi-level tree (>16 leaves with the default node size)
    /// must return *every* leaf for a full-extent query. The previous
    /// child-offset formula re-multiplied the already-absolute child index by
    /// `node_size`, pushing the computed offset out of range for any internal
    /// node whose rank was greater than 0, so all children of non-first parents
    /// were silently dropped and `search()` returned far fewer than all leaves.
    #[test]
    fn test_rtree_search_multilevel_returns_all_leaves() {
        // 40 leaves, node_size 16 -> level sizes [40, 3, 1] (two internal
        // levels, with parents at ranks 0/1/2 -> the bug's trigger condition).
        let node_size = PackedRTree::DEFAULT_NODE_SIZE;
        let n = 40usize;
        let boxes: Vec<BoundingBox> = (0..n)
            .map(|i| {
                let cx = (i as f64) * 4.0 - 78.0; // spread across valid lon range
                let cy = (i as f64) * 2.0 - 39.0; // valid lat range
                BoundingBox::new(cx - 0.25, cy - 0.25, cx + 0.25, cy + 0.25)
            })
            .collect();

        let rtree = PackedRTree::build(boxes, node_size).expect("build rtree");
        assert!(rtree.level_sizes.len() >= 3, "expected >=2 internal levels");

        // Full-extent query must reach every leaf.
        let query = BoundingBox::new(-180.0, -90.0, 180.0, 90.0);
        let mut results = rtree.search(&query);
        results.sort_unstable();

        let expected: Vec<u64> = (0..n as u64).collect();
        assert_eq!(
            results, expected,
            "full-extent search must return all leaf node indices exactly once"
        );
    }

    /// Regression: a query that only covers part of the extent must still return
    /// the correct leaves via the corrected internal-node traversal (exercising
    /// parents beyond rank 0).
    #[test]
    fn test_rtree_search_partial_extent_subset() {
        let node_size = PackedRTree::DEFAULT_NODE_SIZE;
        let n = 40usize;
        let centers: Vec<(f64, f64)> = (0..n)
            .map(|i| ((i as f64) * 4.0 - 78.0, (i as f64) * 2.0 - 39.0))
            .collect();
        let boxes: Vec<BoundingBox> = centers
            .iter()
            .map(|&(cx, cy)| BoundingBox::new(cx - 0.25, cy - 0.25, cx + 0.25, cy + 0.25))
            .collect();

        let rtree = PackedRTree::build(boxes, node_size).expect("build rtree");

        // Query a window covering roughly the upper third of the centers.
        let query = BoundingBox::new(30.0, 15.0, 200.0, 100.0);
        let results = rtree.search(&query);

        // Every returned index must be a valid leaf whose bbox actually
        // intersects the query (no false positives, no out-of-range indices).
        for &idx in &results {
            let node = &rtree.nodes[idx as usize];
            assert!(idx < n as u64, "leaf index in range");
            assert!(
                node.bbox.intersects(&query),
                "returned leaf {idx} must intersect the query"
            );
        }

        // And every leaf that intersects the query must be returned.
        let expected_hits: usize = centers
            .iter()
            .filter(|&&(cx, cy)| {
                let b = BoundingBox::new(cx - 0.25, cy - 0.25, cx + 0.25, cy + 0.25);
                b.intersects(&query)
            })
            .count();
        assert_eq!(
            results.len(),
            expected_hits,
            "search must return exactly the intersecting leaves"
        );
        assert!(expected_hits > 0, "test window should hit some features");
    }
}
