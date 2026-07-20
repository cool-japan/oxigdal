//! Self-contained, domain-separated Merkle tree with inclusion proofs.
//!
//! Leaves are combined left-to-right; a level with an odd number of nodes
//! carries its trailing node up unchanged (no duplication). Every internal node
//! is `blake3(DOM_NODE ‖ left ‖ right)`, and the root of an *empty* leaf set is
//! `blake3(DOM_EMPTY ‖ session_id)`, binding the root to the session.

use super::{AttestationError, DOM_EMPTY, hash_parts, node_hash};

/// One step of a Merkle inclusion proof: a sibling hash and which side it sits
/// on relative to the accumulated value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofStep {
    /// The sibling node's 32-byte hash.
    pub sibling: [u8; 32],
    /// `true` if the sibling is the *left* operand (accumulator on the right).
    pub sibling_is_left: bool,
}

/// Compute the domain-separated Merkle root over `leaves`.
///
/// `leaves` are already leaf hashes (see
/// [`leaf_hash`](super::AttestedOperation)); typically produced from a
/// [`SessionLog`](super::SessionLog). An empty `leaves` yields the session-bound
/// empty root `blake3(DOM_EMPTY ‖ session_id)`.
pub fn merkle_root(leaves: &[[u8; 32]], session_id: &[u8; 16]) -> [u8; 32] {
    if leaves.is_empty() {
        return hash_parts(&[DOM_EMPTY, session_id]);
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            if i + 1 < level.len() {
                next.push(node_hash(&level[i], &level[i + 1]));
                i += 2;
            } else {
                next.push(level[i]);
                i += 1;
            }
        }
        level = next;
    }
    level[0]
}

/// Build an inclusion proof for the leaf at `index`.
///
/// The returned steps, replayed by [`verify_merkle_proof`], reconstruct the
/// Merkle root. Returns [`AttestationError::IndexOutOfRange`] if `index` is not
/// a valid leaf index.
///
/// Note: the proof intentionally does **not** carry the empty/single-leaf
/// session binding, so it must be checked against the corresponding
/// [`merkle_root`] output.
pub fn merkle_proof(leaves: &[[u8; 32]], index: usize) -> Result<Vec<ProofStep>, AttestationError> {
    if index >= leaves.len() {
        return Err(AttestationError::IndexOutOfRange(index));
    }
    let mut proof = Vec::new();
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = index;
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            if i + 1 < level.len() {
                if idx == i {
                    proof.push(ProofStep {
                        sibling: level[i + 1],
                        sibling_is_left: false,
                    });
                } else if idx == i + 1 {
                    proof.push(ProofStep {
                        sibling: level[i],
                        sibling_is_left: true,
                    });
                }
                next.push(node_hash(&level[i], &level[i + 1]));
                i += 2;
            } else {
                // Odd trailing node carried up unchanged: no proof step.
                next.push(level[i]);
                i += 1;
            }
        }
        idx /= 2;
        level = next;
    }
    Ok(proof)
}

/// Replay an inclusion `proof` for `leaf` and check it reproduces `root`.
///
/// An empty proof verifies exactly when `leaf == root` (single-leaf tree).
pub fn verify_merkle_proof(leaf: &[u8; 32], proof: &[ProofStep], root: &[u8; 32]) -> bool {
    let mut acc = *leaf;
    for step in proof {
        acc = if step.sibling_is_left {
            node_hash(&step.sibling, &acc)
        } else {
            node_hash(&acc, &step.sibling)
        };
    }
    &acc == root
}
