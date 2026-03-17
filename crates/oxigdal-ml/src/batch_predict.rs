//! Adaptive batch prediction for geospatial ML inference
//!
//! This module provides:
//! - [`PredictionRequest`] / [`PredictionResult`] — typed request/response pairs
//! - [`AdaptiveBatcher`] — adjusts batch size based on observed latency so that
//!   throughput is maximised while keeping per-batch latency near a configurable
//!   target.
//!
//! # Algorithm
//!
//! After each batch completes the batcher computes a rolling average over the
//! last `N` observations and compares it to `target_latency_ms`:
//!
//! - **Too slow** (avg > target): shrink towards `min_batch_size`
//! - **Too fast** (avg < target): grow towards `max_batch_size`
//!
//! The magnitude of each adjustment is controlled by `adaptation_rate`.

use crate::error::MlError;

/// Single prediction request carrying raw float tensors.
#[derive(Debug, Clone)]
pub struct PredictionRequest {
    /// Caller-assigned identifier (used to correlate results)
    pub id: u64,
    /// Input tensors as flat float vectors
    pub inputs: Vec<Vec<f32>>,
    /// Shape of each input tensor (e.g. `[3, 256, 256]` for a CHW image)
    pub input_shapes: Vec<Vec<usize>>,
}

/// Single prediction result produced by an inference run.
#[derive(Debug, Clone)]
pub struct PredictionResult {
    /// Matches the `id` field of the originating [`PredictionRequest`]
    pub id: u64,
    /// Output tensors as flat float vectors
    pub outputs: Vec<Vec<f32>>,
    /// Shape of each output tensor
    pub output_shapes: Vec<Vec<usize>>,
    /// Wall-clock latency of the inference call in milliseconds
    pub latency_ms: f64,
}

/// Adaptive batch sizing configuration
#[derive(Debug, Clone)]
pub struct AdaptiveBatchConfig {
    /// Minimum allowed batch size (≥ 1)
    pub min_batch_size: usize,
    /// Maximum allowed batch size
    pub max_batch_size: usize,
    /// Target latency per batch in milliseconds
    pub target_latency_ms: f64,
    /// Learning rate for batch-size adaptation (0.0 – 1.0)
    ///
    /// Higher values cause larger adjustments; lower values yield smoother
    /// adaptation.
    pub adaptation_rate: f64,
}

impl Default for AdaptiveBatchConfig {
    fn default() -> Self {
        Self {
            min_batch_size: 1,
            max_batch_size: 64,
            target_latency_ms: 50.0,
            adaptation_rate: 0.1,
        }
    }
}

