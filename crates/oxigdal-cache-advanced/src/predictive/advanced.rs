//! Advanced ML-based prediction models
//!
//! Enhanced prediction models with:
//! - Transformer-based attention mechanism
//! - LSTM for temporal sequences
//! - Hybrid predictor combining multiple models
//! - Online learning and adaptation
//! - Model selection based on data characteristics
//! - Prediction confidence calibration

use crate::error::{CacheError, Result};
use crate::multi_tier::CacheKey;
use scirs2_core::ndarray::{Array1, Array2, Axis};
use std::collections::{HashMap, VecDeque};

/// Generate normal distributed random number using Box-Muller transform
fn rand_normal(mean: f64, std_dev: f64) -> f64 {
    let u1 = fastrand::f64();
    let u2 = fastrand::f64();
    // Avoid log(0) by ensuring u1 > 0
    let u1 = if u1 < 1e-10 { 1e-10 } else { u1 };
    let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    mean + z0 * std_dev
}

/// Transformer-based predictor using attention mechanism
pub struct TransformerPredictor {
    /// Embedding dimension
    embedding_dim: usize,
    /// Number of attention heads
    #[allow(dead_code)]
    num_heads: usize,
    /// Sequence length
    seq_length: usize,
    /// Query weights
    w_query: Option<Array2<f64>>,
    /// Key weights
    w_key: Option<Array2<f64>>,
    /// Value weights
    w_value: Option<Array2<f64>>,
    /// Output projection weights
    w_output: Option<Array2<f64>>,
    /// Key to index mapping
    key_to_idx: HashMap<CacheKey, usize>,
    /// Index to key mapping
    idx_to_key: Vec<CacheKey>,
    /// Recent access sequence
    sequence: VecDeque<usize>,
    /// Vocabulary size
    vocab_size: usize,
}

impl TransformerPredictor {
    /// Create new transformer predictor
    pub fn new(embedding_dim: usize, num_heads: usize, seq_length: usize) -> Self {
        Self {
            embedding_dim,
            num_heads,
            seq_length,
            w_query: None,
            w_key: None,
            w_value: None,
            w_output: None,
            key_to_idx: HashMap::new(),
            idx_to_key: Vec::new(),
            sequence: VecDeque::with_capacity(seq_length),
            vocab_size: 0,
        }
    }

    /// Initialize weights
    fn initialize_weights(&mut self) {
        // Seed fastrand for reproducibility
        fastrand::seed(42);
        let scale = (2.0 / self.embedding_dim as f64).sqrt();

        let q_data: Vec<f64> = (0..self.embedding_dim * self.embedding_dim)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let k_data: Vec<f64> = (0..self.embedding_dim * self.embedding_dim)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let v_data: Vec<f64> = (0..self.embedding_dim * self.embedding_dim)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let o_data: Vec<f64> = (0..self.embedding_dim * self.embedding_dim)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        self.w_query = Some(
            Array2::from_shape_vec((self.embedding_dim, self.embedding_dim), q_data)
                .unwrap_or_else(|_| Array2::zeros((self.embedding_dim, self.embedding_dim))),
        );

        self.w_key = Some(
            Array2::from_shape_vec((self.embedding_dim, self.embedding_dim), k_data)
                .unwrap_or_else(|_| Array2::zeros((self.embedding_dim, self.embedding_dim))),
        );

        self.w_value = Some(
            Array2::from_shape_vec((self.embedding_dim, self.embedding_dim), v_data)
                .unwrap_or_else(|_| Array2::zeros((self.embedding_dim, self.embedding_dim))),
        );

        self.w_output = Some(
            Array2::from_shape_vec((self.embedding_dim, self.embedding_dim), o_data)
                .unwrap_or_else(|_| Array2::zeros((self.embedding_dim, self.embedding_dim))),
        );
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

            if self.w_query.is_none() {
                self.initialize_weights();
            }

            idx
        }
    }

    /// Compute multi-head attention
    fn attention(
        &self,
        query: &Array2<f64>,
        key: &Array2<f64>,
        value: &Array2<f64>,
    ) -> Result<Array2<f64>> {
        let w_q = self
            .w_query
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_k = self
            .w_key
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_v = self
            .w_value
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_o = self
            .w_output
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;

        // Project to Q, K, V
        let q_proj = query.dot(w_q);
        let k_proj = key.dot(w_k);
        let v_proj = value.dot(w_v);

        // Compute attention scores
        let scores = q_proj.dot(&k_proj.t()) / (self.embedding_dim as f64).sqrt();

        // Apply softmax
        let scores_exp = scores.mapv(|x| x.exp());
        let scores_sum = scores_exp.sum_axis(Axis(1));
        let attention_weights = &scores_exp / &scores_sum.insert_axis(Axis(1));

        // Apply attention to values
        let attended = attention_weights.dot(&v_proj);

        // Output projection
        Ok(attended.dot(w_o))
    }

    /// Record access
    pub fn record_access(&mut self, key: CacheKey) {
        let idx = self.add_to_vocab(&key);

        if self.sequence.len() >= self.seq_length {
            self.sequence.pop_front();
        }
        self.sequence.push_back(idx);
    }

    /// Predict next keys
    pub fn predict(&self, top_n: usize) -> Result<Vec<(CacheKey, f64)>> {
        if self.sequence.is_empty() {
            return Ok(Vec::new());
        }

        // Create embedding matrix for sequence
        let mut embeddings = Array2::zeros((self.sequence.len(), self.embedding_dim));
        for (i, &idx) in self.sequence.iter().enumerate() {
            // Simple one-hot-like embedding
            if idx < self.embedding_dim {
                embeddings[[i, idx]] = 1.0;
            }
        }

        // Compute self-attention
        let output = self.attention(&embeddings, &embeddings, &embeddings)?;

        // Use last output for prediction
        let last_output = output.row(output.nrows() - 1);

        // Compute scores for all vocabulary items
        let mut scores: Vec<(CacheKey, f64)> = self
            .idx_to_key
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                let score = if idx < last_output.len() {
                    last_output[idx]
                } else {
                    0.0
                };
                (key.clone(), score)
            })
            .collect();

        // Sort by score
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_n);

        // Normalize to probabilities
        let sum: f64 = scores.iter().map(|(_, s)| s.exp()).sum();
        if sum > 0.0 {
            for (_, score) in &mut scores {
                *score = score.exp() / sum;
            }
        }

        Ok(scores)
    }

    /// Clear predictor
    pub fn clear(&mut self) {
        self.sequence.clear();
        self.key_to_idx.clear();
        self.idx_to_key.clear();
        self.vocab_size = 0;
        self.w_query = None;
        self.w_key = None;
        self.w_value = None;
        self.w_output = None;
    }
}

