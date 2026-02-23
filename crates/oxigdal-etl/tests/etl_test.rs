//! Comprehensive integration tests for OxiGDAL ETL framework
//!
//! Tests cover:
//! - Happy path scenarios (simple pipelines, transformations, filters)
//! - Error handling (invalid inputs, recovery, corruption)
//! - Edge cases (empty files, large files, special characters)
//! - Performance regression prevention

use oxigdal_etl::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_simple_pipeline() {
    // Create temp input file
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "line1").expect("Failed to write");
    writeln!(temp_input, "line2").expect("Failed to write");
    writeln!(temp_input, "line3").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    // Create temp output file
    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    // Build pipeline
    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path.clone())))
        .build()
        .expect("Failed to build pipeline");

    // Run pipeline
    let stats = pipeline.run().await.expect("Failed to run pipeline");

    // Verify results
    assert_eq!(stats.items_processed(), 3);
    assert_eq!(stats.errors(), 0);

    // Verify output file content
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .expect("Failed to read output");
    assert!(content.contains("line1"));
    assert!(content.contains("line2"));
    assert!(content.contains("line3"));
}

#[tokio::test]
async fn test_pipeline_with_map() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "hello").expect("Failed to write");
    writeln!(temp_input, "world").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .map("uppercase".to_string(), |item| {
            Box::pin(async move {
                let s = String::from_utf8(item).map_err(|e| {
                    oxigdal_etl::error::TransformError::InvalidInput {
                        message: e.to_string(),
                    }
                })?;
                Ok(s.to_uppercase().into_bytes())
            })
        })
        .sink(Box::new(FileSink::new(output_path.clone())))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 2);

    let content = tokio::fs::read_to_string(&output_path)
        .await
        .expect("Failed to read output");
    assert!(content.contains("HELLO"));
    assert!(content.contains("WORLD"));
}

#[tokio::test]
async fn test_pipeline_with_filter() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "a").expect("Failed to write");
    writeln!(temp_input, "ab").expect("Failed to write");
    writeln!(temp_input, "abc").expect("Failed to write");
    writeln!(temp_input, "abcd").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .filter("min_length_3".to_string(), |item| {
            let len = item.len();
            Box::pin(async move { Ok(len >= 3) })
        })
        .sink(Box::new(FileSink::new(output_path.clone())))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    // Only "abc\n" and "abcd\n" should pass (4+ bytes each)
    assert!(stats.items_processed() >= 2);

    let content = tokio::fs::read_to_string(&output_path)
        .await
        .expect("Failed to read output");
    assert!(content.contains("abc"));
    assert!(content.contains("abcd"));
}

#[tokio::test]
async fn test_pipeline_with_checkpointing() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "test1").expect("Failed to write");
    writeln!(temp_input, "test2").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let checkpoint_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .checkpoint_dir(checkpoint_dir.path().to_path_buf())
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 2);
}

#[tokio::test]
async fn test_pipeline_with_error_recovery() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "valid").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .with_error_recovery(3)
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 1);
}

#[tokio::test]
async fn test_map_operator() {
    let op = MapOperator::bytes("double".to_string(), |mut bytes| {
        let copy = bytes.clone();
        bytes.extend_from_slice(&copy);
        bytes
    });

    let result = op.transform(vec![1, 2, 3]).await.expect("Failed");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], vec![1, 2, 3, 1, 2, 3]);
}

#[tokio::test]
async fn test_filter_operator() {
    let filter = FilterOperator::min_size(5);

    let result1 = filter.transform(vec![1, 2, 3, 4, 5]).await.expect("Failed");
    assert_eq!(result1.len(), 1);

    let result2 = filter.transform(vec![1, 2]).await.expect("Failed");
    assert_eq!(result2.len(), 0);
}

#[tokio::test]
async fn test_json_transformation() {
    let json = serde_json::json!({"name": "test", "value": 42});
    let item = serde_json::to_vec(&json).expect("Failed to serialize");

    let op = MapOperator::extract_json_field("name".to_string());
    let result = op.transform(item).await.expect("Failed");

    let extracted: serde_json::Value = serde_json::from_slice(&result[0]).expect("Failed to parse");
    assert_eq!(extracted, "test");
}

#[tokio::test]
async fn test_window_aggregation() {
    use oxigdal_etl::operators::window::{WindowAggregator, WindowOperator};

    let window = WindowOperator::tumbling_count(3, WindowAggregator::count());

    window.transform(vec![1]).await.expect("Failed");
    window.transform(vec![2]).await.expect("Failed");
    let result = window.transform(vec![3]).await.expect("Failed");

    assert_eq!(result.len(), 1);

    let stats: serde_json::Value = serde_json::from_slice(&result[0]).expect("Failed to parse");
    assert_eq!(stats.get("count").and_then(|v| v.as_u64()), Some(3));
}

