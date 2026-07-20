//! Integration tests for the SQL:2003 window-function evaluation kernel.
//!
//! These exercise the public window API (`evaluate_window` /
//! `evaluate_window_batch`) directly, without going through the SQL parser or
//! planner, matching the kernel's intended usage.

use std::sync::Arc;

use oxigeo_query::executor::filter::Value;
use oxigeo_query::executor::scan::{ColumnData, DataType, Field, RecordBatch, Schema};
use oxigeo_query::executor::window::{
    OrderKey, WindowFunction, WindowSpec, evaluate_window, evaluate_window_batch,
};

/// Build a `(num_rows, value_at)` closure over a column-major table of `Value`s.
fn table(columns: Vec<Vec<Value>>) -> (usize, impl Fn(usize, usize) -> Value) {
    let num_rows = columns.first().map(|c| c.len()).unwrap_or(0);
    (num_rows, move |row: usize, col: usize| {
        columns[col][row].clone()
    })
}

#[test]
fn test_row_number_sequential_within_partition() {
    // One partition (no PARTITION BY), ordered ascending by column 0.
    // Input column 0: 50, 10, 30, 20, 40 -> sorted ascending -> ranks 5,1,3,2,4.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(50),
        Value::Int64(10),
        Value::Int64(30),
        Value::Int64(20),
        Value::Int64(40),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 0, value_at)
        .expect("row_number should evaluate");

    assert_eq!(out[0], Value::Int64(5));
    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[2], Value::Int64(3));
    assert_eq!(out[3], Value::Int64(2));
    assert_eq!(out[4], Value::Int64(4));
}

#[test]
fn test_row_number_resets_per_partition() {
    // PARTITION BY column 0 (group), ORDER BY column 1 (value) ascending.
    // Rows (group, value):
    //   r0 (A, 30), r1 (B, 10), r2 (A, 10), r3 (B, 20), r4 (A, 20)
    // Partition A rows in input order: r0,r2,r4 ; sorted by value: r2(10),r4(20),r0(30)
    //   -> r2=1, r4=2, r0=3
    // Partition B rows: r1,r3 ; sorted: r1(10),r3(20) -> r1=1, r3=2
    let (rows, value_at) = table(vec![
        vec![
            Value::String("A".to_string()),
            Value::String("B".to_string()),
            Value::String("A".to_string()),
            Value::String("B".to_string()),
            Value::String("A".to_string()),
        ],
        vec![
            Value::Int64(30),
            Value::Int64(10),
            Value::Int64(10),
            Value::Int64(20),
            Value::Int64(20),
        ],
    ]);
    let spec = WindowSpec::new(vec![0], vec![OrderKey::asc(1)]);
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 1, value_at)
        .expect("row_number should evaluate");

    assert_eq!(out[2], Value::Int64(1));
    assert_eq!(out[4], Value::Int64(2));
    assert_eq!(out[0], Value::Int64(3));
    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[3], Value::Int64(2));
}

#[test]
fn test_rank_ties_share_rank_with_gap() {
    // ORDER BY column 0 ascending: values 10,10,20,30 -> RANK: 1,1,3,4.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out =
        evaluate_window(&WindowFunction::Rank, &spec, rows, 0, value_at).expect("rank should eval");

    // Sorted order equals input order here (already ascending, stable).
    assert_eq!(out[0], Value::Int64(1));
    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[2], Value::Int64(3));
    assert_eq!(out[3], Value::Int64(4));
}

#[test]
fn test_dense_rank_ties_no_gap() {
    // Same data as rank test but DENSE_RANK: 1,1,2,3 (no gap after the tie).
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out = evaluate_window(&WindowFunction::DenseRank, &spec, rows, 0, value_at)
        .expect("dense_rank should eval");

    assert_eq!(out[0], Value::Int64(1));
    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[2], Value::Int64(2));
    assert_eq!(out[3], Value::Int64(3));
}

#[test]
fn test_lag_returns_previous_row_value() {
    // ORDER BY column 0 ascending: 10,20,30. LAG(value,1) over the same column.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out =
        evaluate_window(&WindowFunction::lag(), &spec, rows, 0, value_at).expect("lag should eval");

    // Sorted = input order. row0 has no predecessor -> NULL; row1 -> 10; row2 -> 20.
    assert_eq!(out[0], Value::Null);
    assert_eq!(out[1], Value::Int64(10));
    assert_eq!(out[2], Value::Int64(20));
}

