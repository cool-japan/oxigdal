//! In-memory octree spatial index for point cloud data.
//!
//! The [`Octree`] partitions 3-D space recursively using [`OctreeNode`] cells,
//! each of which stores up to `max_points_per_node` points before being split
//! into eight children.  The tree supports bbox / sphere / k-NN queries, per-
//! classification filtering, and voxel-grid downsampling.

use std::collections::HashMap;

use crate::point::{BoundingBox3D, Point3D};

// ---------------------------------------------------------------------------
// OctreeNode
// ---------------------------------------------------------------------------

/// A single node in the octree.
///
/// Leaf nodes hold points directly; internal nodes delegate to eight children.
pub struct OctreeNode {
    /// Spatial extent covered by this node.
    pub bounds: BoundingBox3D,
    /// Points stored in this node (empty once split into children).
    pub points: Vec<Point3D>,
    /// Eight children, present only after this node has been split.
    pub children: Option<Box<[OctreeNode; 8]>>,
    /// Depth of this node within the tree (root = 0).
    pub depth: u8,
}

impl OctreeNode {
    /// Create an empty leaf node at the given depth.
    pub fn new(bounds: BoundingBox3D, depth: u8) -> Self {
        Self {
            bounds,
            points: Vec::new(),
            children: None,
            depth,
        }
    }

    /// Total number of points in this subtree (recursive).
    pub fn point_count(&self) -> usize {
        match &self.children {
            None => self.points.len(),
            Some(children) => {
                self.points.len() + children.iter().map(|c| c.point_count()).sum::<usize>()
            }
        }
    }

    /// `true` when this node has no children (i.e. is a leaf).
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children.is_none()
    }

    /// Maximum depth reached anywhere in this subtree.
    pub fn depth_max(&self) -> u8 {
        match &self.children {
            None => self.depth,
            Some(children) => children
                .iter()
                .map(|c| c.depth_max())
                .max()
                .unwrap_or(self.depth),
        }
    }
}

// ---------------------------------------------------------------------------
// Octree
// ---------------------------------------------------------------------------

/// An in-memory octree index over a collection of [`Point3D`] values.
///
/// # Example
/// ```
/// use oxigdal_copc::point::{BoundingBox3D, Point3D};
/// use oxigdal_copc::octree::Octree;
///
/// let bounds = BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 100.0).unwrap();
/// let mut tree = Octree::new(bounds);
/// tree.insert(Point3D::new(10.0, 20.0, 5.0));
/// assert_eq!(tree.len(), 1);
/// ```
pub struct Octree {
    /// Root node of the tree.
    root: OctreeNode,
    /// Maximum number of points in a leaf before it is split.
    max_points_per_node: usize,
    /// Maximum subdivision depth (nodes at this depth are never split).
    max_depth: u8,
    /// Cached total point count.
    total_points: usize,
}

impl Octree {
    /// Create a new empty octree covering `bounds`.
    ///
    /// Defaults: `max_points_per_node = 64`, `max_depth = 16`.
    pub fn new(bounds: BoundingBox3D) -> Self {
        Self {
            root: OctreeNode::new(bounds, 0),
            max_points_per_node: 64,
            max_depth: 16,
            total_points: 0,
        }
    }

    /// Builder: override the maximum points-per-leaf threshold.
    pub fn with_max_points(mut self, n: usize) -> Self {
        self.max_points_per_node = n;
        self
    }

    /// Builder: override the maximum tree depth.
    pub fn with_max_depth(mut self, depth: u8) -> Self {
        self.max_depth = depth;
        self
    }

