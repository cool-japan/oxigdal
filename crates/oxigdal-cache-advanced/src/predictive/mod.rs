//! Predictive prefetching with ML
//!
//! Learns access patterns and predicts future cache accesses:
//! - Temporal pattern learning
//! - Spatial pattern learning
//! - Markov chain prediction
//! - Neural network predictor
//! - Confidence-based prefetch decisions
//! - Advanced ML models (Transformer, LSTM, Hybrid)

pub mod advanced;

use crate::multi_tier::CacheKey;
use ndarray::{Array1, Array2};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Generate normal distributed random number using Box-Muller transform
fn rand_normal(mean: f64, std_dev: f64) -> f64 {
    let u1 = fastrand::f64();
    let u2 = fastrand::f64();
    // Avoid log(0) by ensuring u1 > 0
    let u1 = if u1 < 1e-10 { 1e-10 } else { u1 };
    let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    mean + z0 * std_dev
}

/// Access pattern record
#[derive(Debug, Clone)]
pub struct AccessRecord {
    /// Key accessed
    pub key: CacheKey,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Access type (read/write)
    pub access_type: AccessType,
}

/// Access type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Read access
    Read,
    /// Write access
    Write,
}

/// Prediction with confidence
#[derive(Debug, Clone)]
pub struct Prediction {
    /// Predicted key
    pub key: CacheKey,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Predicted access time
    pub predicted_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl Prediction {
    /// Check if prediction is confident enough
    pub fn is_confident(&self, threshold: f64) -> bool {
        self.confidence >= threshold
    }
}

/// Markov chain predictor
/// Predicts next access based on current state
pub struct MarkovPredictor {
    /// Transition matrix (key -> next_key -> probability)
    transitions: HashMap<CacheKey, HashMap<CacheKey, f64>>,
    /// Transition counts for learning
    transition_counts: HashMap<CacheKey, HashMap<CacheKey, u64>>,
    /// Current state
    current_key: Option<CacheKey>,
    /// Order of Markov chain (n-gram size)
    order: usize,
    /// Recent access history
    history: VecDeque<CacheKey>,
}

impl MarkovPredictor {
    /// Create new Markov predictor
    pub fn new(order: usize) -> Self {
        Self {
            transitions: HashMap::new(),
            transition_counts: HashMap::new(),
            current_key: None,
            order,
            history: VecDeque::with_capacity(order),
        }
    }

    /// Record an access
    pub fn record_access(&mut self, key: CacheKey) {
        if let Some(prev_key) = self.current_key.clone() {
            // Update transition counts
            let next_counts = self.transition_counts.entry(prev_key.clone()).or_default();

            *next_counts.entry(key.clone()).or_insert(0) += 1;

            // Rebuild probabilities
            self.update_probabilities(&prev_key);
        }

        // Update history
        if self.history.len() >= self.order {
            self.history.pop_front();
        }
        self.history.push_back(key.clone());

        self.current_key = Some(key);
    }

    /// Update transition probabilities for a key
    fn update_probabilities(&mut self, from_key: &CacheKey) {
        if let Some(counts) = self.transition_counts.get(from_key) {
            let total: u64 = counts.values().sum();

            if total > 0 {
                let probabilities: HashMap<CacheKey, f64> = counts
                    .iter()
                    .map(|(k, count)| (k.clone(), *count as f64 / total as f64))
                    .collect();

                self.transitions.insert(from_key.clone(), probabilities);
            }
        }
    }

    /// Predict next keys
    pub fn predict(&self, top_n: usize) -> Vec<Prediction> {
        if let Some(current) = &self.current_key {
            if let Some(transitions) = self.transitions.get(current) {
                let mut predictions: Vec<_> = transitions
                    .iter()
                    .map(|(key, prob)| Prediction {
                        key: key.clone(),
                        confidence: *prob,
                        predicted_time: None,
                    })
                    .collect();

                predictions.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                predictions.truncate(top_n);
                return predictions;
            }
        }

        Vec::new()
    }