#[test]
fn test_lag_first_row_returns_default() {
    // LAG(value, 1, -1): first row falls before the partition -> default -1.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let func = WindowFunction::lag_offset_default(1, Value::Int64(-1));
    let out = evaluate_window(&func, &spec, rows, 0, value_at).expect("lag should eval");

    assert_eq!(out[0], Value::Int64(-1));
    assert_eq!(out[1], Value::Int64(10));
    assert_eq!(out[2], Value::Int64(20));
}

#[test]
fn test_lead_returns_next_row_value() {
    // LEAD(value, 1): each row sees the next sorted row's value.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out = evaluate_window(&WindowFunction::lead(), &spec, rows, 0, value_at)
        .expect("lead should eval");

    assert_eq!(out[0], Value::Int64(20));
    assert_eq!(out[1], Value::Int64(30));
    assert_eq!(out[2], Value::Null);
}

#[test]
fn test_lead_last_row_returns_null_default() {
    // LEAD(value, 1) with no default: last row -> NULL.
    let (rows, value_at) = table(vec![vec![Value::Int64(7), Value::Int64(9)]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out = evaluate_window(&WindowFunction::lead(), &spec, rows, 0, value_at)
        .expect("lead should eval");

    assert_eq!(out[0], Value::Int64(9));
    assert_eq!(out[1], Value::Null);
}

#[test]
fn test_lag_offset_2() {
    // LAG(value, 2): look two rows back in the sorted partition.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(20),
        Value::Int64(30),
        Value::Int64(40),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let func = WindowFunction::lag_offset(2);
    let out = evaluate_window(&func, &spec, rows, 0, value_at).expect("lag should eval");

    // row0,row1 -> before start -> NULL ; row2 -> 10 ; row3 -> 20.
    assert_eq!(out[0], Value::Null);
    assert_eq!(out[1], Value::Null);
    assert_eq!(out[2], Value::Int64(10));
    assert_eq!(out[3], Value::Int64(20));
}

#[test]
fn test_partition_by_two_keys() {
    // PARTITION BY (col0, col1), ORDER BY col2 ascending; ROW_NUMBER.
    // Rows (k1, k2, v):
    //   r0 (A,1,5) r1 (A,1,3) r2 (A,2,9) r3 (B,1,8) r4 (A,1,4)
    // Partition (A,1): r0,r1,r4 sorted by v: r1(3),r4(4),r0(5) -> r1=1,r4=2,r0=3
    // Partition (A,2): r2 -> r2=1
    // Partition (B,1): r3 -> r3=1
    let (rows, value_at) = table(vec![
        vec![
            Value::String("A".to_string()),
            Value::String("A".to_string()),
            Value::String("A".to_string()),
            Value::String("B".to_string()),
            Value::String("A".to_string()),
        ],
        vec![
            Value::Int64(1),
            Value::Int64(1),
            Value::Int64(2),
            Value::Int64(1),
            Value::Int64(1),
        ],
        vec![
            Value::Int64(5),
            Value::Int64(3),
            Value::Int64(9),
            Value::Int64(8),
            Value::Int64(4),
        ],
    ]);
    let spec = WindowSpec::new(vec![0, 1], vec![OrderKey::asc(2)]);
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 2, value_at)
        .expect("row_number should eval");

    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[4], Value::Int64(2));
    assert_eq!(out[0], Value::Int64(3));
    assert_eq!(out[2], Value::Int64(1));
    assert_eq!(out[3], Value::Int64(1));
}

#[test]
fn test_order_by_desc() {
    // ORDER BY column 0 DESC: values 10,30,20 -> sorted 30(r1),20(r2),10(r0).
    // ROW_NUMBER -> r1=1, r2=2, r0=3.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(10),
        Value::Int64(30),
        Value::Int64(20),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::desc(0)]);
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 0, value_at)
        .expect("row_number should eval");

    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[2], Value::Int64(2));
    assert_eq!(out[0], Value::Int64(3));
}

#[test]
fn test_empty_input_returns_empty() {
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let value_at = |_row: usize, _col: usize| Value::Null;
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, 0, 0, value_at)
        .expect("empty input should eval");
    assert!(out.is_empty());

    let out_rank =
        evaluate_window(&WindowFunction::Rank, &spec, 0, 0, value_at).expect("empty rank");
    assert!(out_rank.is_empty());
}

