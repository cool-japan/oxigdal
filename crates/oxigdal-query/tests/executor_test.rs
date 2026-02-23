//! Executor tests.

use oxigdal_query::Result;
use oxigdal_query::executor::Executor;
use oxigdal_query::executor::scan::{
    ColumnData, DataType, Field, MemoryDataSource, RecordBatch, Schema,
};
use oxigdal_query::parser::sql::parse_sql;
use std::sync::Arc;

fn create_test_data() -> Result<(Arc<Schema>, Vec<RecordBatch>)> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id".to_string(), DataType::Int64, false),
        Field::new("name".to_string(), DataType::String, false),
        Field::new("age".to_string(), DataType::Int32, false),
    ]));

    let columns = vec![
        ColumnData::Int64(vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
        ColumnData::String(vec![
            Some("Alice".to_string()),
            Some("Bob".to_string()),
            Some("Charlie".to_string()),
            Some("David".to_string()),
            Some("Eve".to_string()),
        ]),
        ColumnData::Int32(vec![Some(25), Some(30), Some(35), Some(40), Some(17)]),
    ];

    let batch = RecordBatch::new(schema.clone(), columns, 5)?;
    Ok((schema, vec![batch]))
}

#[tokio::test]
async fn test_simple_select() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT * FROM users";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 5);

    Ok(())
}

#[tokio::test]
async fn test_select_with_filter() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT * FROM users WHERE age > 20";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    // Should filter out the user with age 17
    assert!(results[0].num_rows < 5);

    Ok(())
}

#[tokio::test]
async fn test_select_with_limit() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT * FROM users LIMIT 2";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 2);

    Ok(())
}

#[tokio::test]
async fn test_select_with_offset() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT * FROM users LIMIT 2 OFFSET 2";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 2);

    Ok(())
}

#[tokio::test]
async fn test_select_with_order_by() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT * FROM users ORDER BY age DESC";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 5);

    Ok(())
}

#[tokio::test]
async fn test_select_with_aggregation() -> Result<()> {
    let (schema, batches) = create_test_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("users".to_string(), source);

    let sql = "SELECT COUNT(*) FROM users";
    let stmt = parse_sql(sql)?;

    let results = executor.execute(&stmt).await?;
    assert!(!results.is_empty());
    assert_eq!(results[0].num_rows, 1);

    Ok(())
}

#[tokio::test]
async fn test_table_not_found() {
    let executor = Executor::new();

    let sql = "SELECT * FROM nonexistent";
    let stmt = parse_sql(sql).ok();

    if let Some(stmt) = stmt {
        let result = executor.execute(&stmt).await;
        assert!(result.is_err());
    }
}
