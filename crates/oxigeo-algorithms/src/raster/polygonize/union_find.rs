//! Union-Find (Disjoint Set Union) data structure for connected component labeling
//!
//! Implements a weighted union-find with path compression and union by rank,
//! giving near-O(1) amortized time per find/union operation.

/// A union-find data structure for efficient connected component labeling.
///
/// Uses path compression on `find()` and union by rank on `union()`.
/// Labels are `u32` values; label 0 is reserved for nodata/background.
#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    /// Parent array. `parent[i]` is the parent of node `i`.
    /// A node is a root when `parent[i] == i`.
    parent: Vec<u32>,
    /// Rank array for union by rank heuristic.
    rank: Vec<u8>,
}

impl UnionFind {
    /// Creates a new union-find with `capacity` elements (0..capacity-1).
    ///
    /// Each element is initially its own representative (self-parent).
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let mut parent = Vec::with_capacity(capacity);
        for i in 0..capacity {
            parent.push(i as u32);
        }
        let rank = vec![0u8; capacity];
        Self { parent, rank }
    }

    /// Ensures the union-find can hold at least `label` elements,
    /// extending with self-parents and zero ranks as needed.
    pub(crate) fn ensure_label(&mut self, label: u32) {
        let needed = label as usize + 1;
        if needed > self.parent.len() {
            self.parent.reserve(needed - self.parent.len());
            self.rank.reserve(needed - self.rank.len());
            for i in self.parent.len()..needed {
                self.parent.push(i as u32);
                self.rank.push(0);
            }
        }
    }

    /// Finds the representative (root) of the set containing `x`,
    /// applying path compression.
    pub(crate) fn find(&mut self, x: u32) -> u32 {
        let mut root = x;
        // Walk to the root
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        // Path compression: point everything along the path to the root
        let mut current = x;
        while current != root {
            let next = self.parent[current as usize];
            self.parent[current as usize] = root;
            current = next;
        }
        root
    }

    /// Merges the sets containing `a` and `b` using union by rank.
    ///
    /// Returns the representative of the merged set.
    pub(crate) fn union(&mut self, a: u32, b: u32) -> u32 {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a == root_b {
            return root_a;
        }
        let rank_a = self.rank[root_a as usize];
        let rank_b = self.rank[root_b as usize];
        if rank_a < rank_b {
            self.parent[root_a as usize] = root_b;
            root_b
        } else if rank_a > rank_b {
            self.parent[root_b as usize] = root_a;
            root_a
        } else {
            self.parent[root_b as usize] = root_a;
            self.rank[root_a as usize] = rank_a.saturating_add(1);
            root_a
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let uf = UnionFind::with_capacity(5);
        assert_eq!(uf.parent.len(), 5);
        for i in 0..5 {
            assert_eq!(uf.parent[i], i as u32);
            assert_eq!(uf.rank[i], 0);
        }
    }

    #[test]
    fn test_find_self() {
        let mut uf = UnionFind::with_capacity(5);
        for i in 0..5u32 {
            assert_eq!(uf.find(i), i);
        }
    }

    #[test]
    fn test_union_basic() {
        let mut uf = UnionFind::with_capacity(5);
        let root = uf.union(0, 1);
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(0), root);
    }

    #[test]
    fn test_union_transitivity() {
        let mut uf = UnionFind::with_capacity(5);
        uf.union(0, 1);
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(2));
    }

    #[test]
    fn test_union_idempotent() {
        let mut uf = UnionFind::with_capacity(5);
        let root1 = uf.union(0, 1);
        let root2 = uf.union(0, 1);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_separate_components() {
        let mut uf = UnionFind::with_capacity(6);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_ne!(uf.find(0), uf.find(2));
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(2), uf.find(3));
    }

    #[test]
    fn test_union_by_rank() {
        let mut uf = UnionFind::with_capacity(8);
        // Build a chain: {0,1}, {2,3}, then merge
        uf.union(0, 1);
        uf.union(2, 3);
        uf.union(0, 2);
        // All four should share a root
        let root = uf.find(0);
        assert_eq!(uf.find(1), root);
        assert_eq!(uf.find(2), root);
        assert_eq!(uf.find(3), root);
    }

    #[test]
    fn test_path_compression() {
        let mut uf = UnionFind::with_capacity(10);
        // Create a long chain: 0->1->2->3->4
        for i in 0..4u32 {
            uf.parent[i as usize] = i + 1;
        }
        // find(0) should compress the entire path
        let root = uf.find(0);
        assert_eq!(root, 4);
        // After compression, all nodes should point directly to root
        assert_eq!(uf.parent[0], 4);
        assert_eq!(uf.parent[1], 4);
        assert_eq!(uf.parent[2], 4);
        assert_eq!(uf.parent[3], 4);
    }

    #[test]
    fn test_ensure_label() {
        let mut uf = UnionFind::with_capacity(3);
        assert_eq!(uf.parent.len(), 3);
        uf.ensure_label(9);
        assert_eq!(uf.parent.len(), 10);
        // New labels should be self-parents
        for i in 3..10u32 {
            assert_eq!(uf.parent[i as usize], i);
            assert_eq!(uf.rank[i as usize], 0);
        }
    }

    #[test]
    fn test_ensure_label_no_shrink() {
        let mut uf = UnionFind::with_capacity(10);
        uf.ensure_label(5); // already covered
        assert_eq!(uf.parent.len(), 10);
    }

    #[test]
    fn test_large_union_find() {
        let n = 10_000;
        let mut uf = UnionFind::with_capacity(n);
        // Union all elements into one set
        for i in 1..n as u32 {
            uf.union(0, i);
        }
        let root = uf.find(0);
        for i in 0..n as u32 {
            assert_eq!(uf.find(i), root);
        }
    }

    #[test]
    fn test_rank_stays_bounded() {
        let mut uf = UnionFind::with_capacity(16);
        // Union in a balanced binary tree pattern
        for step in [1, 2, 4, 8] {
            let mut i = 0;
            while i + step < 16 {
                uf.union(i as u32, (i + step) as u32);
                i += step * 2;
            }
        }
        // Rank should never exceed log2(n) ~ 4 for 16 elements
        for &r in &uf.rank {
            assert!(r <= 4, "rank {r} exceeds expected bound for 16 elements");
        }
    }
}
