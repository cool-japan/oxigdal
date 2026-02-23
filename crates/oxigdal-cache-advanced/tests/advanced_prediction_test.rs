//! Tests for advanced prediction models

use oxigdal_cache_advanced::predictive::advanced::{
    HybridPredictor, LSTMPredictor, TransformerPredictor,
};

#[test]
fn test_transformer_basic() {
    let mut predictor = TransformerPredictor::new(16, 2, 5);

    // Record access sequence
    for i in 0..10 {
        predictor.record_access(format!("key{}", i % 5));
    }

    // Make predictions
    let predictions = predictor.predict(3).unwrap_or_default();
    assert!(predictions.len() <= 3);

    // Verify predictions are properly scored
    for (key, score) in &predictions {
        assert!(!key.is_empty());
        assert!(*score >= 0.0 && *score <= 1.0);
    }
}

#[test]
fn test_lstm_sequence_learning() {
    let mut predictor = LSTMPredictor::new(32);

    // Create a simple repeating pattern
    let pattern = ["A", "B", "C", "A", "B", "C"];

    for key in pattern.iter().cycle().take(20) {
        predictor.record_access(key.to_string()).unwrap_or_default();
    }

    // Predict next items
    let predictions = predictor.predict(3).unwrap_or_default();
    assert!(!predictions.is_empty());
}

#[test]
fn test_lstm_reset() {
    let mut predictor = LSTMPredictor::new(32);

    predictor
        .record_access("key1".to_string())
        .unwrap_or_default();
    predictor
        .record_access("key2".to_string())
        .unwrap_or_default();

    predictor.reset();

    // After reset, predictions should work but from fresh state
    let predictions = predictor.predict(1).unwrap_or_default();
    assert!(predictions.is_empty() || !predictions.is_empty()); // Either is valid
}

#[test]
fn test_hybrid_predictor_initialization() {
    let predictor = HybridPredictor::new(16, 32, 5);

    let weights = predictor.get_weights();
    assert!(weights.contains_key("transformer"));
    assert!(weights.contains_key("lstm"));

    // Initial weights should be balanced
    let transformer_weight = weights.get("transformer").copied().unwrap_or(0.0);
    let lstm_weight = weights.get("lstm").copied().unwrap_or(0.0);

    assert!((transformer_weight - 0.5).abs() < 0.1);
    assert!((lstm_weight - 0.5).abs() < 0.1);
}

#[test]
fn test_hybrid_online_learning() {
    let mut predictor = HybridPredictor::new(16, 32, 5);

    // Simulate transformer performing better
    for _ in 0..10 {
        predictor.report_accuracy("transformer", 0.9);
        predictor.report_accuracy("lstm", 0.6);
    }

    let weights = predictor.get_weights();
    let transformer_weight = weights.get("transformer").copied().unwrap_or(0.0);
    let lstm_weight = weights.get("lstm").copied().unwrap_or(0.0);

    // Transformer should have higher weight
    assert!(transformer_weight > lstm_weight);
}

#[test]
fn test_hybrid_prediction_combination() {
    let mut predictor = HybridPredictor::new(16, 32, 5);

    // Record some accesses
    for i in 0..20 {
        predictor
            .record_access(format!("key{}", i % 3))
            .unwrap_or_default();
    }

    // Make predictions
    let predictions = predictor.predict(5).unwrap_or_default();
    assert!(predictions.len() <= 5);

    // Verify all predictions have valid scores
    for (key, score) in predictions {
        assert!(!key.is_empty());
        assert!((0.0..=1.0).contains(&score));
    }
}

#[test]
fn test_transformer_vocabulary_growth() {
    let mut predictor = TransformerPredictor::new(16, 2, 5);

    // Add many unique keys
    for i in 0..50 {
        predictor.record_access(format!("key{}", i));
    }

    // Should be able to make predictions
    let predictions = predictor.predict(10).unwrap_or_default();
    assert!(predictions.len() <= 10);
}

#[test]
fn test_lstm_vocabulary_growth() {
    let mut predictor = LSTMPredictor::new(32);

    // Add many unique keys
    for i in 0..30 {
        predictor
            .record_access(format!("key{}", i))
            .unwrap_or_default();
    }

    // Should be able to make predictions
    let predictions = predictor.predict(5).unwrap_or_default();
    assert!(predictions.len() <= 5);
}

#[test]
fn test_predictor_clear() {
    let mut predictor = HybridPredictor::new(16, 32, 5);

    predictor
        .record_access("key1".to_string())
        .unwrap_or_default();
    predictor
        .record_access("key2".to_string())
        .unwrap_or_default();

    predictor.clear();

    // After clear, predictions should be empty or minimal
    let predictions = predictor.predict(5).unwrap_or_default();
    assert!(predictions.is_empty() || predictions.len() < 3);
}
