//! Auto-selection tests

use oxigdal_compress::auto_select::*;
use oxigdal_compress::codecs::CodecType;

#[test]
fn test_data_characteristics_analysis() {
    // Low entropy data (uniform)
    let data = vec![42u8; 1000];
    let chars = DataCharacteristics::analyze(&data, DataType::Categorical);

    assert!(chars.entropy < 0.1);
    assert_eq!(chars.unique_count, Some(1));
    assert!(chars.run_length_ratio.is_some());
    assert!(
        chars
            .run_length_ratio
            .expect("run length ratio should be present for uniform data")
            > 100.0
    );
}

#[test]
fn test_data_characteristics_high_entropy() {
    // High entropy data (varied)
    let data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    let chars = DataCharacteristics::analyze(&data, DataType::Generic);

    assert!(chars.entropy > 0.5);
    assert_eq!(chars.unique_count, Some(256));
}

#[test]
fn test_auto_selector_categorical_data() {
    let selector = AutoSelector::new(CompressionGoal::Balanced);

    let chars = DataCharacteristics {
        data_type: DataType::Categorical,
        size: 100000,
        entropy: 0.2,
        unique_count: Some(5),
        value_range: None,
        run_length_ratio: Some(200.0),
    };

    let recommendations = selector.recommend(&chars);

    assert!(!recommendations.is_empty());

    // RLE or Dictionary should be highly recommended
    let top_codecs: Vec<CodecType> = recommendations.iter().take(3).map(|r| r.codec).collect();

    assert!(
        top_codecs.contains(&CodecType::Rle) || top_codecs.contains(&CodecType::Dictionary),
        "Expected RLE or Dictionary in top recommendations for categorical data"
    );
}

#[test]
fn test_auto_selector_continuous_float() {
    let selector = AutoSelector::new(CompressionGoal::Ratio);

    let chars = DataCharacteristics {
        data_type: DataType::ContinuousFloat,
        size: 100000,
        entropy: 0.6,
        unique_count: None,
        value_range: Some((0.0, 1000.0)),
        run_length_ratio: None,
    };

    let recommendations = selector.recommend(&chars);

    assert!(!recommendations.is_empty());

    // Zstd or Brotli should be recommended for high compression ratio goal
    let top_codec = recommendations[0].codec;
    assert!(
        top_codec == CodecType::Zstd || top_codec == CodecType::Brotli,
        "Expected Zstd or Brotli for continuous float with ratio goal"
    );
}

#[test]
fn test_auto_selector_speed_goal() {
    let selector = AutoSelector::new(CompressionGoal::Speed);

    let chars = DataCharacteristics {
        data_type: DataType::Image,
        size: 1000000,
        entropy: 0.5,
        unique_count: Some(256),
        value_range: None,
        run_length_ratio: Some(5.0),
    };

    let recommendations = selector.recommend(&chars);

    assert!(!recommendations.is_empty());

    // Fast codecs should be recommended
    let top_codec = recommendations[0].codec;
    assert!(
        top_codec == CodecType::Snappy || top_codec == CodecType::Lz4,
        "Expected fast codec (Snappy/LZ4) for speed goal"
    );
}

#[test]
fn test_performance_history() {
    use oxigdal_compress::metadata::CompressionMetadata;
    use std::time::Duration;

    let mut history = PerformanceHistory::new();

    // Add some records
    for _ in 0..5 {
        let metadata = CompressionMetadata::new("lz4".to_string(), 1000, 500)
            .with_duration(Duration::from_millis(10));
        history.add_record(CodecType::Lz4, metadata);
    }

    let avg_ratio = history.average_ratio(CodecType::Lz4);
    assert!(avg_ratio.is_some());
    assert!(
        (avg_ratio.expect("average ratio should be calculated from history records") - 2.0).abs()
            < 0.01
    );

    let avg_throughput = history.average_throughput(CodecType::Lz4);
    assert!(avg_throughput.is_some());
}

#[test]
fn test_codec_scoring() {
    let selector = AutoSelector::new(CompressionGoal::Balanced);

    // Test with integer coordinate data (should favor Delta)
    let chars = DataCharacteristics {
        data_type: DataType::IntegerCoordinate,
        size: 50000,
        entropy: 0.4,
        unique_count: Some(1000),
        value_range: None,
        run_length_ratio: Some(10.0),
    };

    let recommendations = selector.recommend(&chars);

    // Find Delta codec in recommendations
    let delta_rec = recommendations.iter().find(|r| r.codec == CodecType::Delta);

    assert!(
        delta_rec.is_some(),
        "Delta codec should be recommended for integer coordinates"
    );

    let delta_score = delta_rec
        .expect("delta codec recommendation should be present for integer coordinates")
        .score;
    assert!(
        delta_score > 60.0,
        "Delta codec should have high score for integer coordinates"
    );
}

#[test]
fn test_multiple_recommendations() {
    let selector = AutoSelector::new(CompressionGoal::Balanced);

    let chars = DataCharacteristics {
        data_type: DataType::Generic,
        size: 10000,
        entropy: 0.5,
        unique_count: Some(100),
        value_range: None,
        run_length_ratio: Some(20.0),
    };

    let recommendations = selector.recommend(&chars);

    // Should have multiple recommendations
    assert!(recommendations.len() >= 3);

    // Scores should be in descending order
    for i in 1..recommendations.len() {
        assert!(
            recommendations[i - 1].score >= recommendations[i].score,
            "Recommendations should be sorted by score"
        );
    }
}
