//! Hash join optimization

use super::Join;
use crate::error::Result;
use crate::executor::scan::{ColumnData, RecordBatch, Schema};
use crate::parser::ast::{BinaryOperator, Expr};
use std::collections::HashMap;
use std::sync::Arc;

impl Join {
    pub(super) fn try_hash_join(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
    ) -> Option<Result<RecordBatch>> {
        let condition = self.on_condition.as_ref()?;

        // Check if condition is a simple equality: col1 = col2
        if let Expr::BinaryOp {
            left: left_expr,
            op: BinaryOperator::Eq,
            right: right_expr,
        } = condition
        {
            // Check if both sides are column references
            if let (
                Expr::Column {
                    table: left_table,
                    name: left_col,
                },
                Expr::Column {
                    table: right_table,
                    name: right_col,
                },
            ) = (left_expr.as_ref(), right_expr.as_ref())
            {
                // Determine which column is from which table
                let (left_idx, right_idx) = self.resolve_join_columns(
                    left,
                    right,
                    left_table.as_deref(),
                    left_col,
                    right_table.as_deref(),
                    right_col,
                )?;

                // Check if column types are compatible for hash join
                // Hash join requires exact type match (or we need type normalization)
                if !self
                    .are_types_hash_compatible(&left.columns[left_idx], &right.columns[right_idx])
                {
                    // Fall back to nested loop join for mixed types
                    return None;
                }

                return Some(self.hash_join_impl(left, right, left_idx, right_idx));
            }
        }

        None
    }

    /// Check if two column types are compatible for hash join.
    /// Hash join requires the same hash key format, so we need compatible types.
    pub(super) fn are_types_hash_compatible(&self, left: &ColumnData, right: &ColumnData) -> bool {
        matches!(
            (left, right),
            (ColumnData::Boolean(_), ColumnData::Boolean(_))
                | (ColumnData::Int32(_), ColumnData::Int32(_))
                | (ColumnData::Int32(_), ColumnData::Int64(_))
                | (ColumnData::Int64(_), ColumnData::Int32(_))
                | (ColumnData::Int64(_), ColumnData::Int64(_))
                | (ColumnData::Float32(_), ColumnData::Float32(_))
                | (ColumnData::Float32(_), ColumnData::Float64(_))
                | (ColumnData::Float64(_), ColumnData::Float32(_))
                | (ColumnData::Float64(_), ColumnData::Float64(_))
                | (ColumnData::String(_), ColumnData::String(_))
        )
    }

    /// Resolve join columns to their indices.
    pub(super) fn resolve_join_columns(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
        table1: Option<&str>,
        col1: &str,
        table2: Option<&str>,
        col2: &str,
    ) -> Option<(usize, usize)> {
        // Try to match columns by table alias or by name
        let left_alias = self.left_alias.as_deref();
        let right_alias = self.right_alias.as_deref();

        // First attempt: table1.col1 is from left, table2.col2 is from right
        let left_idx_1 = self.find_column_index(left, table1, col1, left_alias);
        let right_idx_2 = self.find_column_index(right, table2, col2, right_alias);

        if let (Some(l), Some(r)) = (left_idx_1, right_idx_2) {
            return Some((l, r));
        }

        // Second attempt: table1.col1 is from right, table2.col2 is from left
        let right_idx_1 = self.find_column_index(right, table1, col1, right_alias);
        let left_idx_2 = self.find_column_index(left, table2, col2, left_alias);

        if let (Some(l), Some(r)) = (left_idx_2, right_idx_1) {
            return Some((l, r));
        }

        // Third attempt: no table qualifier, try by column name only
        let left_by_name_1 = left.schema.index_of(col1);
        let right_by_name_2 = right.schema.index_of(col2);

        if let (Some(l), Some(r)) = (left_by_name_1, right_by_name_2) {
            return Some((l, r));
        }

        None
    }

    /// Find column index in a batch.
    pub(super) fn find_column_index(
        &self,
        batch: &RecordBatch,
        table: Option<&str>,
        col_name: &str,
        alias: Option<&str>,
    ) -> Option<usize> {
        // If table qualifier matches alias, look for column by name
        match (table, alias) {
            (Some(t), Some(a)) if t == a => batch.schema.index_of(col_name),
            (Some(_), Some(_)) => None, // Table doesn't match alias
            (None, _) => batch.schema.index_of(col_name),
            (Some(t), None) => {
                // Check if table name matches any field name pattern
                batch
                    .schema
                    .index_of(col_name)
                    .filter(|_| {
                        // Simple heuristic: if no alias, accept the column if it exists
                        true
                    })
                    .or_else(|| {
                        // Try qualified name like "table.column"
                        batch.schema.index_of(&format!("{}.{}", t, col_name))
                    })
            }
        }
    }

    /// Hash join implementation.
    pub(super) fn hash_join_impl(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
        left_col_idx: usize,
        right_col_idx: usize,
    ) -> Result<RecordBatch> {
        // Build hash table from left side
        // Skip NULL values - NULL should not match NULL in SQL semantics
        let mut hash_table: HashMap<String, Vec<usize>> = HashMap::new();

        for row in 0..left.num_rows {
            let value = self.get_column_value(&left.columns[left_col_idx], row)?;
            // Skip NULL values - they should not be added to hash table
            if value.is_null() {
                continue;
            }
            let key = value.to_hash_key();
            hash_table.entry(key).or_default().push(row);
        }

        let mut result_columns = Vec::new();
        let mut result_fields = Vec::new();

        // Collect schema
        for field in &left.schema.fields {
            result_fields.push(field.clone());
        }
        for field in &right.schema.fields {
            result_fields.push(field.clone());
        }

        // Initialize result columns
        for _ in 0..result_fields.len() {
            result_columns.push(Vec::new());
        }

        // Probe phase
        let mut result_rows = 0;
        for right_row in 0..right.num_rows {
            let value = self.get_column_value(&right.columns[right_col_idx], right_row)?;
            // Skip NULL values - they should not match anything
            if value.is_null() {
                continue;
            }
            let key = value.to_hash_key();

            if let Some(left_rows) = hash_table.get(&key) {
                for &left_row in left_rows {
                    self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                    result_rows += 1;
                }
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }
}