impl AdaptiveBatchConfig {
    /// Validate the configuration.  Returns an error if any invariant is
    /// violated.
    pub fn validate(&self) -> Result<(), MlError> {
        if self.min_batch_size == 0 {
            return Err(MlError::InvalidConfig(
                "min_batch_size must be at least 1".into(),
            ));
        }
        if self.max_batch_size < self.min_batch_size {
            return Err(MlError::InvalidConfig(
                "max_batch_size must be >= min_batch_size".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.adaptation_rate) {
            return Err(MlError::InvalidConfig(
                "adaptation_rate must be in [0.0, 1.0]".into(),
            ));
        }
        if self.target_latency_ms <= 0.0 {
            return Err(MlError::InvalidConfig(
                "target_latency_ms must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Adaptive batch size controller
///
/// Tracks recent inference latencies and adjusts the recommended batch size
/// to keep per-batch latency near the configured target.
pub struct AdaptiveBatcher {
    config: AdaptiveBatchConfig,
    current_batch_size: usize,
    /// Ring buffer of the most recent latency observations (milliseconds)
    recent_latencies: Vec<f64>,
    total_batches: u64,
    total_items: u64,
    /// Maximum number of latency samples to keep for the rolling average
    window_size: usize,
}

impl AdaptiveBatcher {
    /// Create a new `AdaptiveBatcher` starting at `min_batch_size`.
    pub fn new(config: AdaptiveBatchConfig) -> Self {
        let start = config.min_batch_size;
        Self {
            config,
            current_batch_size: start,
            recent_latencies: Vec::new(),
            total_batches: 0,
            total_items: 0,
            window_size: 10,
        }
    }

    /// Return the current recommended batch size.
    pub fn recommended_batch_size(&self) -> usize {
        self.current_batch_size
    }

    /// Update the batch-size estimate based on the observed latency for a
    /// completed batch.
    ///
    /// # Parameters
    /// - `latency_ms`: wall-clock time the batch took in milliseconds
    /// - `batch_size`: number of items that were in the completed batch
    pub fn update_latency(&mut self, latency_ms: f64, batch_size: usize) {
        // Maintain a rolling window of latency observations
        self.recent_latencies.push(latency_ms);
        if self.recent_latencies.len() > self.window_size {
            self.recent_latencies.remove(0);
        }

        self.total_batches += 1;
        self.total_items += batch_size as u64;

        let avg = self.average_latency_ms();
        let target = self.config.target_latency_ms;
        let rate = self.config.adaptation_rate;
        let min_bs = self.config.min_batch_size as f64;
        let max_bs = self.config.max_batch_size as f64;
        let current = self.current_batch_size as f64;

        let new_size = if avg > target {
            // Too slow — reduce batch size; always move at least 1 step down
            let reduction = (current * rate * (avg - target) / target).max(1.0);
            (current - reduction).max(min_bs)
        } else {
            // Too fast — increase batch size; always move at least 1 step up
            let gain = (current * rate * (target - avg) / target).max(1.0);
            (current + gain).min(max_bs)
        };

        self.current_batch_size = (new_size.round() as usize)
            .max(self.config.min_batch_size)
            .min(self.config.max_batch_size);
    }

    /// Group a flat list of requests into batches, each of at most
    /// `recommended_batch_size()` items.
    pub fn create_batches(&self, requests: Vec<PredictionRequest>) -> Vec<Vec<PredictionRequest>> {
        if requests.is_empty() {
            return Vec::new();
        }
        let bs = self.current_batch_size.max(1);
        requests.chunks(bs).map(|chunk| chunk.to_vec()).collect()
    }

    /// Rolling average latency over the recent observation window.
    ///
    /// Returns `0.0` when no observations have been recorded yet.
    pub fn average_latency_ms(&self) -> f64 {
        if self.recent_latencies.is_empty() {
            return 0.0;
        }
        self.recent_latencies.iter().sum::<f64>() / self.recent_latencies.len() as f64
    }

    /// Total number of completed batches.
    pub fn total_batches(&self) -> u64 {
        self.total_batches
    }

    /// Total number of individual items processed across all batches.
    pub fn total_items(&self) -> u64 {
        self.total_items
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &AdaptiveBatchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_batcher() -> AdaptiveBatcher {
        AdaptiveBatcher::new(AdaptiveBatchConfig::default())
    }

    fn make_request(id: u64) -> PredictionRequest {
        PredictionRequest {
            id,
            inputs: vec![vec![1.0, 2.0, 3.0]],
            input_shapes: vec![vec![3]],
        }
    }

    #[test]
    fn test_construction_with_default_config() {
        let batcher = default_batcher();
        assert_eq!(
            batcher.recommended_batch_size(),
            AdaptiveBatchConfig::default().min_batch_size
        );
    }

    #[test]
    fn test_recommended_batch_size_starts_at_min() {
        let config = AdaptiveBatchConfig {
            min_batch_size: 4,
            max_batch_size: 64,
            ..Default::default()
        };
        let batcher = AdaptiveBatcher::new(config);
        assert_eq!(batcher.recommended_batch_size(), 4);
    }

    #[test]
    fn test_update_latency_adjusts_up_when_fast() {
        let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 1,
            max_batch_size: 128,
            target_latency_ms: 100.0,
            adaptation_rate: 0.5,
        });
        let initial = batcher.recommended_batch_size();
        // Very fast batch — should grow
        batcher.update_latency(10.0, initial);
        assert!(
            batcher.recommended_batch_size() > initial,
            "batch size should grow when latency is well below target"
        );
    }

    #[test]
    fn test_update_latency_adjusts_down_when_slow() {
        let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 1,
            max_batch_size: 64,
            target_latency_ms: 50.0,
            adaptation_rate: 0.5,
        });
        // Force the current size up first
        for _ in 0..10 {
            let sz = batcher.recommended_batch_size();
            batcher.update_latency(10.0, sz);
        }
        let high = batcher.recommended_batch_size();
        // Now feed a very slow batch
        batcher.update_latency(9999.0, high);
        assert!(
            batcher.recommended_batch_size() < high,
            "batch size should shrink when latency exceeds target"
        );
    }

    #[test]
    fn test_batch_size_does_not_exceed_max() {
        let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 1,
            max_batch_size: 8,
            target_latency_ms: 1000.0, // very long target → always growing
            adaptation_rate: 1.0,
        });
        for _ in 0..100 {
            let sz = batcher.recommended_batch_size();
            batcher.update_latency(0.001, sz);
        }
        assert!(batcher.recommended_batch_size() <= 8);
    }

    #[test]
    fn test_batch_size_does_not_go_below_min() {
        let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 4,
            max_batch_size: 64,
            target_latency_ms: 1.0, // very short target → always shrinking
            adaptation_rate: 1.0,
        });
        for _ in 0..100 {
            let sz = batcher.recommended_batch_size();
            batcher.update_latency(99999.0, sz);
        }
        assert!(batcher.recommended_batch_size() >= 4);
    }

