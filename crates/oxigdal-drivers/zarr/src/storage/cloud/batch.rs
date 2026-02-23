//! Request batching for cloud storage

use super::range::ByteRange;
use crate::storage::StoreKey;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Request Batching
// ============================================================================

/// A batch of read requests that can be combined for efficiency
#[derive(Debug)]
pub struct RequestBatch {
    /// Requests in this batch
    requests: Vec<BatchedRequest>,
    /// Total size of all requests
    total_size: u64,
    /// Creation timestamp
    created_at: Instant,
    /// Maximum batch size (bytes)
    max_size: usize,
    /// Maximum batch age before flushing
    max_age: Duration,
}

/// A single request within a batch
#[derive(Debug, Clone)]
pub struct BatchedRequest {
    /// Store key for this request
    pub key: StoreKey,
    /// Optional byte range
    pub range: Option<ByteRange>,
    /// Request priority (lower = higher priority)
    pub priority: u32,
}

impl RequestBatch {
    /// Creates a new request batch
    #[must_use]
    pub fn new(max_size: usize, max_age: Duration) -> Self {
        Self {
            requests: Vec::new(),
            total_size: 0,
            created_at: Instant::now(),
            max_size,
            max_age,
        }
    }

    /// Adds a request to the batch
    ///
    /// # Returns
    /// `true` if the request was added, `false` if the batch is full
    pub fn add(&mut self, request: BatchedRequest, estimated_size: u64) -> bool {
        if self.is_full(estimated_size) {
            return false;
        }

        self.requests.push(request);
        self.total_size += estimated_size;
        true
    }

    /// Checks if the batch is full
    #[must_use]
    pub fn is_full(&self, additional_size: u64) -> bool {
        self.total_size.saturating_add(additional_size) > self.max_size as u64
    }

    /// Checks if the batch should be flushed due to age
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.max_age
    }

    /// Returns the number of requests in the batch
    #[must_use]
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    /// Returns true if the batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Takes all requests from the batch
    pub fn take(&mut self) -> Vec<BatchedRequest> {
        self.total_size = 0;
        self.created_at = Instant::now();
        std::mem::take(&mut self.requests)
    }

    /// Sorts requests by priority
    pub fn sort_by_priority(&mut self) {
        self.requests.sort_by_key(|r| r.priority);
    }

    /// Groups requests by contiguous ranges for the same key
    pub fn optimize_ranges(&mut self) {
        if self.requests.len() < 2 {
            return;
        }

        // Group by key
        let mut by_key: HashMap<String, Vec<&BatchedRequest>> = HashMap::new();
        for request in &self.requests {
            by_key
                .entry(request.key.to_string())
                .or_default()
                .push(request);
        }

        // Merge contiguous ranges within each key group
        let mut optimized = Vec::new();
        for (key_str, mut requests) in by_key {
            if requests.len() == 1 {
                optimized.push(requests[0].clone());
                continue;
            }

            // Sort by range start
            requests.sort_by_key(|r| r.range.map(|rng| rng.start).unwrap_or(0));

            let mut current: Option<BatchedRequest> = None;
            for req in requests {
                match (&mut current, &req.range) {
                    (Some(curr), Some(range)) => {
                        if let Some(curr_range) = curr.range {
                            if let Some(merged) = curr_range.merge(range) {
                                curr.range = Some(merged);
                                curr.priority = curr.priority.min(req.priority);
                            } else {
                                optimized.push(curr.clone());
                                current = Some(req.clone());
                            }
                        } else {
                            optimized.push(curr.clone());
                            current = Some(req.clone());
                        }
                    }
                    (Some(_), None) => {
                        if let Some(c) = current.take() {
                            optimized.push(c);
                        }
                        current = Some(BatchedRequest {
                            key: StoreKey::new(key_str.clone()),
                            range: req.range,
                            priority: req.priority,
                        });
                    }
                    (None, _) => {
                        if let Some(c) = current.take() {
                            optimized.push(c);
                        }
                        current = Some(BatchedRequest {
                            key: StoreKey::new(key_str.clone()),
                            range: req.range,
                            priority: req.priority,
                        });
                    }
                }
            }

            if let Some(c) = current {
                optimized.push(c);
            }
        }

        self.requests = optimized;
    }
}
