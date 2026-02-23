//! Delta encoding for bandwidth-efficient synchronization
//!
//! This module provides delta encoding/decoding capabilities to minimize
//! the amount of data transmitted during synchronization.

use crate::{SyncError, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Delta operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaOp {
    /// Copy bytes from the base at offset, length
    Copy {
        /// Offset in the base data
        offset: usize,
        /// Number of bytes to copy
        length: usize,
    },
    /// Insert new data
    Insert {
        /// Data to insert
        data: Vec<u8>,
    },
}

/// A delta representing the difference between two byte sequences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    /// The operations that transform base into target
    ops: Vec<DeltaOp>,
    /// Size of the base data
    base_size: usize,
    /// Size of the target data
    target_size: usize,
}

impl Delta {
    /// Creates a new delta
    pub fn new(base_size: usize, target_size: usize) -> Self {
        Self {
            ops: Vec::new(),
            base_size,
            target_size,
        }
    }

    /// Adds a copy operation
    ///
    /// # Arguments
    ///
    /// * `offset` - Offset in base data
    /// * `length` - Number of bytes to copy
    pub fn add_copy(&mut self, offset: usize, length: usize) {
        if length == 0 {
            return;
        }

        self.ops.push(DeltaOp::Copy { offset, length });
    }

    /// Adds an insert operation
    ///
    /// # Arguments
    ///
    /// * `data` - Data to insert
    pub fn add_insert(&mut self, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }

        self.ops.push(DeltaOp::Insert { data });
    }

    /// Gets the operations
    pub fn operations(&self) -> &[DeltaOp] {
        &self.ops
    }

    /// Gets the base size
    pub fn base_size(&self) -> usize {
        self.base_size
    }

    /// Gets the target size
    pub fn target_size(&self) -> usize {
        self.target_size
    }

    /// Applies the delta to base data to produce target data
    ///
    /// # Arguments
    ///
    /// * `base` - The base data
    ///
    /// # Returns
    ///
    /// The reconstructed target data
    pub fn apply(&self, base: &[u8]) -> SyncResult<Vec<u8>> {
        if base.len() != self.base_size {
            return Err(SyncError::DeltaEncodingError(format!(
                "Base size mismatch: expected {}, got {}",
                self.base_size,
                base.len()
            )));
        }

        let mut result = Vec::with_capacity(self.target_size);

        for op in &self.ops {
            match op {
                DeltaOp::Copy { offset, length } => {
                    if *offset + *length > base.len() {
                        return Err(SyncError::DeltaEncodingError(
                            "Copy beyond base data".to_string(),
                        ));
                    }
                    result.extend_from_slice(&base[*offset..*offset + *length]);
                }
                DeltaOp::Insert { data } => {
                    result.extend_from_slice(data);
                }
            }
        }

        if result.len() != self.target_size {
            return Err(SyncError::DeltaEncodingError(format!(
                "Target size mismatch: expected {}, got {}",
                self.target_size,
                result.len()
            )));
        }

        Ok(result)
    }

    /// Calculates the compression ratio
    ///
    /// # Returns
    ///
    /// Ratio of delta size to target size (lower is better)
    pub fn compression_ratio(&self) -> f64 {
        let delta_size = self.delta_size();
        if self.target_size == 0 {
            return 0.0;
        }
        delta_size as f64 / self.target_size as f64
    }

    /// Calculates the size of the delta in bytes
    fn delta_size(&self) -> usize {
        let mut size = 0;
        for op in &self.ops {
            match op {
                DeltaOp::Copy { .. } => {
                    // Size of offset + length (roughly 16 bytes)
                    size += 16;
                }
                DeltaOp::Insert { data } => {
                    size += data.len() + 8; // Data plus length field
                }
            }
        }
        size
    }
}

/// Delta encoder using a simple block-based algorithm
pub struct DeltaEncoder {
    /// Block size for matching
    block_size: usize,
}

impl DeltaEncoder {
    /// Creates a new delta encoder
    ///
    /// # Arguments
    ///
    /// * `block_size` - Size of blocks for matching (larger = faster but less compression)
    pub fn new(block_size: usize) -> Self {
        Self { block_size }
    }

    /// Creates a delta encoder with default settings
    pub fn default_encoder() -> Self {
        Self::new(16) // 16-byte blocks
    }

    /// Encodes the difference between base and target
    ///
    /// # Arguments
    ///
    /// * `base` - The base data
    /// * `target` - The target data
    ///
    /// # Returns
    ///
    /// A delta that transforms base into target
    pub fn encode(&self, base: &[u8], target: &[u8]) -> SyncResult<Delta> {
        let mut delta = Delta::new(base.len(), target.len());

        // Build a hash table of base blocks
        let base_blocks = self.build_block_index(base);

        let mut target_pos = 0;
        let mut pending_insert = Vec::new();

        while target_pos < target.len() {
            // Try to find a match in base
            if let Some(match_info) = self.find_match(base, target, target_pos, &base_blocks) {
                // Flush any pending insert
                if !pending_insert.is_empty() {
                    delta.add_insert(pending_insert.clone());
                    pending_insert.clear();
                }

                // Add copy operation
                delta.add_copy(match_info.0, match_info.1);
                target_pos += match_info.1;
            } else {
                // No match found, accumulate for insert
                if target_pos < target.len() {
                    pending_insert.push(target[target_pos]);
                }
                target_pos += 1;
            }
        }

        // Flush remaining insert
        if !pending_insert.is_empty() {
            delta.add_insert(pending_insert);
        }

        Ok(delta)
    }

