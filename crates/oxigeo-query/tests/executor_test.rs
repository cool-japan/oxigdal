//! Executor tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_query::Result;
use oxigeo_query::executor::Executor;
use oxigeo_query::executor::scan::{
    ColumnData, DataType, Field, MemoryDataSource, RecordBatch, Schema,
};
use oxigeo_query::parser::sql::parse_sql;
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

fn create_grouped_data() -> Result<(Arc<Schema>, Vec<RecordBatch>)> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("dept".to_string(), DataType::String, false),
        Field::new("region".to_string(), DataType::String, false),
        Field::new("salary".to_string(), DataType::Int64, false),
    ]));

    let columns = vec![
        ColumnData::String(vec![
            Some("eng".to_string()),
            Some("eng".to_string()),
            Some("sales".to_string()),
            Some("eng".to_string()),
            Some("sales".to_string()),
        ]),
        ColumnData::String(vec![
            Some("east".to_string()),
            Some("west".to_string()),
            Some("east".to_string()),
            Some("east".to_string()),
            Some("east".to_string()),
        ]),
        ColumnData::Int64(vec![Some(100), Some(200), Some(300), Some(300), Some(100)]),
    ];

    let batch = RecordBatch::new(schema.clone(), columns, 5)?;
    Ok((schema, vec![batch]))
}

#[tokio::test]
async fn test_group_by_count_and_avg() -> Result<()> {
    let (schema, batches) = create_grouped_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("emp".to_string(), source);

    let sql = "SELECT dept, COUNT(*), AVG(salary) FROM emp GROUP BY dept";
    let stmt = parse_sql(sql)?;
    let results = executor.execute(&stmt).await?;

    assert_eq!(results.len(), 1);
    let batch = &results[0];
    // Two groups: eng, sales.
    assert_eq!(batch.num_rows, 2);
    assert_eq!(batch.columns.len(), 3);

    // First column is the group key (dept), preserving first-appearance order.
    let ColumnData::String(dept) = &batch.columns[0] else {
        panic!("expected string group column");
    };
    assert_eq!(dept[0], Some("eng".to_string()));
    assert_eq!(dept[1], Some("sales".to_string()));

    // COUNT(*): eng=3, sales=2.
    let ColumnData::Float64(count) = &batch.columns[1] else {
        panic!("expected float count column");
    };
    assert_eq!(count[0], Some(3.0));
    assert_eq!(count[1], Some(2.0));

    // AVG(salary): eng=(100+200+300)/3=200, sales=(300+100)/2=200.
    let ColumnData::Float64(avg) = &batch.columns[2] else {
        panic!("expected float avg column");
    };
    assert_eq!(avg[0], Some(200.0));
    assert_eq!(avg[1], Some(200.0));

    Ok(())
}

#[tokio::test]
async fn test_group_by_multi_column() -> Result<()> {
    let (schema, batches) = create_grouped_data()?;
    let source = Arc::new(MemoryDataSource::new(schema, batches));

    let mut executor = Executor::new();
    executor.register_data_source("emp".to_string(), source);

    // Composite key (dept, region): (eng,east), (eng,west), (sales,east) => 3 groups.
    let sql = "SELECT dept, region, SUM(salary) FROM emp GROUP BY dept, region";
    let stmt = parse_sql(sql)?;
    let results = executor.execute(&stmt).await?;

    assert_eq!(results.len(), 1);
    let batch = &results[0];
    assert_eq!(batch.num_rows, 3);
    assert_eq!(batch.columns.len(), 3);

    Ok(())
}
