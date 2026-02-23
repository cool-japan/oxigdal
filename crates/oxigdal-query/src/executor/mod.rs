//! Query execution engine.

pub mod aggregate;
pub mod filter;
pub mod join;
pub mod scan;
pub mod sort;

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use aggregate::{Aggregate, AggregateFunc, AggregateFunction};
use filter::Filter;
use join::Join;
use scan::{DataSource, RecordBatch, TableScan};
use sort::Sort;
use std::collections::HashMap;
use std::sync::Arc;

/// Query executor.
pub struct Executor {
    /// Data sources registry.
    data_sources: HashMap<String, Arc<dyn DataSource>>,
}

impl Executor {
    /// Create a new executor.
    pub fn new() -> Self {
        Self {
            data_sources: HashMap::new(),
        }
    }

    /// Register a data source.
    pub fn register_data_source(&mut self, name: String, source: Arc<dyn DataSource>) {
        self.data_sources.insert(name, source);
    }

    /// Execute a query.
    pub async fn execute(&self, stmt: &Statement) -> Result<Vec<RecordBatch>> {
        match stmt {
            Statement::Select(select) => self.execute_select(select).await,
        }
    }

    /// Execute a SELECT statement.
    async fn execute_select(&self, select: &SelectStatement) -> Result<Vec<RecordBatch>> {
        // Execute FROM clause
        let mut batches = if let Some(ref table_ref) = select.from {
            self.execute_table_reference(table_ref).await?
        } else {
            return Err(QueryError::semantic("SELECT without FROM not supported"));
        };

        // Execute WHERE clause
        if let Some(ref selection) = select.selection {
            batches = self.execute_filter(batches, selection)?;
        }

        // Execute GROUP BY / aggregation
        if !select.group_by.is_empty() || self.has_aggregates(&select.projection) {
            batches = self.execute_aggregate(batches, select)?;
        }

        // Execute ORDER BY
        if !select.order_by.is_empty() {
            batches = self.execute_sort(batches, &select.order_by)?;
        }

        // Execute LIMIT and OFFSET
        if select.limit.is_some() || select.offset.is_some() {
            batches = self.execute_limit_offset(batches, select.limit, select.offset)?;
        }

        Ok(batches)
    }

    /// Execute a table reference.
    async fn execute_table_reference(
        &self,
        table_ref: &TableReference,
    ) -> Result<Vec<RecordBatch>> {
        match table_ref {
            TableReference::Table { name, .. } => {
                let source = self
                    .data_sources
                    .get(name)
                    .ok_or_else(|| QueryError::TableNotFound(name.clone()))?;

                let scan = TableScan::new(name.clone(), source.clone());
                scan.execute().await
            }
            TableReference::Join {
                left,
                right,
                join_type,
                on,
            } => {
                // Use Box::pin to avoid infinite size for recursive async fn
                let left_batches = Box::pin(self.execute_table_reference(left)).await?;
                let right_batches = Box::pin(self.execute_table_reference(right)).await?;

                let join = Join::new(*join_type, on.clone());
                let mut result = Vec::new();

                for left_batch in &left_batches {
                    for right_batch in &right_batches {
                        result.push(join.execute(left_batch, right_batch)?);
                    }
                }

                Ok(result)
            }
            TableReference::Subquery { query, .. } => Box::pin(self.execute_select(query)).await,
        }
    }

    /// Execute filter operation.
    fn execute_filter(
        &self,
        batches: Vec<RecordBatch>,
        predicate: &Expr,
    ) -> Result<Vec<RecordBatch>> {
        let filter = Filter::new(predicate.clone());
        let mut result = Vec::new();

        for batch in batches {
            result.push(filter.execute(&batch)?);
        }

        Ok(result)
    }

    /// Execute aggregation.
    fn execute_aggregate(
        &self,
        batches: Vec<RecordBatch>,
        select: &SelectStatement,
    ) -> Result<Vec<RecordBatch>> {
        // Extract aggregate functions from projection
        let mut agg_funcs = Vec::new();

        for item in &select.projection {
            if let SelectItem::Expr { expr, alias } = item {
                if let Some(agg_func) = self.extract_aggregate(expr) {
                    let func_alias = alias.clone().or_else(|| Some("agg".to_string()));
                    agg_funcs.push(AggregateFunction {
                        func: agg_func.0,
                        column: agg_func.1,
                        alias: func_alias,
                    });
                }
            }
        }

        let aggregate = Aggregate::new(select.group_by.clone(), agg_funcs);
        let mut result = Vec::new();

        for batch in batches {
            result.push(aggregate.execute(&batch)?);
        }

        Ok(result)
    }