#[test]
fn test_first_last_nth_value() {
    // ORDER BY column 0 ascending: 30,10,20 -> sorted 10(r1),20(r2),30(r0).
    let (rows, value_at) = table(vec![vec![
        Value::Int64(30),
        Value::Int64(10),
        Value::Int64(20),
    ]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);

    let first = evaluate_window(&WindowFunction::FirstValue, &spec, rows, 0, &value_at)
        .expect("first_value should eval");
    // FIRST_VALUE broadcasts the smallest (first sorted) value to every row.
    assert_eq!(first[0], Value::Int64(10));
    assert_eq!(first[1], Value::Int64(10));
    assert_eq!(first[2], Value::Int64(10));

    let last = evaluate_window(&WindowFunction::LastValue, &spec, rows, 0, &value_at)
        .expect("last_value should eval");
    assert_eq!(last[0], Value::Int64(30));
    assert_eq!(last[1], Value::Int64(30));
    assert_eq!(last[2], Value::Int64(30));

    let nth = evaluate_window(&WindowFunction::nth_value(2), &spec, rows, 0, &value_at)
        .expect("nth_value should eval");
    // 2nd sorted value is 20.
    assert_eq!(nth[0], Value::Int64(20));
    assert_eq!(nth[1], Value::Int64(20));
    assert_eq!(nth[2], Value::Int64(20));

    // Out-of-range NTH_VALUE -> NULL everywhere.
    let nth_oob = evaluate_window(&WindowFunction::nth_value(99), &spec, rows, 0, &value_at)
        .expect("nth_value oob should eval");
    assert_eq!(nth_oob[0], Value::Null);
}

#[test]
fn test_rank_no_order_by_all_tie() {
    // With no ORDER BY, every row is in one tie group -> RANK 1 for all,
    // DENSE_RANK 1 for all, ROW_NUMBER still 1..n in input order.
    let (rows, value_at) = table(vec![vec![
        Value::Int64(7),
        Value::Int64(7),
        Value::Int64(7),
    ]]);
    let spec = WindowSpec::default();

    let rank =
        evaluate_window(&WindowFunction::Rank, &spec, rows, 0, &value_at).expect("rank eval");
    assert_eq!(
        rank,
        vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)]
    );

    let dense =
        evaluate_window(&WindowFunction::DenseRank, &spec, rows, 0, &value_at).expect("dense eval");
    assert_eq!(
        dense,
        vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)]
    );

    let rn = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 0, &value_at)
        .expect("row_number eval");
    assert_eq!(rn, vec![Value::Int64(1), Value::Int64(2), Value::Int64(3)]);
}

#[test]
fn test_evaluate_window_batch_record_batch() {
    // Drive the RecordBatch convenience wrapper end-to-end.
    let schema = Arc::new(Schema::new(vec![
        Field::new("grp".to_string(), DataType::String, true),
        Field::new("val".to_string(), DataType::Int64, true),
    ]));
    let columns = vec![
        ColumnData::String(vec![
            Some("A".to_string()),
            Some("A".to_string()),
            Some("B".to_string()),
        ]),
        ColumnData::Int64(vec![Some(20), Some(10), Some(5)]),
    ];
    let batch = RecordBatch::new(schema, columns, 3).expect("batch should build");

    // PARTITION BY grp(0) ORDER BY val(1) ASC, ROW_NUMBER.
    let spec = WindowSpec::new(vec![0], vec![OrderKey::asc(1)]);
    let out = evaluate_window_batch(&WindowFunction::RowNumber, &spec, 1, &batch)
        .expect("batch window should eval");

    // Partition A: row1(10)=1, row0(20)=2 ; Partition B: row2(5)=1.
    assert_eq!(out[1], Value::Int64(1));
    assert_eq!(out[0], Value::Int64(2));
    assert_eq!(out[2], Value::Int64(1));
}

#[test]
fn test_evaluate_window_batch_out_of_bounds_column() {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "val".to_string(),
        DataType::Int64,
        true,
    )]));
    let columns = vec![ColumnData::Int64(vec![Some(1), Some(2)])];
    let batch = RecordBatch::new(schema, columns, 2).expect("batch should build");

    // Order-by references column 5 which does not exist -> error, no panic.
    let spec = WindowSpec::ordered(vec![OrderKey::asc(5)]);
    let result = evaluate_window_batch(&WindowFunction::RowNumber, &spec, 0, &batch);
    assert!(result.is_err());
}

#[test]
fn test_null_ordering_places_nulls_last() {
    // ORDER BY column 0 ASC with a NULL: 20, NULL, 10 -> sorted 10,20,NULL.
    let (rows, value_at) = table(vec![vec![Value::Int64(20), Value::Null, Value::Int64(10)]]);
    let spec = WindowSpec::ordered(vec![OrderKey::asc(0)]);
    let out = evaluate_window(&WindowFunction::RowNumber, &spec, rows, 0, value_at)
        .expect("row_number should eval");

    // 10(row2)=1, 20(row0)=2, NULL(row1)=3.
    assert_eq!(out[2], Value::Int64(1));
    assert_eq!(out[0], Value::Int64(2));
    assert_eq!(out[1], Value::Int64(3));
}
