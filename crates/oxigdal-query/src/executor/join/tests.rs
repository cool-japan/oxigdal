use super::{Join, JoinValue};
use crate::error::Result;
use crate::executor::scan::{ColumnData, DataType, Field, RecordBatch, Schema};
use crate::parser::ast::{BinaryOperator, Expr, JoinType, Literal};
use std::sync::Arc;

/// Create a simple test schema with one column.
fn create_schema(name: &str, data_type: DataType) -> Arc<Schema> {
    Arc::new(Schema::new(vec![Field::new(
        name.to_string(),
        data_type,
        true,
    )]))
}

/// Create a simple test schema with multiple columns.
fn create_multi_column_schema(fields: Vec<(&str, DataType)>) -> Arc<Schema> {
    Arc::new(Schema::new(
        fields
            .into_iter()
            .map(|(name, dt)| Field::new(name.to_string(), dt, true))
            .collect(),
    ))
}

#[test]
fn test_cross_join() -> Result<()> {
    let left_schema = create_schema("a", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2)])];
    let left = RecordBatch::new(left_schema, left_columns, 2)?;

    let right_schema = create_schema("b", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(3), Some(4)])];
    let right = RecordBatch::new(right_schema, right_columns, 2)?;

    let join = Join::new(JoinType::Cross, None);
    let result = join.execute(&left, &right)?;

    assert_eq!(result.num_rows, 4); // 2 * 2
    assert_eq!(result.columns.len(), 2); // a + b

    Ok(())
}

#[test]
fn test_inner_join_with_equality() -> Result<()> {
    // Left: id = [1, 2, 3]
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    // Right: id = [2, 3, 4]
    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(2), Some(3), Some(4)])];
    let right = RecordBatch::new(right_schema, right_columns, 3)?;

    // Join on id = id (both have same column name, will match by position)
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("left".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("right".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("left")
        .with_right_alias("right");

    let result = join.execute(&left, &right)?;

    // Should match: (2, 2) and (3, 3)
    assert_eq!(result.num_rows, 2);
    assert_eq!(result.columns.len(), 2);

    Ok(())
}

#[test]
fn test_left_join() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(2)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Left, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // All 3 left rows should appear:
    // (1, null), (2, 2), (3, null)
    assert_eq!(result.num_rows, 3);

    Ok(())
}

#[test]
fn test_right_join() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(2)])];
    let left = RecordBatch::new(left_schema, left_columns, 1)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let right = RecordBatch::new(right_schema, right_columns, 3)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Right, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // All 3 right rows should appear:
    // (null, 1), (2, 2), (null, 3)
    assert_eq!(result.num_rows, 3);

    Ok(())
}

#[test]
fn test_full_outer_join() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2)])];
    let left = RecordBatch::new(left_schema, left_columns, 2)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(2), Some(3)])];
    let right = RecordBatch::new(right_schema, right_columns, 2)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Full, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Full outer join: (1, null), (2, 2), (null, 3)
    assert_eq!(result.num_rows, 3);

    Ok(())
}

