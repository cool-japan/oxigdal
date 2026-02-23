//! Merkle tree implementation for efficient change detection
//!
//! Merkle trees allow efficient comparison of datasets by computing
//! hierarchical hashes. This enables quick detection of differences
//! without transmitting entire datasets.

use crate::{SyncError, SyncResult};
use serde::{Deserialize, Serialize};

/// A hash value
pub type Hash = [u8; 32];

/// Merkle tree node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum MerkleNode {
    /// Leaf node containing data hash
    Leaf {
        /// Hash of the data
        hash: Hash,
    },
    /// Internal node with child hashes
    Internal {
        /// Hash of combined child hashes
        hash: Hash,
        /// Left child
        left: Box<MerkleNode>,
        /// Right child
        right: Box<MerkleNode>,
    },
}

impl MerkleNode {
    /// Gets the hash of this node
    fn hash(&self) -> &Hash {
        match self {
            MerkleNode::Leaf { hash } => hash,
            MerkleNode::Internal { hash, .. } => hash,
        }
    }
}

/// Merkle tree for efficient data synchronization
///
/// A Merkle tree is a tree where each non-leaf node is labeled with the
/// hash of its children. This allows efficient verification and comparison
/// of large datasets.
///
/// # Example
///
/// ```rust
/// use oxigdal_sync::merkle::MerkleTree;
///
/// let data = vec![
///     b"block1".to_vec(),
///     b"block2".to_vec(),
///     b"block3".to_vec(),
///     b"block4".to_vec(),
/// ];
///
/// let tree = MerkleTree::from_data(data).ok();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleTree {
    /// Root node of the tree
    root: Option<MerkleNode>,
    /// Number of leaves
    leaf_count: usize,
}

impl MerkleTree {
    /// Creates an empty Merkle tree
    pub fn new() -> Self {
        Self {
            root: None,
            leaf_count: 0,
        }
    }

    /// Creates a Merkle tree from data blocks
    ///
    /// # Arguments
    ///
    /// * `data` - Vector of data blocks
    ///
    /// # Returns
    ///
    /// A Merkle tree constructed from the data
    pub fn from_data(data: Vec<Vec<u8>>) -> SyncResult<Self> {
        if data.is_empty() {
            return Ok(Self::new());
        }

        let leaf_count = data.len();
        let leaves: Vec<MerkleNode> = data
            .into_iter()
            .map(|block| {
                let hash = Self::hash_data(&block);
                MerkleNode::Leaf { hash }
            })
            .collect();

        let root = Self::build_tree(leaves)?;

        Ok(Self {
            root: Some(root),
            leaf_count,
        })
    }

    /// Builds a tree from leaf nodes
    fn build_tree(mut nodes: Vec<MerkleNode>) -> SyncResult<MerkleNode> {
        if nodes.is_empty() {
            return Err(SyncError::MerkleVerificationFailed(
                "Cannot build tree from empty nodes".to_string(),
            ));
        }

        while nodes.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in nodes.chunks(2) {
                match chunk {
                    [left, right] => {
                        let combined = Self::combine_hashes(left.hash(), right.hash());
                        next_level.push(MerkleNode::Internal {
                            hash: combined,
                            left: Box::new(left.clone()),
                            right: Box::new(right.clone()),
                        });
                    }
                    [single] => {
                        // Odd number of nodes - promote the single node
                        next_level.push(single.clone());
                    }
                    _ => unreachable!(),
                }
            }

            nodes = next_level;
        }