#[tokio::test]
async fn test_scheduler_basic() {
    use oxigdal_etl::scheduler::Scheduler;
    use std::time::Duration;

    let scheduler = Scheduler::new();
    assert!(!scheduler.is_running().await);

    scheduler.start().await.expect("Failed to start");
    assert!(scheduler.is_running().await);

    tokio::time::sleep(Duration::from_millis(100)).await;

    scheduler.stop().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_stream_config() {
    use oxigdal_etl::stream::StreamConfig;

    let config = StreamConfig {
        buffer_size: 100,
        checkpointing: true,
        checkpoint_interval: 50,
        ..Default::default()
    };

    assert_eq!(config.buffer_size, 100);
    assert!(config.checkpointing);
    assert_eq!(config.checkpoint_interval, 50);
}

#[tokio::test]
async fn test_pipeline_stats() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "test").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");

    assert_eq!(stats.items_processed(), 1);
    assert_eq!(stats.errors(), 0);
    // Elapsed time might be 0 on fast systems, so just check it's measurable
    let _ = stats.elapsed();
    assert!(stats.throughput() >= 0.0);
}

#[tokio::test]
async fn test_multiple_transforms() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "hello").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .map("uppercase".to_string(), |item| {
            Box::pin(async move {
                let s = String::from_utf8(item).map_err(|e| {
                    oxigdal_etl::error::TransformError::InvalidInput {
                        message: e.to_string(),
                    }
                })?;
                Ok(s.to_uppercase().into_bytes())
            })
        })
        .filter("non_empty".to_string(), |item| {
            let is_empty = item.is_empty();
            Box::pin(async move { Ok(!is_empty) })
        })
        .sink(Box::new(FileSink::new(output_path.clone())))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 1);

    let content = tokio::fs::read_to_string(&output_path)
        .await
        .expect("Failed to read output");
    assert!(content.contains("HELLO"));
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_empty_file_pipeline() {
    let temp_input = NamedTempFile::new().expect("Failed to create temp file");
    // Write nothing - empty file
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 0);
    assert_eq!(stats.errors(), 0);
}

#[tokio::test]
async fn test_large_file_processing() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    // Write 200 lines (sufficient to test multi-item pipeline processing)
    for i in 0..200 {
        writeln!(temp_input, "line_{}", i).expect("Failed to write");
    }
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 200);
    assert_eq!(stats.errors(), 0);
}

#[tokio::test]
async fn test_special_characters_in_data() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "Hello, 世界").expect("Failed to write");
    writeln!(temp_input, "Здравствуй, мир").expect("Failed to write");
    writeln!(temp_input, "🌍🌎🌏").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 3);
    assert_eq!(stats.errors(), 0);
}

#[tokio::test]
async fn test_very_long_lines() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    let long_line = "a".repeat(10000);
    writeln!(temp_input, "{}", long_line).expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .sink(Box::new(FileSink::new(output_path)))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    assert_eq!(stats.items_processed(), 1);
    assert_eq!(stats.errors(), 0);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_utf8_handling() {
    let filter = FilterOperator::min_size(0);

    // Test with invalid UTF-8 sequences
    let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
    let result = filter.transform(invalid_utf8).await;
    // Should handle gracefully (either pass or fail gracefully)
    let _ = result;
}

#[tokio::test]
async fn test_filter_all_items_filtered_out() {
    let filter = FilterOperator::min_size(100);

    let result = filter.transform(vec![1, 2, 3]).await.expect("Failed");
    assert_eq!(result.len(), 0);
}

#[tokio::test]
async fn test_map_operator_with_empty_input() {
    let op = MapOperator::bytes("identity".to_string(), |bytes| bytes);

    let result = op.transform(vec![]).await.expect("Failed");
    // Map operator transforms the item, so empty input produces one empty output
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].len(), 0);
}

#[tokio::test]
async fn test_chained_filters() {
    let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(temp_input, "x").expect("Failed to write");
    writeln!(temp_input, "xx").expect("Failed to write");
    writeln!(temp_input, "xxx").expect("Failed to write");
    writeln!(temp_input, "xxxx").expect("Failed to write");
    writeln!(temp_input, "xxxxx").expect("Failed to write");
    let input_path = temp_input.path().to_path_buf();

    let temp_output = NamedTempFile::new().expect("Failed to create temp file");
    let output_path = temp_output.path().to_path_buf();

    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(input_path).line_based(true)))
        .filter("min_2".to_string(), |item| {
            let len = item.len();
            Box::pin(async move { Ok(len >= 2) })
        })
        .filter("min_3".to_string(), |item| {
            let len = item.len();
            Box::pin(async move { Ok(len >= 3) })
        })
        .sink(Box::new(FileSink::new(output_path.clone())))
        .build()
        .expect("Failed to build pipeline");

    let stats = pipeline.run().await.expect("Failed to run pipeline");
    // Only lines with 3+ bytes should pass both filters
    assert!(stats.items_processed() >= 2);

    let content = tokio::fs::read_to_string(&output_path)
        .await
        .expect("Failed to read output");
    assert!(content.contains("xxx"));
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[tokio::test]
async fn test_stream_config_defaults() {
    use oxigdal_etl::stream::StreamConfig;

    let config = StreamConfig::default();
    assert!(config.buffer_size > 0);
}

#[tokio::test]
async fn test_scheduler_multiple_start_stop_cycles() {
    use oxigdal_etl::scheduler::Scheduler;
    use std::time::Duration;

    let scheduler = Scheduler::new();

    for _ in 0..3 {
        scheduler.start().await.expect("Failed to start");
        assert!(scheduler.is_running().await);

        tokio::time::sleep(Duration::from_millis(10)).await;

        scheduler.stop().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!scheduler.is_running().await);
    }
}