#[test]
fn test_compound_condition_and() -> Result<()> {
    let left_schema =
        create_multi_column_schema(vec![("id", DataType::Int64), ("value", DataType::Int64)]);
    let left_columns = vec![
        ColumnData::Int64(vec![Some(1), Some(2), Some(3)]),
        ColumnData::Int64(vec![Some(10), Some(20), Some(30)]),
    ];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema =
        create_multi_column_schema(vec![("id", DataType::Int64), ("value", DataType::Int64)]);
    let right_columns = vec![
        ColumnData::Int64(vec![Some(2), Some(2), Some(3)]),
        ColumnData::Int64(vec![Some(15), Some(20), Some(35)]),
    ];
    let right = RecordBatch::new(right_schema, right_columns, 3)?;

    // Join on a.id = b.id AND a.value = b.value
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: Some("a".to_string()),
                name: "id".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Column {
                table: Some("b".to_string()),
                name: "id".to_string(),
            }),
        }),
        op: BinaryOperator::And,
        right: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: Some("a".to_string()),
                name: "value".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Column {
                table: Some("b".to_string()),
                name: "value".to_string(),
            }),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Only (2, 20) = (2, 20) matches
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_compound_condition_or() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(1), Some(3)])];
    let right = RecordBatch::new(right_schema, right_columns, 2)?;

    // Join on a.id = 1 OR a.id = 3
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: Some("a".to_string()),
                name: "id".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Column {
                table: Some("b".to_string()),
                name: "id".to_string(),
            }),
        }),
        op: BinaryOperator::Or,
        right: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Column {
                table: Some("a".to_string()),
                name: "id".to_string(),
            }),
            op: BinaryOperator::Eq,
            right: Box::new(Expr::Literal(Literal::Integer(1))),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // (1, 1), (1, 3) - because 1=1 is true; (3, 3) because 3=3
    // Also (2, 1) because 2=1 is false but we have OR 1=1 which would be evaluated
    // Wait, let me reconsider: a.id = b.id OR a.id = 1
    // For (1, 1): 1=1 OR 1=1 => true
    // For (1, 3): 1=3 OR 1=1 => true
    // For (2, 1): 2=1 OR 2=1 => false (should be 2=1 which is false)
    // For (2, 3): 2=3 OR 2=1 => false
    // For (3, 1): 3=1 OR 3=1 => false
    // For (3, 3): 3=3 OR 3=1 => true
    // So matches: (1,1), (1,3), (3,3) => 3 rows
    assert_eq!(result.num_rows, 3);

    Ok(())
}

