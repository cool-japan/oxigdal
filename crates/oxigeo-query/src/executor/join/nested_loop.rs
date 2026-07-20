//! Nested loop join implementations

use super::Join;
use crate::error::Result;
use crate::executor::scan::{RecordBatch, Schema};
use std::sync::Arc;

impl Join {
    pub(super) fn inner_join(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
    ) -> Result<RecordBatch> {
        // Try hash join first
        if let Some(result) = self.try_hash_join(left, right) {
            return result;
        }

        // Fall back to nested loop join
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

        // Nested loop join
        let mut result_rows = 0;
        for left_row in 0..left.num_rows {
            for right_row in 0..right.num_rows {
                if self.evaluate_join_condition(left, right, left_row, right_row)? {
                    self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                    result_rows += 1;
                }
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }

    /// Left outer join.
    pub(super) fn left_join(&self, left: &RecordBatch, right: &RecordBatch) -> Result<RecordBatch> {
        let mut result_columns = Vec::new();
        let mut result_fields = Vec::new();

        for field in &left.schema.fields {
            result_fields.push(field.clone());
        }
        for field in &right.schema.fields {
            result_fields.push(field.clone());
        }

        for _ in 0..result_fields.len() {
            result_columns.push(Vec::new());
        }

        let mut result_rows = 0;
        for left_row in 0..left.num_rows {
            let mut matched = false;
            for right_row in 0..right.num_rows {
                if self.evaluate_join_condition(left, right, left_row, right_row)? {
                    self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                    result_rows += 1;
                    matched = true;
                }
            }
            if !matched {
                // Append left row with nulls for right side
                self.append_left_with_nulls(&mut result_columns, left, right, left_row)?;
                result_rows += 1;
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }

    /// Right outer join.
    pub(super) fn right_join(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
    ) -> Result<RecordBatch> {
        let mut result_columns = Vec::new();
        let mut result_fields = Vec::new();

        // For right join, we still want left columns first, then right columns
        for field in &left.schema.fields {
            result_fields.push(field.clone());
        }
        for field in &right.schema.fields {
            result_fields.push(field.clone());
        }

        for _ in 0..result_fields.len() {
            result_columns.push(Vec::new());
        }

        let mut result_rows = 0;
        for right_row in 0..right.num_rows {
            let mut matched = false;
            for left_row in 0..left.num_rows {
                if self.evaluate_join_condition(left, right, left_row, right_row)? {
                    self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                    result_rows += 1;
                    matched = true;
                }
            }
            if !matched {
                // Append nulls for left side with right row
                self.append_right_with_nulls(&mut result_columns, left, right, right_row)?;
                result_rows += 1;
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }

    /// Full outer join.
    pub(super) fn full_join(&self, left: &RecordBatch, right: &RecordBatch) -> Result<RecordBatch> {
        let mut result_columns = Vec::new();
        let mut result_fields = Vec::new();

        for field in &left.schema.fields {
            result_fields.push(field.clone());
        }
        for field in &right.schema.fields {
            result_fields.push(field.clone());
        }

        for _ in 0..result_fields.len() {
            result_columns.push(Vec::new());
        }

        // Track which right rows have been matched
        let mut right_matched = vec![false; right.num_rows];

        let mut result_rows = 0;

        // First pass: process all left rows (similar to left join)
        for left_row in 0..left.num_rows {
            let mut matched = false;
            for (right_row, right_match) in right_matched.iter_mut().enumerate() {
                if self.evaluate_join_condition(left, right, left_row, right_row)? {
                    self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                    result_rows += 1;
                    matched = true;
                    *right_match = true;
                }
            }
            if !matched {
                // Append left row with nulls for right side
                self.append_left_with_nulls(&mut result_columns, left, right, left_row)?;
                result_rows += 1;
            }
        }

        // Second pass: add unmatched right rows with nulls for left side
        for (right_row, &right_match) in right_matched.iter().enumerate() {
            if !right_match {
                self.append_right_with_nulls(&mut result_columns, left, right, right_row)?;
                result_rows += 1;
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }

    /// Cross join.
    pub(super) fn cross_join(
        &self,
        left: &RecordBatch,
        right: &RecordBatch,
    ) -> Result<RecordBatch> {
        let mut result_columns = Vec::new();
        let mut result_fields = Vec::new();

        for field in &left.schema.fields {
            result_fields.push(field.clone());
        }
        for field in &right.schema.fields {
            result_fields.push(field.clone());
        }

        for _ in 0..result_fields.len() {
            result_columns.push(Vec::new());
        }

        let mut result_rows = 0;
        for left_row in 0..left.num_rows {
            for right_row in 0..right.num_rows {
                self.append_row(&mut result_columns, left, right, left_row, right_row)?;
                result_rows += 1;
            }
        }

        let schema = Arc::new(Schema::new(result_fields));
        let columns = self.convert_columns(result_columns);
        RecordBatch::new(schema, columns, result_rows)
    }
}