    /// Extract aggregate function from expression.
    fn extract_aggregate(&self, expr: &Expr) -> Option<(AggregateFunc, String)> {
        if let Expr::Function { name, args } = expr {
            let func = match name.to_uppercase().as_str() {
                "COUNT" => Some(AggregateFunc::Count),
                "SUM" => Some(AggregateFunc::Sum),
                "AVG" => Some(AggregateFunc::Avg),
                "MIN" => Some(AggregateFunc::Min),
                "MAX" => Some(AggregateFunc::Max),
                _ => None,
            }?;

            if let Some(arg) = args.first() {
                match arg {
                    Expr::Column { name, .. } => {
                        return Some((func, name.clone()));
                    }
                    Expr::Wildcard => {
                        // COUNT(*) uses any column
                        return Some((func, "*".to_string()));
                    }
                    _ => {}
                }
            } else if matches!(func, AggregateFunc::Count) {
                // COUNT(*) with no args
                return Some((func, "*".to_string()));
            }
        }
        None
    }

    /// Check if projection has aggregates.
    fn has_aggregates(&self, projection: &[SelectItem]) -> bool {
        for item in projection {
            if let SelectItem::Expr { expr, .. } = item {
                if self.extract_aggregate(expr).is_some() {
                    return true;
                }
            }
        }
        false
    }

    /// Execute sort operation.
    fn execute_sort(
        &self,
        batches: Vec<RecordBatch>,
        order_by: &[OrderByExpr],
    ) -> Result<Vec<RecordBatch>> {
        let sort = Sort::new(order_by.to_vec());
        let mut result = Vec::new();

        for batch in batches {
            result.push(sort.execute(&batch)?);
        }

        Ok(result)
    }

    /// Execute LIMIT and OFFSET.
    fn execute_limit_offset(
        &self,
        batches: Vec<RecordBatch>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<RecordBatch>> {
        let offset = offset.unwrap_or(0);
        let mut current_row = 0;
        let mut result = Vec::new();
        let mut remaining = limit;

        for batch in batches {
            if let Some(rem) = remaining {
                if rem == 0 {
                    break;
                }
            }

            let start = if current_row < offset {
                let skip = (offset - current_row).min(batch.num_rows);
                current_row += skip;
                skip
            } else {
                0
            };

            let end = if let Some(rem) = remaining {
                (start + rem).min(batch.num_rows)
            } else {
                batch.num_rows
            };

            if start < end {
                let slice_batch = self.slice_batch(&batch, start, end)?;
                let slice_rows = slice_batch.num_rows;
                result.push(slice_batch);

                if let Some(rem) = &mut remaining {
                    *rem = rem.saturating_sub(slice_rows);
                }
            }

            current_row += batch.num_rows;
        }

        Ok(result)
    }

    /// Slice a record batch.
    fn slice_batch(&self, batch: &RecordBatch, start: usize, end: usize) -> Result<RecordBatch> {
        let mut sliced_columns = Vec::new();

        for column in &batch.columns {
            sliced_columns.push(self.slice_column(column, start, end));
        }

        RecordBatch::new(batch.schema.clone(), sliced_columns, end - start)
    }

    /// Slice a column.
    fn slice_column(
        &self,
        column: &scan::ColumnData,
        start: usize,
        end: usize,
    ) -> scan::ColumnData {
        use scan::ColumnData;

        match column {
            ColumnData::Boolean(data) => ColumnData::Boolean(data[start..end].to_vec()),
            ColumnData::Int32(data) => ColumnData::Int32(data[start..end].to_vec()),
            ColumnData::Int64(data) => ColumnData::Int64(data[start..end].to_vec()),
            ColumnData::Float32(data) => ColumnData::Float32(data[start..end].to_vec()),
            ColumnData::Float64(data) => ColumnData::Float64(data[start..end].to_vec()),
            ColumnData::String(data) => ColumnData::String(data[start..end].to_vec()),
            ColumnData::Binary(data) => ColumnData::Binary(data[start..end].to_vec()),
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::scan::{DataType, Field, MemoryDataSource, Schema};
    use crate::parser::sql::parse_sql;

    #[tokio::test]
    async fn test_executor_simple_query() -> Result<()> {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id".to_string(), DataType::Int64, false),
            Field::new("value".to_string(), DataType::Int64, false),
        ]));

        let columns = vec![
            scan::ColumnData::Int64(vec![Some(1), Some(2), Some(3)]),
            scan::ColumnData::Int64(vec![Some(10), Some(20), Some(30)]),
        ];

        let batch = RecordBatch::new(schema.clone(), columns, 3)?;
        let source = Arc::new(MemoryDataSource::new(schema, vec![batch]));

        let mut executor = Executor::new();
        executor.register_data_source("test_table".to_string(), source);

        let sql = "SELECT * FROM test_table";
        let stmt = parse_sql(sql)?;

        let result = executor.execute(&stmt).await?;
        assert!(!result.is_empty());

        Ok(())
    }
}
