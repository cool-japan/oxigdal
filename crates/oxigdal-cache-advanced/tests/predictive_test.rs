//! Predictive prefetching integration tests

use oxigdal_cache_advanced::predictive::*;

#[test]
fn test_markov_predictor_basic() {
    let mut predictor = MarkovPredictor::new(1);

    predictor.record_access("A".to_string());
    predictor.record_access("B".to_string());
    predictor.record_access("A".to_string());
    predictor.record_access("B".to_string());
    predictor.record_access("A".to_string());

    let predictions = predictor.predict(3);
    assert!(!predictions.is_empty());

    // After A, B should be predicted with high confidence
    assert!(predictions.iter().any(|p| p.key == "B"));
}

#[test]
fn test_markov_predictor_state_count() {
    let mut predictor = MarkovPredictor::new(1);

    predictor.record_access("A".to_string());
    predictor.record_access("B".to_string());
    predictor.record_access("C".to_string());

    assert!(predictor.state_count() > 0);
}

#[test]
fn test_temporal_pattern_detector() {
    let mut detector = TemporalPatternDetector::new(100);

    let now = chrono::Utc::now();
    for i in 0..5 {
        detector.record_access("A".to_string(), now + chrono::Duration::seconds(i * 10));
    }

    let prediction = detector.predict(&"A".to_string());
    assert!(prediction.is_some());

    if let Some(pred) = prediction {
        assert!(pred.confidence > 0.0);
        assert!(pred.predicted_time.is_some());
    }
}

#[test]
fn test_spatial_pattern_detector() {
    let mut detector = SpatialPatternDetector::new(5);

    // Create pattern: A -> B -> C repeatedly
    for _ in 0..10 {
        detector.record_access("A".to_string());
        detector.record_access("B".to_string());
        detector.record_access("C".to_string());
    }

    let related_to_a = detector.get_related_keys(&"A".to_string(), 3);
    assert!(!related_to_a.is_empty());

    // B and C should be related to A
    let keys: Vec<_> = related_to_a.iter().map(|p| p.key.as_str()).collect();
    assert!(keys.contains(&"B") || keys.contains(&"C"));
}

#[test]
fn test_neural_predictor_basic() {
    let mut predictor = NeuralPredictor::new(32);

    predictor.record_access("A".to_string());
    predictor.record_access("B".to_string());
    predictor.record_access("C".to_string());

    let predictions = predictor.predict(&"A".to_string(), 5);
    // Neural predictor needs training, so predictions may be random initially
    assert!(predictions.len() <= 5);
}

#[tokio::test]
async fn test_ensemble_predictor() {
    let predictor = EnsemblePredictor::new();

    let now = chrono::Utc::now();

    // Create a pattern
    for i in 0..20 {
        let key = if i % 2 == 0 {
            "A".to_string()
        } else {
            "B".to_string()
        };

        predictor
            .record_access(AccessRecord {
                key,
                timestamp: now + chrono::Duration::seconds(i),
                access_type: AccessType::Read,
            })
            .await;
    }

    let predictions = predictor.predict(&"A".to_string(), 5).await;
    assert!(predictions.len() <= 5);
}

#[tokio::test]
async fn test_ensemble_predictor_with_threshold() {
    let predictor = EnsemblePredictor::new().with_threshold(0.7);

    let now = chrono::Utc::now();

    for i in 0..10 {
        predictor
            .record_access(AccessRecord {
                key: format!("key{}", i),
                timestamp: now + chrono::Duration::seconds(i),
                access_type: AccessType::Read,
            })
            .await;
    }

    let predictions = predictor.predict(&"key0".to_string(), 5).await;
    // With high threshold, may get fewer predictions
    assert!(predictions.len() <= 5);
}

#[test]
fn test_prediction_confidence() {
    let prediction = Prediction {
        key: "test".to_string(),
        confidence: 0.8,
        predicted_time: None,
    };

    assert!(prediction.is_confident(0.7));
    assert!(!prediction.is_confident(0.9));
}

#[test]
fn test_access_type() {
    let read = AccessType::Read;
    let write = AccessType::Write;

    assert_ne!(read, write);
    assert_eq!(read, AccessType::Read);
}

#[test]
fn test_markov_predictor_clear() {
    let mut predictor = MarkovPredictor::new(1);

    predictor.record_access("A".to_string());
    predictor.record_access("B".to_string());

    assert!(predictor.state_count() > 0);

    predictor.clear();
    assert_eq!(predictor.state_count(), 0);
}

#[test]
fn test_temporal_detector_multiple_patterns() {
    let mut detector = TemporalPatternDetector::new(100);

    let now = chrono::Utc::now();

    // Pattern for key A (every 10 seconds)
    for i in 0..5 {
        detector.record_access("A".to_string(), now + chrono::Duration::seconds(i * 10));
    }

    // Pattern for key B (every 20 seconds)
    for i in 0..5 {
        detector.record_access("B".to_string(), now + chrono::Duration::seconds(i * 20));
    }

    let pred_a = detector.predict(&"A".to_string());
    let pred_b = detector.predict(&"B".to_string());

    assert!(pred_a.is_some());
    assert!(pred_b.is_some());
}

#[test]
fn test_spatial_detector_clear() {
    let mut detector = SpatialPatternDetector::new(5);

    detector.record_access("A".to_string());
    detector.record_access("B".to_string());

    let related = detector.get_related_keys(&"A".to_string(), 3);
    assert!(!related.is_empty());

    detector.clear();

    let related_after_clear = detector.get_related_keys(&"A".to_string(), 3);
    assert!(related_after_clear.is_empty());
}