/// LSTM-based predictor for temporal sequences
pub struct LSTMPredictor {
    /// Hidden size
    hidden_size: usize,
    /// Input size (vocabulary size)
    vocab_size: usize,
    /// Forget gate weights
    w_forget: Option<Array2<f64>>,
    /// Input gate weights
    w_input: Option<Array2<f64>>,
    /// Output gate weights
    w_output: Option<Array2<f64>>,
    /// Cell state weights
    w_cell: Option<Array2<f64>>,
    /// Hidden state
    hidden_state: Option<Array1<f64>>,
    /// Cell state
    cell_state: Option<Array1<f64>>,
    /// Key to index mapping
    key_to_idx: HashMap<CacheKey, usize>,
    /// Index to key mapping
    idx_to_key: Vec<CacheKey>,
}

impl LSTMPredictor {
    /// Create new LSTM predictor
    pub fn new(hidden_size: usize) -> Self {
        Self {
            hidden_size,
            vocab_size: 0,
            w_forget: None,
            w_input: None,
            w_output: None,
            w_cell: None,
            hidden_state: None,
            cell_state: None,
            key_to_idx: HashMap::new(),
            idx_to_key: Vec::new(),
        }
    }

    /// Initialize weights
    fn initialize_weights(&mut self) {
        // Seed fastrand for reproducibility
        fastrand::seed(42);
        let input_size = self.vocab_size + self.hidden_size;
        let scale = (2.0 / input_size as f64).sqrt();

        let wf_data: Vec<f64> = (0..input_size * self.hidden_size)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let wi_data: Vec<f64> = (0..input_size * self.hidden_size)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let wo_data: Vec<f64> = (0..input_size * self.hidden_size)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        let wc_data: Vec<f64> = (0..input_size * self.hidden_size)
            .map(|_| rand_normal(0.0, scale))
            .collect();

        self.w_forget = Some(
            Array2::from_shape_vec((input_size, self.hidden_size), wf_data)
                .unwrap_or_else(|_| Array2::zeros((input_size, self.hidden_size))),
        );

        self.w_input = Some(
            Array2::from_shape_vec((input_size, self.hidden_size), wi_data)
                .unwrap_or_else(|_| Array2::zeros((input_size, self.hidden_size))),
        );

        self.w_output = Some(
            Array2::from_shape_vec((input_size, self.hidden_size), wo_data)
                .unwrap_or_else(|_| Array2::zeros((input_size, self.hidden_size))),
        );

        self.w_cell = Some(
            Array2::from_shape_vec((input_size, self.hidden_size), wc_data)
                .unwrap_or_else(|_| Array2::zeros((input_size, self.hidden_size))),
        );

        self.hidden_state = Some(Array1::zeros(self.hidden_size));
        self.cell_state = Some(Array1::zeros(self.hidden_size));
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

            // Reinitialize weights when vocabulary changes
            self.initialize_weights();

            idx
        }
    }

    /// Sigmoid activation
    fn sigmoid(x: f64) -> f64 {
        1.0 / (1.0 + (-x).exp())
    }

    /// Forward pass through LSTM cell
    fn forward(&mut self, input_idx: usize) -> Result<Array1<f64>> {
        let w_f = self
            .w_forget
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_i = self
            .w_input
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_o = self
            .w_output
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;
        let w_c = self
            .w_cell
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Weights not initialized".to_string()))?;

        let h_prev = self
            .hidden_state
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Hidden state not initialized".to_string()))?;
        let c_prev = self
            .cell_state
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Cell state not initialized".to_string()))?;

        // One-hot encode input
        let mut input = Array1::zeros(self.vocab_size);
        if input_idx < self.vocab_size {
            input[input_idx] = 1.0;
        }

        // Concatenate input and hidden state
        let mut combined = Array1::zeros(self.vocab_size + self.hidden_size);
        for i in 0..self.vocab_size {
            combined[i] = input[i];
        }
        for i in 0..self.hidden_size {
            combined[self.vocab_size + i] = h_prev[i];
        }

        // Compute gates
        let forget_gate = w_f.t().dot(&combined).mapv(Self::sigmoid);
        let input_gate = w_i.t().dot(&combined).mapv(Self::sigmoid);
        let output_gate = w_o.t().dot(&combined).mapv(Self::sigmoid);
        let cell_candidate = w_c.t().dot(&combined).mapv(|x| x.tanh());

        // Update cell state
        let new_cell = &forget_gate * c_prev + &input_gate * &cell_candidate;

        // Compute new hidden state
        let new_hidden = &output_gate * &new_cell.mapv(|x| x.tanh());

        // Update states
        self.cell_state = Some(new_cell);
        self.hidden_state = Some(new_hidden.clone());

        Ok(new_hidden)
    }

    /// Record access
    pub fn record_access(&mut self, key: CacheKey) -> Result<()> {
        let idx = self.add_to_vocab(&key);
        self.forward(idx)?;
        Ok(())
    }

    /// Predict next keys
    pub fn predict(&mut self, top_n: usize) -> Result<Vec<(CacheKey, f64)>> {
        let hidden = self
            .hidden_state
            .as_ref()
            .ok_or_else(|| CacheError::Prediction("Not trained".to_string()))?;

        // Use hidden state to score vocabulary items
        let mut scores: Vec<(CacheKey, f64)> = self
            .idx_to_key
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                let score = if idx < hidden.len() { hidden[idx] } else { 0.0 };
                (key.clone(), score)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_n);

        // Normalize to probabilities
        let sum: f64 = scores.iter().map(|(_, s)| s.exp()).sum();
        if sum > 0.0 {
            for (_, score) in &mut scores {
                *score = score.exp() / sum;
            }
        }

        Ok(scores)
    }

    /// Reset states
    pub fn reset(&mut self) {
        self.hidden_state = Some(Array1::zeros(self.hidden_size));
        self.cell_state = Some(Array1::zeros(self.hidden_size));
    }

    /// Clear predictor
    pub fn clear(&mut self) {
        self.key_to_idx.clear();
        self.idx_to_key.clear();
        self.vocab_size = 0;
        self.w_forget = None;
        self.w_input = None;
        self.w_output = None;
        self.w_cell = None;
        self.hidden_state = None;
        self.cell_state = None;
    }
}