    /// Builds an index of block positions in base data
    fn build_block_index(&self, base: &[u8]) -> HashMap<u64, Vec<usize>> {
        let mut index = HashMap::new();

        for i in 0..base.len().saturating_sub(self.block_size - 1) {
            let block = &base[i..i + self.block_size];
            let hash = self.hash_block(block);
            index.entry(hash).or_insert_with(Vec::new).push(i);
        }

        index
    }

    /// Simple hash function for blocks
    fn hash_block(&self, block: &[u8]) -> u64 {
        let mut hash: u64 = 0;
        for (i, &byte) in block.iter().enumerate() {
            hash = hash.wrapping_add((byte as u64).wrapping_mul(31_u64.wrapping_pow(i as u32)));
        }
        hash
    }

    /// Finds a match for target data in base
    ///
    /// Returns (offset, length) of the match, or None if no match found
    fn find_match(
        &self,
        base: &[u8],
        target: &[u8],
        target_pos: usize,
        base_blocks: &HashMap<u64, Vec<usize>>,
    ) -> Option<(usize, usize)> {
        if target_pos + self.block_size > target.len() {
            return None;
        }

        let target_block = &target[target_pos..target_pos + self.block_size];
        let hash = self.hash_block(target_block);

        // Find candidates
        let candidates = base_blocks.get(&hash)?;

        // Find the longest match among candidates
        let mut best_match: Option<(usize, usize)> = None;

        for &base_pos in candidates {
            let mut length = 0;

            while base_pos + length < base.len()
                && target_pos + length < target.len()
                && base[base_pos + length] == target[target_pos + length]
            {
                length += 1;
            }

            if length >= self.block_size {
                if let Some((_, best_len)) = best_match {
                    if length > best_len {
                        best_match = Some((base_pos, length));
                    }
                } else {
                    best_match = Some((base_pos, length));
                }
            }
        }

        best_match
    }
}

impl Default for DeltaEncoder {
    fn default() -> Self {
        Self::default_encoder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_creation() {
        let delta = Delta::new(100, 150);
        assert_eq!(delta.base_size(), 100);
        assert_eq!(delta.target_size(), 150);
    }

    #[test]
    fn test_delta_add_copy() {
        let mut delta = Delta::new(100, 100);
        delta.add_copy(0, 50);
        assert_eq!(delta.operations().len(), 1);
    }

    #[test]
    fn test_delta_add_insert() {
        let mut delta = Delta::new(100, 110);
        delta.add_insert(b"hello".to_vec());
        assert_eq!(delta.operations().len(), 1);
    }

    #[test]
    fn test_delta_apply_copy() -> SyncResult<()> {
        let base = b"hello world";
        let mut delta = Delta::new(base.len(), 5);
        delta.add_copy(0, 5);

        let result = delta.apply(base)?;
        assert_eq!(result, b"hello");

        Ok(())
    }

    #[test]
    fn test_delta_apply_insert() -> SyncResult<()> {
        let base = b"";
        let mut delta = Delta::new(0, 5);
        delta.add_insert(b"hello".to_vec());

        let result = delta.apply(base)?;
        assert_eq!(result, b"hello");

        Ok(())
    }

    #[test]
    fn test_delta_apply_mixed() -> SyncResult<()> {
        let base = b"hello world";
        // "hello, world" = 12 characters
        let mut delta = Delta::new(base.len(), 12);
        delta.add_copy(0, 5);
        delta.add_insert(b", ".to_vec());
        delta.add_copy(6, 5);

        let result = delta.apply(base)?;
        assert_eq!(result, b"hello, world");

        Ok(())
    }

    #[test]
    fn test_delta_encoder() -> SyncResult<()> {
        let encoder = DeltaEncoder::default_encoder();
        let base = b"hello world, this is a test";
        let target = b"hello world, this is a test!";

        let delta = encoder.encode(base, target)?;
        let result = delta.apply(base)?;

        assert_eq!(result, target);

        Ok(())
    }

    #[test]
    fn test_delta_encoder_identical() -> SyncResult<()> {
        let encoder = DeltaEncoder::default_encoder();
        let base = b"hello world";
        let target = b"hello world";

        let delta = encoder.encode(base, target)?;
        let result = delta.apply(base)?;

        assert_eq!(result, target);

        Ok(())
    }

    #[test]
    fn test_delta_encoder_no_match() -> SyncResult<()> {
        let encoder = DeltaEncoder::default_encoder();
        let base = b"hello world";
        let target = b"completely different";

        let delta = encoder.encode(base, target)?;
        let result = delta.apply(base)?;

        assert_eq!(result, target);

        Ok(())
    }
}