    #[test]
    fn test_create_batches_splits_correctly() {
        let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 3,
            max_batch_size: 3,
            ..Default::default()
        });
        // Force batch size to 3
        batcher.current_batch_size = 3;

        let requests: Vec<PredictionRequest> = (0..7).map(make_request).collect();
        let batches = batcher.create_batches(requests);

        assert_eq!(batches.len(), 3, "7 items / 3 = 3 batches (3, 3, 1)");
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 3);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn test_create_batches_fewer_than_batch_size() {
        let batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
            min_batch_size: 16,
            max_batch_size: 64,
            ..Default::default()
        });
        let requests: Vec<PredictionRequest> = (0..5).map(make_request).collect();
        let batches = batcher.create_batches(requests);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 5);
    }

    #[test]
    fn test_create_batches_empty_input() {
        let batcher = default_batcher();
        let batches = batcher.create_batches(vec![]);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_average_latency_ms_no_observations() {
        let batcher = default_batcher();
        assert_eq!(batcher.average_latency_ms(), 0.0);
    }

    #[test]
    fn test_average_latency_ms_single_observation() {
        let mut batcher = default_batcher();
        batcher.update_latency(42.0, 1);
        assert!((batcher.average_latency_ms() - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_average_latency_ms_multiple_observations() {
        let mut batcher = default_batcher();
        batcher.update_latency(10.0, 1);
        batcher.update_latency(20.0, 1);
        batcher.update_latency(30.0, 1);
        assert!((batcher.average_latency_ms() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn test_total_batches_and_items_tracking() {
        let mut batcher = default_batcher();
        batcher.update_latency(50.0, 8);
        batcher.update_latency(50.0, 4);
        assert_eq!(batcher.total_batches(), 2);
        assert_eq!(batcher.total_items(), 12);
    }

    #[test]
    fn test_config_validation_invalid_min_batch() {
        let config = AdaptiveBatchConfig {
            min_batch_size: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_max_less_than_min() {
        let config = AdaptiveBatchConfig {
            min_batch_size: 10,
            max_batch_size: 5,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_adaptation_rate() {
        let config = AdaptiveBatchConfig {
            adaptation_rate: 1.5,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_prediction_result_fields() {
        let result = PredictionResult {
            id: 42,
            outputs: vec![vec![0.9, 0.1]],
            output_shapes: vec![vec![2]],
            latency_ms: 12.5,
        };
        assert_eq!(result.id, 42);
        assert!((result.latency_ms - 12.5).abs() < 1e-9);
    }
}