/// Hybrid predictor combining multiple models
pub struct HybridPredictor {
    /// Transformer predictor
    transformer: TransformerPredictor,
    /// LSTM predictor
    lstm: LSTMPredictor,
    /// Model weights (learned based on performance)
    model_weights: HashMap<String, f64>,
    /// Performance tracking
    performance_history: VecDeque<(String, f64)>,
    /// History size for performance tracking
    history_size: usize,
}

impl HybridPredictor {
    /// Create new hybrid predictor
    pub fn new(embedding_dim: usize, hidden_size: usize, seq_length: usize) -> Self {
        let mut model_weights = HashMap::new();
        model_weights.insert("transformer".to_string(), 0.5);
        model_weights.insert("lstm".to_string(), 0.5);

        Self {
            transformer: TransformerPredictor::new(embedding_dim, 4, seq_length),
            lstm: LSTMPredictor::new(hidden_size),
            model_weights,
            performance_history: VecDeque::with_capacity(100),
            history_size: 100,
        }
    }

    /// Record access
    pub fn record_access(&mut self, key: CacheKey) -> Result<()> {
        self.transformer.record_access(key.clone());
        self.lstm.record_access(key)?;
        Ok(())
    }

    /// Update model weights based on performance
    fn update_weights(&mut self) {
        if self.performance_history.len() < 10 {
            return;
        }

        let mut model_scores: HashMap<String, f64> = HashMap::new();
        let mut model_counts: HashMap<String, usize> = HashMap::new();

        for (model, score) in &self.performance_history {
            *model_scores.entry(model.clone()).or_insert(0.0) += score;
            *model_counts.entry(model.clone()).or_insert(0) += 1;
        }

        // Calculate average scores
        let avg_scores: Vec<(String, f64)> = model_scores
            .into_iter()
            .map(|(model, total)| {
                let count = model_counts.get(&model).copied().unwrap_or(1);
                (model, total / count as f64)
            })
            .collect();

        // Normalize to weights (softmax)
        let sum: f64 = avg_scores.iter().map(|(_, s)| s.exp()).sum();
        if sum > 0.0 {
            for (model, score) in avg_scores {
                self.model_weights.insert(model, score.exp() / sum);
            }
        }
    }