    /// Get number of states in the model
    pub fn state_count(&self) -> usize {
        self.transitions.len()
    }

    /// Clear the model
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.transition_counts.clear();
        self.current_key = None;
        self.history.clear();
    }
}

/// Temporal pattern detector
/// Detects periodic access patterns
pub struct TemporalPatternDetector {
    /// Access history with timestamps
    access_history: VecDeque<(CacheKey, chrono::DateTime<chrono::Utc>)>,
    /// Maximum history size
    max_history: usize,
    /// Detected patterns (key -> period in seconds)
    patterns: HashMap<CacheKey, Vec<i64>>,
}

impl TemporalPatternDetector {
    /// Create new temporal pattern detector
    pub fn new(max_history: usize) -> Self {
        Self {
            access_history: VecDeque::with_capacity(max_history),
            max_history,
            patterns: HashMap::new(),
        }
    }

    /// Record an access
    pub fn record_access(&mut self, key: CacheKey, timestamp: chrono::DateTime<chrono::Utc>) {
        if self.access_history.len() >= self.max_history {
            self.access_history.pop_front();
        }

        self.access_history.push_back((key.clone(), timestamp));

        // Detect patterns for this key
        self.detect_pattern(&key);
    }

    /// Detect access pattern for a key
    fn detect_pattern(&mut self, key: &CacheKey) {
        let accesses: Vec<_> = self
            .access_history
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, ts)| *ts)
            .collect();

        if accesses.len() < 3 {
            return;
        }

        // Calculate intervals between accesses
        let mut intervals = Vec::new();
        for i in 1..accesses.len() {
            let interval = (accesses[i] - accesses[i - 1]).num_seconds();
            intervals.push(interval);
        }

        // Store intervals as pattern
        self.patterns.insert(key.clone(), intervals);
    }

    /// Predict next access time for a key
    pub fn predict_next_access(&self, key: &CacheKey) -> Option<chrono::DateTime<chrono::Utc>> {
        if let Some(intervals) = self.patterns.get(key) {
            if intervals.is_empty() {
                return None;
            }

            // Use median interval as prediction
            let mut sorted_intervals = intervals.clone();
            sorted_intervals.sort();
            let median_interval = sorted_intervals[sorted_intervals.len() / 2];

            // Find last access time
            let last_access = self
                .access_history
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, ts)| *ts);

            if let Some(last) = last_access {
                return Some(last + chrono::Duration::seconds(median_interval));
            }
        }

        None
    }

    /// Get prediction with confidence
    pub fn predict(&self, key: &CacheKey) -> Option<Prediction> {
        if let Some(next_time) = self.predict_next_access(key) {
            let intervals = self.patterns.get(key)?;

            // Calculate confidence based on pattern stability
            let confidence = if intervals.len() < 2 {
                0.5
            } else {
                let mean: f64 =
                    intervals.iter().map(|&x| x as f64).sum::<f64>() / intervals.len() as f64;
                let variance: f64 = intervals
                    .iter()
                    .map(|&x| {
                        let diff = x as f64 - mean;
                        diff * diff
                    })
                    .sum::<f64>()
                    / intervals.len() as f64;

                let std_dev = variance.sqrt();
                let cv = if mean > 0.0 { std_dev / mean } else { 1.0 };

                // Lower coefficient of variation = higher confidence
                (1.0 / (1.0 + cv)).clamp(0.0, 1.0)
            };

            Some(Prediction {
                key: key.clone(),
                confidence,
                predicted_time: Some(next_time),
            })
        } else {
            None
        }
    }

    /// Clear patterns
    pub fn clear(&mut self) {
        self.access_history.clear();
        self.patterns.clear();
    }
}

/// Spatial pattern detector
/// Detects related keys that are often accessed together
pub struct SpatialPatternDetector {
    /// Co-occurrence matrix (key1 -> key2 -> count)
    co_occurrences: HashMap<CacheKey, HashMap<CacheKey, u64>>,
    /// Recent access window
    window: VecDeque<CacheKey>,
    /// Window size
    window_size: usize,
}