    /// Total number of points stored in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.total_points
    }

    /// `true` when the tree contains no points.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_points == 0
    }

    /// Insert a single point.
    ///
    /// Points that fall outside the root bounds are silently dropped — ensure
    /// the tree was constructed with a bounding box that covers the data.
    pub fn insert(&mut self, point: Point3D) {
        if !self.root.bounds.contains(&point) {
            return;
        }
        let max_pts = self.max_points_per_node;
        let max_d = self.max_depth;
        Self::node_insert(&mut self.root, point, max_pts, max_d);
        self.total_points += 1;
    }

    /// Insert a batch of points efficiently.
    pub fn insert_batch(&mut self, points: Vec<Point3D>) {
        for p in points {
            self.insert(p);
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Find all points inside `bbox`.
    pub fn query_bbox<'a>(&'a self, bbox: &BoundingBox3D) -> Vec<&'a Point3D> {
        let mut result = Vec::new();
        Self::node_query_bbox(&self.root, bbox, &mut result);
        result
    }

    /// Find all points within `radius` of centre `(cx, cy, cz)`.
    pub fn query_sphere(&self, cx: f64, cy: f64, cz: f64, radius: f64) -> Vec<&Point3D> {
        let r2 = radius * radius;
        // Build a bounding box for the sphere to prune branches.
        let sphere_bbox = BoundingBox3D {
            min_x: cx - radius,
            min_y: cy - radius,
            min_z: cz - radius,
            max_x: cx + radius,
            max_y: cy + radius,
            max_z: cz + radius,
        };
        let mut result = Vec::new();
        Self::node_query_sphere(&self.root, cx, cy, cz, r2, &sphere_bbox, &mut result);
        result
    }

    /// Find the `k` nearest neighbours to `(cx, cy, cz)`.
    ///
    /// Returns at most `min(k, self.len())` entries sorted by ascending distance.
    pub fn k_nearest(&self, cx: f64, cy: f64, cz: f64, k: usize) -> Vec<(&Point3D, f64)> {
        if k == 0 || self.is_empty() {
            return Vec::new();
        }

        // Heap-based k-NN search: we keep a max-heap of size k so that we can
        // prune entire octants whose minimum possible distance exceeds the
        // current k-th best.
        //
        // For simplicity we collect all points via a full traversal (the octree
        // already prunes many octants via `node_min_dist_sq`), then sort.
        let mut candidates: Vec<(&Point3D, f64)> = Vec::new();
        Self::node_collect_knn(&self.root, cx, cy, cz, k, f64::INFINITY, &mut candidates);

        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(k);
        candidates
    }

    /// Return all points whose [`Point3D::classification`] equals `class`.
    pub fn by_classification(&self, class: u8) -> Vec<&Point3D> {
        let mut result = Vec::new();
        Self::node_collect_by_class(&self.root, class, &mut result);
        result
    }

    /// Compute aggregate statistics over all points in the tree.
    pub fn stats(&self) -> PointCloudStats {
        let mut all_points: Vec<&Point3D> = Vec::with_capacity(self.total_points);
        Self::node_collect_all(&self.root, &mut all_points);

        if all_points.is_empty() {
            return PointCloudStats {
                count: 0,
                bounds: None,
                mean_z: 0.0,
                std_z: 0.0,
                density: 0.0,
                classification_counts: HashMap::new(),
            };
        }

        let count = all_points.len();
        let sum_z: f64 = all_points.iter().map(|p| p.z).sum();
        let mean_z = sum_z / count as f64;
        let variance_z: f64 = all_points
            .iter()
            .map(|p| {
                let d = p.z - mean_z;
                d * d
            })
            .sum::<f64>()
            / count as f64;
        let std_z = variance_z.sqrt();

        let mut classification_counts: HashMap<u8, usize> = HashMap::new();
        for p in &all_points {
            *classification_counts.entry(p.classification).or_insert(0) += 1;
        }

        // Build bounding box from collected points.
        let bounds = {
            let first = all_points[0];
            let (mut min_x, mut min_y, mut min_z) = (first.x, first.y, first.z);
            let (mut max_x, mut max_y, mut max_z) = (first.x, first.y, first.z);
            for p in all_points.iter().skip(1) {
                if p.x < min_x {
                    min_x = p.x;
                }
                if p.y < min_y {
                    min_y = p.y;
                }
                if p.z < min_z {
                    min_z = p.z;
                }
                if p.x > max_x {
                    max_x = p.x;
                }
                if p.y > max_y {
                    max_y = p.y;
                }
                if p.z > max_z {
                    max_z = p.z;
                }
            }
            BoundingBox3D::new(min_x, min_y, min_z, max_x, max_y, max_z)
        };

        let density = if let Some(ref bb) = bounds {
            let dx = bb.max_x - bb.min_x;
            let dy = bb.max_y - bb.min_y;
            let xy_area = dx * dy;
            if xy_area > 0.0 {
                count as f64 / xy_area
            } else {
                0.0
            }
        } else {
            0.0
        };

        PointCloudStats {
            count,
            bounds,
            mean_z,
            std_z,
            density,
            classification_counts,
        }
    }

    /// Reduce the point cloud by retaining at most one point per axis-aligned
    /// voxel cell of size `voxel_size`.
    ///
    /// The retained point is the first one encountered in each voxel cell.
    /// Uses the tree's root bounds as the grid origin.
    pub fn voxel_downsample(&self, voxel_size: f64) -> Vec<Point3D> {
        if voxel_size <= 0.0 {
            return Vec::new();
        }

        let mut all_points: Vec<&Point3D> = Vec::with_capacity(self.total_points);
        Self::node_collect_all(&self.root, &mut all_points);

        let origin_x = self.root.bounds.min_x;
        let origin_y = self.root.bounds.min_y;
        let origin_z = self.root.bounds.min_z;

        let mut occupied: HashMap<(i64, i64, i64), ()> = HashMap::new();
        let mut result: Vec<Point3D> = Vec::new();

        for &p in &all_points {
            let ix = ((p.x - origin_x) / voxel_size).floor() as i64;
            let iy = ((p.y - origin_y) / voxel_size).floor() as i64;
            let iz = ((p.z - origin_z) / voxel_size).floor() as i64;
            let key = (ix, iy, iz);
            if occupied.insert(key, ()).is_none() {
                result.push((*p).clone());
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn node_insert(node: &mut OctreeNode, point: Point3D, max_pts: usize, max_d: u8) {
        if node.is_leaf() {
            node.points.push(point);
            // Split if over threshold and not at maximum depth.
            if node.points.len() > max_pts && node.depth < max_d {
                Self::split_node(node, max_pts, max_d);
            }
        } else if let Some(children) = node.children.as_mut() {
            // Route into the child whose bounds contain the point.
            let child_idx = children.iter().enumerate().find_map(|(i, c)| {
                if c.bounds.contains(&point) {
                    Some(i)
                } else {
                    None
                }
            });

            if let Some(idx) = child_idx {
                Self::node_insert(&mut children[idx], point, max_pts, max_d);
            } else {
                // Fallback: point lies on a shared boundary.
                node.points.push(point);
            }
        } else {
            node.points.push(point);
        }
    }

    fn split_node(node: &mut OctreeNode, max_pts: usize, max_d: u8) {
        let octants = node.bounds.split_octants();
        let next_depth = node.depth.saturating_add(1);

        // Safety: we're constructing exactly 8 nodes from 8 octants.
        let children: Box<[OctreeNode; 8]> = Box::new([
            OctreeNode::new(octants[0].clone(), next_depth),
            OctreeNode::new(octants[1].clone(), next_depth),
            OctreeNode::new(octants[2].clone(), next_depth),
            OctreeNode::new(octants[3].clone(), next_depth),
            OctreeNode::new(octants[4].clone(), next_depth),
            OctreeNode::new(octants[5].clone(), next_depth),
            OctreeNode::new(octants[6].clone(), next_depth),
            OctreeNode::new(octants[7].clone(), next_depth),
        ]);

        node.children = Some(children);

        // Redistribute existing points into children.
        let old_points = std::mem::take(&mut node.points);
        // Separate into those that fit a child and those that stay in the node.
        let mut overflow: Vec<Point3D> = Vec::new();
        if let Some(children_ref) = node.children.as_mut() {
            for p in old_points {
                // Find the first child that contains this point.
                let child_idx = children_ref
                    .iter()
                    .enumerate()
                    .find_map(|(i, c)| if c.bounds.contains(&p) { Some(i) } else { None });

                if let Some(idx) = child_idx {
                    Self::node_insert(&mut children_ref[idx], p, max_pts, max_d);
                } else {
                    overflow.push(p);
                }
            }
        }
        node.points.extend(overflow);
    }

    fn node_query_bbox<'a>(
        node: &'a OctreeNode,
        bbox: &BoundingBox3D,
        result: &mut Vec<&'a Point3D>,
    ) {
        if !node.bounds.intersects_3d(bbox) {
            return;
        }
        for p in &node.points {
            if bbox.contains(p) {
                result.push(p);
            }
        }
        if let Some(children) = &node.children {
            for child in children.iter() {
                Self::node_query_bbox(child, bbox, result);
            }
        }
    }

    fn node_query_sphere<'a>(
        node: &'a OctreeNode,
        cx: f64,
        cy: f64,
        cz: f64,
        r2: f64,
        sphere_bbox: &BoundingBox3D,
        result: &mut Vec<&'a Point3D>,
    ) {
        if !node.bounds.intersects_3d(sphere_bbox) {
            return;
        }
        for p in &node.points {
            let dx = p.x - cx;
            let dy = p.y - cy;
            let dz = p.z - cz;
            if dx * dx + dy * dy + dz * dz <= r2 {
                result.push(p);
            }
        }
        if let Some(children) = &node.children {
            for child in children.iter() {
                Self::node_query_sphere(child, cx, cy, cz, r2, sphere_bbox, result);
            }
        }
    }

    /// Collect candidate points for k-NN using the current best-known
    /// distance as a pruning bound.
    fn node_collect_knn<'a>(
        node: &'a OctreeNode,
        cx: f64,
        cy: f64,
        cz: f64,
        k: usize,
        mut best_dist: f64,
        result: &mut Vec<(&'a Point3D, f64)>,
    ) {
        // Prune if the closest possible point in this node is farther than
        // the current worst among the k-best.
        if Self::node_min_dist_sq(node, cx, cy, cz) > best_dist * best_dist {
            return;
        }
        for p in &node.points {
            let dx = p.x - cx;
            let dy = p.y - cy;
            let dz = p.z - cz;
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            result.push((p, dist));
            // Lazily update best_dist once we have k candidates.
            if result.len() >= k {
                // Keep partial max to tighten prune bound.
                if let Some(&(_, d)) = result
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                {
                    if d < best_dist {
                        best_dist = d;
                    }
                }
            }
        }
        if let Some(children) = &node.children {
            for child in children.iter() {
                Self::node_collect_knn(child, cx, cy, cz, k, best_dist, result);
            }
        }
    }

    /// Squared minimum distance from the point `(cx,cy,cz)` to the nearest
    /// point on or inside `node.bounds`.
    fn node_min_dist_sq(node: &OctreeNode, cx: f64, cy: f64, cz: f64) -> f64 {
        let b = &node.bounds;
        let dx = if cx < b.min_x {
            b.min_x - cx
        } else if cx > b.max_x {
            cx - b.max_x
        } else {
            0.0
        };
        let dy = if cy < b.min_y {
            b.min_y - cy
        } else if cy > b.max_y {
            cy - b.max_y
        } else {
            0.0
        };
        let dz = if cz < b.min_z {
            b.min_z - cz
        } else if cz > b.max_z {
            cz - b.max_z
        } else {
            0.0
        };
        dx * dx + dy * dy + dz * dz
    }

    fn node_collect_by_class<'a>(node: &'a OctreeNode, class: u8, result: &mut Vec<&'a Point3D>) {
        for p in &node.points {
            if p.classification == class {
                result.push(p);
            }
        }
        if let Some(children) = &node.children {
            for child in children.iter() {
                Self::node_collect_by_class(child, class, result);
            }
        }
    }

    fn node_collect_all<'a>(node: &'a OctreeNode, result: &mut Vec<&'a Point3D>) {
        result.extend(node.points.iter());
        if let Some(children) = &node.children {
            for child in children.iter() {
                Self::node_collect_all(child, result);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PointCloudStats
// ---------------------------------------------------------------------------

/// Aggregate statistics over a point cloud.
pub struct PointCloudStats {
    /// Total number of points.
    pub count: usize,
    /// Tight bounding box of all points, or `None` if the cloud is empty.
    pub bounds: Option<BoundingBox3D>,
    /// Mean Z value.
    pub mean_z: f64,
    /// Population standard deviation of Z values.
    pub std_z: f64,
    /// Point density: points per unit area (XY plane).
    pub density: f64,
    /// Number of points per ASPRS classification code.
    pub classification_counts: HashMap<u8, usize>,
}