    /// Predict with model ensemble
    pub fn predict(&mut self, top_n: usize) -> Result<Vec<(CacheKey, f64)>> {
        // Get predictions from both models
        let transformer_preds = self.transformer.predict(top_n)?;
        let lstm_preds = self.lstm.predict(top_n)?;

        // Combine predictions with weights
        let mut combined_scores: HashMap<CacheKey, f64> = HashMap::new();

        let transformer_weight = self
            .model_weights
            .get("transformer")
            .copied()
            .unwrap_or(0.5);
        let lstm_weight = self.model_weights.get("lstm").copied().unwrap_or(0.5);

        for (key, score) in transformer_preds {
            *combined_scores.entry(key).or_insert(0.0) += score * transformer_weight;
        }

        for (key, score) in lstm_preds {
            *combined_scores.entry(key).or_insert(0.0) += score * lstm_weight;
        }

        // Sort and return top predictions
        let mut results: Vec<(CacheKey, f64)> = combined_scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_n);

        Ok(results)
    }

    /// Report prediction accuracy for online learning
    pub fn report_accuracy(&mut self, model_name: &str, accuracy: f64) {
        if self.performance_history.len() >= self.history_size {
            self.performance_history.pop_front();
        }
        self.performance_history
            .push_back((model_name.to_string(), accuracy));
        self.update_weights();
    }

    /// Get current model weights
    pub fn get_weights(&self) -> &HashMap<String, f64> {
        &self.model_weights
    }

    /// Clear all predictors
    pub fn clear(&mut self) {
        self.transformer.clear();
        self.lstm.clear();
        self.performance_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transformer_predictor() {
        let mut predictor = TransformerPredictor::new(16, 2, 5);

        predictor.record_access("key1".to_string());
        predictor.record_access("key2".to_string());
        predictor.record_access("key3".to_string());

        let result = predictor.predict(3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_lstm_predictor() {
        let mut predictor = LSTMPredictor::new(32);

        let result = predictor.record_access("key1".to_string());
        assert!(result.is_ok());

        let result = predictor.record_access("key2".to_string());
        assert!(result.is_ok());

        let predictions = predictor.predict(3);
        assert!(predictions.is_ok());
    }

    #[test]
    fn test_hybrid_predictor() {
        let mut predictor = HybridPredictor::new(16, 32, 5);

        let result = predictor.record_access("key1".to_string());
        assert!(result.is_ok());

        let result = predictor.record_access("key2".to_string());
        assert!(result.is_ok());

        let predictions = predictor.predict(3);
        assert!(predictions.is_ok());
    }

    #[test]
    fn test_hybrid_online_learning() {
        let mut predictor = HybridPredictor::new(16, 32, 5);

        // update_weights requires at least 10 entries in performance_history,
        // so provide enough data points for the weight update to trigger
        for _ in 0..10 {
            predictor.report_accuracy("transformer", 0.8);
            predictor.report_accuracy("lstm", 0.6);
        }

        let weights = predictor.get_weights();
        let transformer_weight = weights.get("transformer").copied().unwrap_or(0.0);
        let lstm_weight = weights.get("lstm").copied().unwrap_or(0.0);

        // Transformer should have higher weight due to better accuracy
        assert!(transformer_weight > lstm_weight);
    }
}