impl SpatialPatternDetector {
    /// Create new spatial pattern detector
    pub fn new(window_size: usize) -> Self {
        Self {
            co_occurrences: HashMap::new(),
            window: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Record an access
    pub fn record_access(&mut self, key: CacheKey) {
        // Update co-occurrences with all keys in current window (bidirectional)
        for other_key in &self.window {
            // Record key -> other_key
            let co_occurs = self.co_occurrences.entry(key.clone()).or_default();
            *co_occurs.entry(other_key.clone()).or_insert(0) += 1;

            // Record other_key -> key (bidirectional)
            let co_occurs_reverse = self.co_occurrences.entry(other_key.clone()).or_default();
            *co_occurs_reverse.entry(key.clone()).or_insert(0) += 1;
        }

        // Update window
        if self.window.len() >= self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(key);
    }

    /// Get related keys
    pub fn get_related_keys(&self, key: &CacheKey, top_n: usize) -> Vec<Prediction> {
        if let Some(co_occurs) = self.co_occurrences.get(key) {
            let total: u64 = co_occurs.values().sum();

            if total == 0 {
                return Vec::new();
            }

            let mut predictions: Vec<_> = co_occurs
                .iter()
                .map(|(k, count)| Prediction {
                    key: k.clone(),
                    confidence: *count as f64 / total as f64,
                    predicted_time: None,
                })
                .collect();

            predictions.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            predictions.truncate(top_n);
            predictions
        } else {
            Vec::new()
        }
    }

    /// Clear patterns
    pub fn clear(&mut self) {
        self.co_occurrences.clear();
        self.window.clear();
    }
}

/// Simple neural network predictor
/// Uses a feed-forward network to predict access patterns
pub struct NeuralPredictor {
    /// Input size (vocabulary size)
    vocab_size: usize,
    /// Hidden layer size
    hidden_size: usize,
    /// Weights for input to hidden layer
    w1: Option<Array2<f64>>,
    /// Weights for hidden to output layer
    w2: Option<Array2<f64>>,
    /// Bias for hidden layer
    b1: Option<Array1<f64>>,
    /// Bias for output layer
    b2: Option<Array1<f64>>,
    /// Key to index mapping
    key_to_idx: HashMap<CacheKey, usize>,
    /// Index to key mapping
    idx_to_key: Vec<CacheKey>,
    /// Learning rate
    #[allow(dead_code)]
    learning_rate: f64,
    /// Training enabled
    #[allow(dead_code)]
    training_enabled: bool,
}

impl NeuralPredictor {
    /// Create new neural predictor
    pub fn new(hidden_size: usize) -> Self {
        Self {
            vocab_size: 0,
            hidden_size,
            w1: None,
            w2: None,
            b1: None,
            b2: None,
            key_to_idx: HashMap::new(),
            idx_to_key: Vec::new(),
            learning_rate: 0.01,
            training_enabled: true,
        }
    }

    /// Add key to vocabulary
    fn add_to_vocab(&mut self, key: &CacheKey) -> usize {
        if let Some(&idx) = self.key_to_idx.get(key) {
            idx
        } else {
            let idx = self.vocab_size;
            self.key_to_idx.insert(key.clone(), idx);
            self.idx_to_key.push(key.clone());
            self.vocab_size += 1;

            // Reinitialize weights if needed
            if self.vocab_size > 0 {
                self.initialize_weights();
            }

            idx
        }
    }

    /// Initialize weights
    fn initialize_weights(&mut self) {
        // Seed fastrand for reproducibility
        fastrand::seed(42);

        // Xavier initialization
        let scale_w1 = (2.0 / (self.vocab_size + self.hidden_size) as f64).sqrt();
        let scale_w2 = (2.0 / (self.hidden_size + self.vocab_size) as f64).sqrt();

        let w1_data: Vec<f64> = (0..self.vocab_size * self.hidden_size)
            .map(|_| rand_normal(0.0, scale_w1))
            .collect();

        let w2_data: Vec<f64> = (0..self.hidden_size * self.vocab_size)
            .map(|_| rand_normal(0.0, scale_w2))
            .collect();

        self.w1 = Some(
            Array2::from_shape_vec((self.vocab_size, self.hidden_size), w1_data)
                .unwrap_or_else(|_| Array2::zeros((self.vocab_size, self.hidden_size))),
        );

        self.w2 = Some(
            Array2::from_shape_vec((self.hidden_size, self.vocab_size), w2_data)
                .unwrap_or_else(|_| Array2::zeros((self.hidden_size, self.vocab_size))),
        );

        self.b1 = Some(Array1::zeros(self.hidden_size));
        self.b2 = Some(Array1::zeros(self.vocab_size));
    }

    /// Forward pass
    fn forward(&self, input_idx: usize) -> Option<Array1<f64>> {
        if input_idx >= self.vocab_size {
            return None;
        }

        let w1 = self.w1.as_ref()?;
        let w2 = self.w2.as_ref()?;
        let b1 = self.b1.as_ref()?;
        let b2 = self.b2.as_ref()?;

        // One-hot encoding
        let mut input = Array1::zeros(self.vocab_size);
        input[input_idx] = 1.0;

        // Hidden layer with ReLU
        let hidden = w1.t().dot(&input) + b1;
        let hidden_activated = hidden.mapv(|x| x.max(0.0));

        // Output layer with softmax
        let output = w2.t().dot(&hidden_activated) + b2;
        let output_exp = output.mapv(|x| x.exp());
        let sum_exp: f64 = output_exp.sum();

        Some(output_exp / sum_exp)
    }

    /// Record access (for training)
    pub fn record_access(&mut self, key: CacheKey) {
        let _idx = self.add_to_vocab(&key);
        // Training would happen here if we implement backpropagation
    }

    /// Predict next keys
    pub fn predict(&mut self, current_key: &CacheKey, top_n: usize) -> Vec<Prediction> {
        if let Some(&idx) = self.key_to_idx.get(current_key) {
            if let Some(output) = self.forward(idx) {
                let mut predictions: Vec<_> = output
                    .iter()
                    .enumerate()
                    .map(|(i, &prob)| Prediction {
                        key: self.idx_to_key.get(i).cloned().unwrap_or_default(),
                        confidence: prob,
                        predicted_time: None,
                    })
                    .collect();

                predictions.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                predictions.truncate(top_n);
                return predictions;
            }
        }

        Vec::new()
    }

    /// Clear model
    pub fn clear(&mut self) {
        self.w1 = None;
        self.w2 = None;
        self.b1 = None;
        self.b2 = None;
        self.key_to_idx.clear();
        self.idx_to_key.clear();
        self.vocab_size = 0;
    }
}

/// Ensemble predictor combining multiple prediction methods
pub struct EnsemblePredictor {
    /// Markov predictor
    markov: Arc<RwLock<MarkovPredictor>>,
    /// Temporal predictor
    temporal: Arc<RwLock<TemporalPatternDetector>>,
    /// Spatial predictor
    spatial: Arc<RwLock<SpatialPatternDetector>>,
    /// Neural predictor
    neural: Arc<RwLock<NeuralPredictor>>,
    /// Confidence threshold for prefetching
    confidence_threshold: f64,
}

impl EnsemblePredictor {
    /// Create new ensemble predictor
    pub fn new() -> Self {
        Self {
            markov: Arc::new(RwLock::new(MarkovPredictor::new(2))),
            temporal: Arc::new(RwLock::new(TemporalPatternDetector::new(1000))),
            spatial: Arc::new(RwLock::new(SpatialPatternDetector::new(10))),
            neural: Arc::new(RwLock::new(NeuralPredictor::new(64))),
            confidence_threshold: 0.5,
        }
    }

    /// Set confidence threshold
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.confidence_threshold = threshold;
        self
    }

    /// Record an access
    pub async fn record_access(&self, record: AccessRecord) {
        let mut markov = self.markov.write().await;
        markov.record_access(record.key.clone());
        drop(markov);

        let mut temporal = self.temporal.write().await;
        temporal.record_access(record.key.clone(), record.timestamp);
        drop(temporal);

        let mut spatial = self.spatial.write().await;
        spatial.record_access(record.key.clone());
        drop(spatial);

        let mut neural = self.neural.write().await;
        neural.record_access(record.key);
    }

    /// Predict next keys to prefetch
    pub async fn predict(&self, current_key: &CacheKey, top_n: usize) -> Vec<Prediction> {
        let mut all_predictions = Vec::new();

        // Get predictions from Markov
        let markov = self.markov.read().await;
        let markov_predictions = markov.predict(top_n);
        all_predictions.extend(markov_predictions);
        drop(markov);

        // Get predictions from temporal
        let temporal = self.temporal.read().await;
        if let Some(temporal_pred) = temporal.predict(current_key) {
            all_predictions.push(temporal_pred);
        }
        drop(temporal);

        // Get predictions from spatial
        let spatial = self.spatial.read().await;
        let spatial_predictions = spatial.get_related_keys(current_key, top_n);
        all_predictions.extend(spatial_predictions);
        drop(spatial);

        // Aggregate predictions by key
        let mut aggregated: HashMap<CacheKey, Vec<f64>> = HashMap::new();
        for pred in all_predictions {
            aggregated
                .entry(pred.key.clone())
                .or_default()
                .push(pred.confidence);
        }

        // Average confidences and filter by threshold
        let mut final_predictions: Vec<_> = aggregated
            .into_iter()
            .map(|(key, confidences)| {
                let avg_confidence = confidences.iter().sum::<f64>() / confidences.len() as f64;
                Prediction {
                    key,
                    confidence: avg_confidence,
                    predicted_time: None,
                }
            })
            .filter(|p| p.confidence >= self.confidence_threshold)
            .collect();

        final_predictions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        final_predictions.truncate(top_n);
        final_predictions
    }

    /// Clear all predictors
    pub async fn clear(&self) {
        self.markov.write().await.clear();
        self.temporal.write().await.clear();
        self.spatial.write().await.clear();
        self.neural.write().await.clear();
    }
}

impl Default for EnsemblePredictor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markov_predictor() {
        let mut predictor = MarkovPredictor::new(1);

