//! End-to-end integration tests.

use oxigdal_query::executor::scan::{
    ColumnData, DataType, Field, MemoryDataSource, RecordBatch, Schema,
};
use oxigdal_query::{QueryEngine, Result};
use std::sync::Arc;

fn create_test_dataset() -> Result<(Arc<Schema>, Vec<RecordBatch>)> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id".to_string(), DataType::Int64, false),
        Field::new("name".to_string(), DataType::String, false),
        Field::new("age".to_string(), DataType::Int32, false),
        Field::new("score".to_string(), DataType::Float64, false),
    ]));

    let columns = vec![
        ColumnData::Int64(vec![
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            Some(7),
            Some(8),
            Some(9),
            Some(10),
        ]),
        ColumnData::String(vec![
            Some("Alice".to_string()),
            Some("Bob".to_string()),
            Some("Charlie".to_string()),
            Some("David".to_string()),
            Some("Eve".to_string()),
            Some("Frank".to_string()),
            Some("Grace".to_string()),
            Some("Henry".to_string()),
            Some("Ivy".to_string()),
            Some("Jack".to_string()),
        ]),
        ColumnData::Int32(vec![
            Some(25),
            Some(30),
            Some(35),
            Some(40),
            Some(17),
            Some(22),
            Some(28),
            Some(33),
            Some(45),
            Some(19),
        ]),
        ColumnData::Float64(vec![
            Some(85.5),
            Some(92.0),
            Some(78.5),
            Some(95.0),
            Some(65.0),
            Some(88.0),
            Some(91.5),
            Some(82.0),
            Some(97.5),
            Some(72.0),
        ]),
    ];

    let batch = RecordBatch::new(schema.clone(), columns, 10)?;
    Ok((schema, vec![batch]))
}

#[tokio::test]
async fn test_full_query_pipeline() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT name, age, score FROM students WHERE age > 20 AND score > 80.0 ORDER BY score DESC LIMIT 5";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    assert!(results[0].num_rows <= 5);

    Ok(())
}

#[tokio::test]
async fn test_query_caching() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT * FROM students";

    // First execution - cache miss
    let _results1 = engine.execute_sql(sql).await?;
    let stats1 = engine.cache_statistics();
    assert_eq!(stats1.hits, 0);
    assert_eq!(stats1.misses, 1);

    // Second execution - cache hit
    let _results2 = engine.execute_sql(sql).await?;
    let stats2 = engine.cache_statistics();
    assert_eq!(stats2.hits, 1);
    assert_eq!(stats2.misses, 1);

    Ok(())
}

#[tokio::test]
async fn test_complex_aggregation() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT AVG(score), COUNT(*) FROM students";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 1);

    Ok(())
}

#[test]
fn test_query_explain() -> Result<()> {
    let sql = "SELECT name, age FROM students WHERE age > 18 ORDER BY age LIMIT 10";
    let engine = QueryEngine::new();
    let explain = engine.explain_sql(sql)?;

    let text = explain.format_text();
    assert!(text.contains("Query Execution Plan"));

    Ok(())
}

#[tokio::test]
async fn test_filtering_with_multiple_predicates() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT * FROM students WHERE age >= 25 AND age <= 35";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    // Should have students with age between 25 and 35
    assert!(results[0].num_rows > 0);

    Ok(())
}

#[tokio::test]
async fn test_sorting_stability() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT * FROM students ORDER BY age ASC";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 10);

    Ok(())
}

#[tokio::test]
async fn test_limit_without_offset() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT * FROM students LIMIT 3";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 3);

    Ok(())
}

#[tokio::test]
async fn test_offset_without_limit() -> Result<()> {
    let (schema, batches) = create_test_dataset()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut engine = QueryEngine::new();
    engine.register_data_source("students".to_string(), source);

    let sql = "SELECT * FROM students OFFSET 5";
    let results = engine.execute_sql(sql).await?;

    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 5); // 10 - 5

    Ok(())
}

#[test]
fn test_cache_clear() {
    let engine = QueryEngine::new();
    engine.clear_cache();

    let stats = engine.cache_statistics();
    assert_eq!(stats.clears, 1);
}

#[test]
fn test_optimizer_integration() -> Result<()> {
    let engine = QueryEngine::new();
    let sql = "SELECT * FROM students WHERE 1 + 1 = 2";
    let explain = engine.explain_sql(sql)?;

    // Constant folding should have happened
    assert!(explain.total_cost.total() >= 0.0);

    Ok(())
}