        nodes
            .into_iter()
            .next()
            .ok_or_else(|| SyncError::MerkleVerificationFailed("Failed to build tree".to_string()))
    }

    /// Hashes a data block
    fn hash_data(data: &[u8]) -> Hash {
        let hash = blake3::hash(data);
        *hash.as_bytes()
    }

    /// Combines two hashes into one
    fn combine_hashes(left: &Hash, right: &Hash) -> Hash {
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(left);
        combined.extend_from_slice(right);
        Self::hash_data(&combined)
    }

    /// Gets the root hash
    ///
    /// # Returns
    ///
    /// The root hash, or None if the tree is empty
    pub fn root_hash(&self) -> Option<&Hash> {
        self.root.as_ref().map(|node| node.hash())
    }

    /// Gets the number of leaves
    pub fn leaf_count(&self) -> usize {
        self.leaf_count
    }

    /// Checks if the tree is empty
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Compares this tree with another to find differences
    ///
    /// # Arguments
    ///
    /// * `other` - The tree to compare with
    ///
    /// # Returns
    ///
    /// Indices of differing leaves
    pub fn diff(&self, other: &MerkleTree) -> Vec<usize> {
        let mut differences = Vec::new();

        if let (Some(self_root), Some(other_root)) = (&self.root, &other.root) {
            Self::diff_nodes(self_root, other_root, 0, &mut differences);
        }

        differences
    }

    /// Recursively finds differences between nodes
    fn diff_nodes(
        self_node: &MerkleNode,
        other_node: &MerkleNode,
        index: usize,
        differences: &mut Vec<usize>,
    ) {
        if self_node.hash() == other_node.hash() {
            // Nodes are identical
            return;
        }

        match (self_node, other_node) {
            (MerkleNode::Leaf { .. }, MerkleNode::Leaf { .. }) => {
                differences.push(index);
            }
            (
                MerkleNode::Internal {
                    left: l1,
                    right: r1,
                    ..
                },
                MerkleNode::Internal {
                    left: l2,
                    right: r2,
                    ..
                },
            ) => {
                Self::diff_nodes(l1, l2, index * 2, differences);
                Self::diff_nodes(r1, r2, index * 2 + 1, differences);
            }
            _ => {
                // Trees have different structures
                differences.push(index);
            }
        }
    }

    /// Verifies data against the tree
    ///
    /// # Arguments
    ///
    /// * `data` - The data blocks to verify
    ///
    /// # Returns
    ///
    /// True if the data matches the tree
    pub fn verify(&self, data: &[Vec<u8>]) -> SyncResult<bool> {
        if data.len() != self.leaf_count {
            return Ok(false);
        }

        let verification_tree = Self::from_data(data.to_vec())?;

        Ok(self.root_hash() == verification_tree.root_hash())
    }

    /// Gets a proof for a specific leaf
    ///
    /// Returns the sibling hashes needed to reconstruct the root hash
    ///
    /// # Arguments
    ///
    /// * `index` - The leaf index
    ///
    /// # Returns
    ///
    /// Vector of sibling hashes (proof path)
    pub fn get_proof(&self, index: usize) -> SyncResult<Vec<Hash>> {
        if index >= self.leaf_count {
            return Err(SyncError::MerkleVerificationFailed(
                "Index out of bounds".to_string(),
            ));
        }

        let mut proof = Vec::new();

        if let Some(root) = &self.root {
            Self::collect_proof(root, index, &mut proof)?;
        }

        Ok(proof)
    }

    /// Recursively collects proof hashes
    fn collect_proof(node: &MerkleNode, index: usize, proof: &mut Vec<Hash>) -> SyncResult<bool> {
        match node {
            MerkleNode::Leaf { .. } => Ok(true),
            MerkleNode::Internal { left, right, .. } => {
                let left_leaves = Self::count_leaves(left);

                if index < left_leaves {
                    // Target is in left subtree
                    proof.push(*right.hash());
                    Self::collect_proof(left, index, proof)
                } else {
                    // Target is in right subtree
                    proof.push(*left.hash());
                    Self::collect_proof(right, index - left_leaves, proof)
                }
            }
        }
    }

    /// Counts the number of leaves in a subtree
    fn count_leaves(node: &MerkleNode) -> usize {
        match node {
            MerkleNode::Leaf { .. } => 1,
            MerkleNode::Internal { left, right, .. } => {
                Self::count_leaves(left) + Self::count_leaves(right)
            }
        }
    }

    /// Verifies a proof for a leaf
    ///
    /// # Arguments
    ///
    /// * `leaf_hash` - Hash of the leaf data
    /// * `proof` - Proof path (sibling hashes)
    /// * `index` - Leaf index
    ///
    /// # Returns
    ///
    /// True if the proof is valid
    pub fn verify_proof(&self, leaf_hash: &Hash, proof: &[Hash], index: usize) -> bool {
        let mut current_hash = *leaf_hash;
        let mut current_index = index;

        // Proof is collected top-down, but we need to verify bottom-up
        for sibling_hash in proof.iter().rev() {
            if current_index % 2 == 0 {
                // We're on the left
                current_hash = Self::combine_hashes(&current_hash, sibling_hash);
            } else {
                // We're on the right
                current_hash = Self::combine_hashes(sibling_hash, &current_hash);
            }
            current_index /= 2;
        }

        self.root_hash() == Some(&current_hash)
    }
}

impl Default for MerkleTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_tree_creation() {
        let tree = MerkleTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.leaf_count(), 0);
    }

    #[test]
    fn test_merkle_tree_from_data() -> SyncResult<()> {
        let data = vec![
            b"block1".to_vec(),
            b"block2".to_vec(),
            b"block3".to_vec(),
            b"block4".to_vec(),
        ];

        let tree = MerkleTree::from_data(data)?;
        assert!(!tree.is_empty());
        assert_eq!(tree.leaf_count(), 4);
        assert!(tree.root_hash().is_some());

        Ok(())
    }

    #[test]
    fn test_merkle_tree_verify() -> SyncResult<()> {
        let data = vec![b"block1".to_vec(), b"block2".to_vec(), b"block3".to_vec()];

        let tree = MerkleTree::from_data(data.clone())?;
        assert!(tree.verify(&data)?);

        // Modify data
        let mut modified_data = data.clone();
        modified_data[1] = b"modified".to_vec();
        assert!(!tree.verify(&modified_data)?);

        Ok(())
    }

    #[test]
    fn test_merkle_tree_diff() -> SyncResult<()> {
        let data1 = vec![
            b"block1".to_vec(),
            b"block2".to_vec(),
            b"block3".to_vec(),
            b"block4".to_vec(),
        ];

        let mut data2 = data1.clone();
        data2[1] = b"modified".to_vec();

        let tree1 = MerkleTree::from_data(data1)?;
        let tree2 = MerkleTree::from_data(data2)?;

        let differences = tree1.diff(&tree2);
        assert!(!differences.is_empty());

        Ok(())
    }

    #[test]
    fn test_merkle_tree_proof() -> SyncResult<()> {
        let data = vec![
            b"block1".to_vec(),
            b"block2".to_vec(),
            b"block3".to_vec(),
            b"block4".to_vec(),
        ];

        let tree = MerkleTree::from_data(data.clone())?;

        let leaf_hash = MerkleTree::hash_data(&data[0]);
        let proof = tree.get_proof(0)?;

        assert!(tree.verify_proof(&leaf_hash, &proof, 0));

        Ok(())
    }
}