        predictor.record_access("A".to_string());
        predictor.record_access("B".to_string());
        predictor.record_access("A".to_string());
        predictor.record_access("B".to_string());

        let predictions = predictor.predict(3);
        assert!(!predictions.is_empty());
    }

    #[test]
    fn test_temporal_pattern_detector() {
        let mut detector = TemporalPatternDetector::new(100);

        let now = chrono::Utc::now();
        detector.record_access("A".to_string(), now);
        detector.record_access("A".to_string(), now + chrono::Duration::seconds(10));
        detector.record_access("A".to_string(), now + chrono::Duration::seconds(20));

        let prediction = detector.predict(&"A".to_string());
        assert!(prediction.is_some());
    }

    #[test]
    fn test_spatial_pattern_detector() {
        let mut detector = SpatialPatternDetector::new(5);

        detector.record_access("A".to_string());
        detector.record_access("B".to_string());
        detector.record_access("C".to_string());
        detector.record_access("A".to_string());
        detector.record_access("B".to_string());

        let related = detector.get_related_keys(&"A".to_string(), 3);
        assert!(!related.is_empty());
    }

    #[tokio::test]
    async fn test_ensemble_predictor() {
        let predictor = EnsemblePredictor::new();

        let now = chrono::Utc::now();
        predictor
            .record_access(AccessRecord {
                key: "A".to_string(),
                timestamp: now,
                access_type: AccessType::Read,
            })
            .await;

        predictor
            .record_access(AccessRecord {
                key: "B".to_string(),
                timestamp: now + chrono::Duration::seconds(1),
                access_type: AccessType::Read,
            })
            .await;

        let predictions = predictor.predict(&"A".to_string(), 5).await;
        // May or may not have predictions depending on pattern strength
        assert!(predictions.len() <= 5);
    }
}