#[test]
fn test_comparison_operators() -> Result<()> {
    let left_schema = create_schema("x", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(5), Some(10)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("y", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(5)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.x < b.y
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "x".to_string(),
        }),
        op: BinaryOperator::Lt,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "y".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 1 < 5 => true, 5 < 5 => false, 10 < 5 => false
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_null_handling() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), None, Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(1), None])];
    let right = RecordBatch::new(right_schema, right_columns, 2)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Only (1, 1) should match. NULL = NULL is not true.
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_string_join() -> Result<()> {
    let left_schema = create_schema("name", DataType::String);
    let left_columns = vec![ColumnData::String(vec![
        Some("Alice".to_string()),
        Some("Bob".to_string()),
    ])];
    let left = RecordBatch::new(left_schema, left_columns, 2)?;

    let right_schema = create_schema("name", DataType::String);
    let right_columns = vec![ColumnData::String(vec![
        Some("Bob".to_string()),
        Some("Charlie".to_string()),
    ])];
    let right = RecordBatch::new(right_schema, right_columns, 2)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "name".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "name".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Only "Bob" matches
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_float_comparison() -> Result<()> {
    let left_schema = create_schema("value", DataType::Float64);
    let left_columns = vec![ColumnData::Float64(vec![Some(1.5), Some(2.5), Some(3.5)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("value", DataType::Float64);
    let right_columns = vec![ColumnData::Float64(vec![Some(2.5)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.value >= b.value
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "value".to_string(),
        }),
        op: BinaryOperator::GtEq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "value".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 1.5 >= 2.5 => false, 2.5 >= 2.5 => true, 3.5 >= 2.5 => true
    assert_eq!(result.num_rows, 2);

    Ok(())
}

#[test]
fn test_mixed_type_comparison() -> Result<()> {
    let left_schema = create_schema("int_val", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("float_val", DataType::Float64);
    let right_columns = vec![ColumnData::Float64(vec![Some(2.0)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "int_val".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "float_val".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 2 = 2.0 should match
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_literal_in_condition() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("x", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(100)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.id > 1
    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Gt,
        right: Box::new(Expr::Literal(Literal::Integer(1))),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 1 > 1 => false, 2 > 1 => true, 3 > 1 => true
    // Cross with 1 right row: 2 matches
    assert_eq!(result.num_rows, 2);

    Ok(())
}

#[test]
fn test_is_null_condition() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), None, Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("x", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(100)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.id IS NULL
    let condition = Expr::IsNull(Box::new(Expr::Column {
        table: Some("a".to_string()),
        name: "id".to_string(),
    }));

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Only the row with NULL id should match
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_between_condition() -> Result<()> {
    let left_schema = create_schema("value", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(5), Some(10)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("x", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(100)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.value BETWEEN 3 AND 7
    let condition = Expr::Between {
        expr: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "value".to_string(),
        }),
        low: Box::new(Expr::Literal(Literal::Integer(3))),
        high: Box::new(Expr::Literal(Literal::Integer(7))),
        negated: false,
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 1 not between [3,7], 5 between [3,7], 10 not between [3,7]
    assert_eq!(result.num_rows, 1);

    Ok(())
}

#[test]
fn test_in_list_condition() -> Result<()> {
    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(vec![Some(1), Some(2), Some(3)])];
    let left = RecordBatch::new(left_schema, left_columns, 3)?;

    let right_schema = create_schema("x", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(vec![Some(100)])];
    let right = RecordBatch::new(right_schema, right_columns, 1)?;

    // Join on a.id IN (1, 3)
    let condition = Expr::InList {
        expr: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        list: vec![
            Expr::Literal(Literal::Integer(1)),
            Expr::Literal(Literal::Integer(3)),
        ],
        negated: false,
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // 1 in [1,3], 2 not in [1,3], 3 in [1,3]
    assert_eq!(result.num_rows, 2);

    Ok(())
}

#[test]
fn test_like_match() {
    // Test LIKE pattern matching
    assert!(JoinValue::like_match("hello", "hello"));
    assert!(JoinValue::like_match("hello", "h%"));
    assert!(JoinValue::like_match("hello", "%o"));
    assert!(JoinValue::like_match("hello", "%ll%"));
    assert!(JoinValue::like_match("hello", "h_llo"));
    assert!(JoinValue::like_match("hello", "_____"));
    assert!(!JoinValue::like_match("hello", "____"));
    assert!(!JoinValue::like_match("hello", "world"));
    assert!(JoinValue::like_match("hello", "%"));
    assert!(JoinValue::like_match("", "%"));
    assert!(JoinValue::like_match("", ""));
}

#[test]
fn test_join_value_arithmetic() {
    let a = JoinValue::Integer(10);
    let b = JoinValue::Integer(3);

    assert_eq!(a.add(&b), Some(JoinValue::Integer(13)));
    assert_eq!(a.subtract(&b), Some(JoinValue::Integer(7)));
    assert_eq!(a.multiply(&b), Some(JoinValue::Integer(30)));
    assert_eq!(a.divide(&b), Some(JoinValue::Integer(3)));
    assert_eq!(a.modulo(&b), Some(JoinValue::Integer(1)));

    let c = JoinValue::Float(10.0);
    let d = JoinValue::Float(3.0);

    assert_eq!(c.add(&d), Some(JoinValue::Float(13.0)));
    assert_eq!(c.subtract(&d), Some(JoinValue::Float(7.0)));
    assert_eq!(c.multiply(&d), Some(JoinValue::Float(30.0)));

    // Division by zero returns None
    assert_eq!(a.divide(&JoinValue::Integer(0)), None);
}

#[test]
fn test_hash_join_optimization() -> Result<()> {
    // Create larger datasets to test hash join optimization
    let mut left_data = Vec::new();
    let mut right_data = Vec::new();

    for i in 0..100 {
        left_data.push(Some(i as i64));
    }

    for i in 50..150 {
        right_data.push(Some(i as i64));
    }

    let left_schema = create_schema("id", DataType::Int64);
    let left_columns = vec![ColumnData::Int64(left_data)];
    let left = RecordBatch::new(left_schema, left_columns, 100)?;

    let right_schema = create_schema("id", DataType::Int64);
    let right_columns = vec![ColumnData::Int64(right_data)];
    let right = RecordBatch::new(right_schema, right_columns, 100)?;

    let condition = Expr::BinaryOp {
        left: Box::new(Expr::Column {
            table: Some("a".to_string()),
            name: "id".to_string(),
        }),
        op: BinaryOperator::Eq,
        right: Box::new(Expr::Column {
            table: Some("b".to_string()),
            name: "id".to_string(),
        }),
    };

    let join = Join::new(JoinType::Inner, Some(condition))
        .with_left_alias("a")
        .with_right_alias("b");

    let result = join.execute(&left, &right)?;

    // Overlap: 50-99 (50 values)
    assert_eq!(result.num_rows, 50);

    Ok(())
}
