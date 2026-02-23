//! Simple R-tree implementation for spatial indexing

use oxigdal_core::types::BoundingBox as OxiGdalBBox;

/// Bounding box for R-tree
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum X
    pub min_x: f64,
    /// Minimum Y
    pub min_y: f64,
    /// Maximum X
    pub max_x: f64,
    /// Maximum Y
    pub max_y: f64,
}

impl BoundingBox {
    /// Creates a new bounding box
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Creates from oxigdal BoundingBox
    pub fn from_oxigdal(bbox: &OxiGdalBBox) -> Self {
        Self::new(bbox.min_x(), bbox.min_y(), bbox.max_x(), bbox.max_y())
    }

    /// Returns true if this bbox intersects another
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Returns true if this bbox contains another
    pub fn contains(&self, other: &Self) -> bool {
        self.min_x <= other.min_x
            && self.max_x >= other.max_x
            && self.min_y <= other.min_y
            && self.max_y >= other.max_y
    }

    /// Expands this bbox to include another
    pub fn expand(&mut self, other: &Self) {
        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
    }

    /// Returns the area of this bbox
    pub fn area(&self) -> f64 {
        (self.max_x - self.min_x) * (self.max_y - self.min_y)
    }

    /// Returns the center of this bbox
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }
}

/// R-tree node
#[derive(Debug, Clone)]
enum RTreeNode {
    /// Leaf node containing actual items
    Leaf {
        bbox: BoundingBox,
        items: Vec<RTreeEntry>,
    },
    /// Internal node containing child nodes
    Internal {
        bbox: BoundingBox,
        children: Vec<RTreeNode>,
    },
}

impl RTreeNode {
    /// Returns the bounding box of this node
    fn bbox(&self) -> &BoundingBox {
        match self {
            Self::Leaf { bbox, .. } => bbox,
            Self::Internal { bbox, .. } => bbox,
        }
    }

    /// Returns true if this is a leaf node
    fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf { .. })
    }

    /// Queries this node for items intersecting the given bbox
    fn query(&self, query_bbox: &BoundingBox, results: &mut Vec<usize>) {
        if !self.bbox().intersects(query_bbox) {
            return;
        }

        match self {
            Self::Leaf { items, .. } => {
                for entry in items {
                    if entry.bbox.intersects(query_bbox) {
                        results.push(entry.id);
                    }
                }
            }
            Self::Internal { children, .. } => {
                for child in children {
                    child.query(query_bbox, results);
                }
            }
        }
    }
}

/// R-tree entry (leaf item)
#[derive(Debug, Clone)]
struct RTreeEntry {
    id: usize,
    bbox: BoundingBox,
}

/// Simple R-tree spatial index
///
/// This is a basic R-tree implementation optimized for bulk loading.
/// For production use, consider using a specialized R-tree library.
pub struct RTreeIndex {
    root: Option<RTreeNode>,
    max_entries: usize,
}

impl RTreeIndex {
    /// Creates a new R-tree index
    pub fn new() -> Self {
        Self {
            root: None,
            max_entries: 16,
        }
    }

    /// Creates a new R-tree with custom max entries per node
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            root: None,
            max_entries,
        }
    }

    /// Inserts an item into the index
    pub fn insert(&mut self, id: usize, bbox: BoundingBox) {
        let entry = RTreeEntry { id, bbox };

        match &mut self.root {
            None => {
                // Create first leaf node
                self.root = Some(RTreeNode::Leaf {
                    bbox,
                    items: vec![entry],
                });
            }
            Some(root) => {
                // Add to existing tree (simple bulk loading)
                if let RTreeNode::Leaf {
                    bbox: root_bbox,
                    items,
                } = root
                {
                    items.push(entry);
                    root_bbox.expand(&bbox);

                    // Split if necessary
                    if items.len() > self.max_entries {
                        self.split_root();
                    }
                }
            }
        }
    }

    /// Bulk loads entries into the index
    pub fn bulk_load(&mut self, entries: Vec<(usize, BoundingBox)>) {
        for (id, bbox) in entries {
            self.insert(id, bbox);
        }
    }

    /// Queries the index for items intersecting the given bbox
    pub fn query(&self, bbox: &BoundingBox) -> Vec<usize> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            root.query(bbox, &mut results);
        }
        results
    }

    /// Splits the root node when it becomes too large
    fn split_root(&mut self) {
        if let Some(RTreeNode::Leaf { items, .. }) = self.root.take() {
            // Simple split: divide items into two groups
            let mid = items.len() / 2;
            let (left_items, right_items) = items.split_at(mid);

            let left_bbox = Self::compute_bbox(left_items.iter().map(|e| &e.bbox));
            let right_bbox = Self::compute_bbox(right_items.iter().map(|e| &e.bbox));

            let left_node = RTreeNode::Leaf {
                bbox: left_bbox,
                items: left_items.to_vec(),
            };

            let right_node = RTreeNode::Leaf {
                bbox: right_bbox,
                items: right_items.to_vec(),
            };

            let mut root_bbox = left_bbox;
            root_bbox.expand(&right_bbox);

            self.root = Some(RTreeNode::Internal {
                bbox: root_bbox,
                children: vec![left_node, right_node],
            });
        }
    }

    /// Computes the combined bounding box of multiple bboxes
    fn compute_bbox<'a>(bboxes: impl Iterator<Item = &'a BoundingBox>) -> BoundingBox {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for bbox in bboxes {
            min_x = min_x.min(bbox.min_x);
            min_y = min_y.min(bbox.min_y);
            max_x = max_x.max(bbox.max_x);
            max_y = max_y.max(bbox.max_y);
        }

        BoundingBox::new(min_x, min_y, max_x, max_y)
    }

    /// Returns true if the index is empty
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }
}

impl Default for RTreeIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_operations() {
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 15.0, 15.0);
        let bbox3 = BoundingBox::new(20.0, 20.0, 30.0, 30.0);

        assert!(bbox1.intersects(&bbox2));
        assert!(!bbox1.intersects(&bbox3));

        assert_eq!(bbox1.area(), 100.0);
        assert_eq!(bbox1.center(), (5.0, 5.0));
    }

    #[test]
    fn test_bbox_expand() {
        let mut bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 15.0, 15.0);

        bbox1.expand(&bbox2);
        assert_eq!(bbox1.min_x, 0.0);
        assert_eq!(bbox1.max_x, 15.0);
    }

    #[test]
    fn test_rtree_insertion_and_query() {
        let mut rtree = RTreeIndex::new();

        rtree.insert(0, BoundingBox::new(0.0, 0.0, 10.0, 10.0));
        rtree.insert(1, BoundingBox::new(10.0, 10.0, 20.0, 20.0));
        rtree.insert(2, BoundingBox::new(20.0, 20.0, 30.0, 30.0));

        let query_bbox = BoundingBox::new(5.0, 5.0, 15.0, 15.0);
        let results = rtree.query(&query_bbox);

        assert_eq!(results.len(), 2);
        assert!(results.contains(&0));
        assert!(results.contains(&1));
    }

    #[test]
    fn test_rtree_bulk_load() {
        let mut rtree = RTreeIndex::new();

        let entries = vec![
            (0, BoundingBox::new(0.0, 0.0, 10.0, 10.0)),
            (1, BoundingBox::new(10.0, 10.0, 20.0, 20.0)),
            (2, BoundingBox::new(20.0, 20.0, 30.0, 30.0)),
        ];

        rtree.bulk_load(entries);

        let query_bbox = BoundingBox::new(15.0, 15.0, 25.0, 25.0);
        let results = rtree.query(&query_bbox);

        assert!(!results.is_empty());
    }
}
