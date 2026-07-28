//! Query execution engine.

pub mod aggregate;
pub mod filter;
pub mod join;
pub(crate) mod like;
pub mod scan;
pub mod sort;
pub mod spatial_funcs;

pub use spatial_funcs::evaluate_spatial_function;

pub mod window;

pub use window::{OrderKey, WindowFunction, WindowSpec, evaluate_window, evaluate_window_batch};

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use aggregate::{Aggregate, AggregateFunc, AggregateFunction};
use filter::{Filter, Value, evaluate_expr_for_row};
use join::Join;
use scan::{ColumnData, DataSource, Field, RecordBatch, Schema, TableScan};
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

        // Execute GROUP BY / aggregation (and HAVING, which is only meaningful
        // for an aggregated result).
        let aggregated = !select.group_by.is_empty()
            || self.has_aggregates(&select.projection)
            || select
                .having
                .as_ref()
                .is_some_and(|h| self.expr_has_aggregate(h));
        if aggregated {
            batches = self.execute_aggregate(batches, select)?;
        } else if select.having.is_some() {
            return Err(QueryError::semantic(
                "HAVING requires an aggregate query (GROUP BY or aggregate functions)",
            ));
        }

        // Execute ORDER BY
        if !select.order_by.is_empty() {
            batches = self.execute_sort(batches, &select.order_by)?;
        }

        // Execute LIMIT and OFFSET
        if select.limit.is_some() || select.offset.is_some() {
            batches = self.execute_limit_offset(batches, select.limit, select.offset)?;
        }

        // Apply the SELECT projection list for the non-aggregate path. For the
        // aggregate path the projection is already materialised by
        // `execute_aggregate` (group-by keys followed by aggregate results), so
        // re-projecting here would be incorrect. Projection runs last so that
        // ORDER BY may still reference source columns that are not in the
        // SELECT list.
        if !aggregated {
            batches = self.execute_projection(batches, &select.projection)?;
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

    /// Execute aggregation, including the `HAVING` filter when present.
    ///
    /// Aggregate specs are collected from the projection (visible, in order)
    /// and additionally from the `HAVING` predicate (any aggregate not already
    /// computed for the projection is appended as a hidden column). After the
    /// per-group aggregates are computed, the `HAVING` predicate is evaluated
    /// against the aggregated batch — its aggregate function references are
    /// rewritten to the corresponding output columns — and non-matching groups
    /// are dropped. Finally the hidden `HAVING`-only columns are removed so the
    /// result contains exactly the projection's group-by keys and aggregates.
    fn execute_aggregate(
        &self,
        batches: Vec<RecordBatch>,
        select: &SelectStatement,
    ) -> Result<Vec<RecordBatch>> {
        // Aggregate specs from the projection, preserving order. Each unique
        // (func, column) gets a canonical output name (its explicit alias, or a
        // derived `FUNC(column)` name) that is stable and referenceable.
        let mut agg_funcs: Vec<AggregateFunction> = Vec::new();
        let mut name_map: HashMap<(AggregateFunc, String), String> = HashMap::new();

        for item in &select.projection {
            if let SelectItem::Expr { expr, alias } = item
                && let Some((func, column)) = self.extract_aggregate(expr)
            {
                let key = (func, column.clone());
                if let std::collections::hash_map::Entry::Vacant(e) = name_map.entry(key) {
                    let name = alias
                        .clone()
                        .unwrap_or_else(|| canonical_agg_name(func, &column));
                    e.insert(name.clone());
                    agg_funcs.push(AggregateFunction {
                        func,
                        column,
                        alias: Some(name),
                    });
                }
            }
        }

        let visible_agg_count = agg_funcs.len();
        let group_count = select.group_by.len();

        // Additional aggregates referenced only by HAVING must also be computed.
        if let Some(having) = select.having.as_ref() {
            let mut having_aggs = Vec::new();
            self.collect_aggregates(having, &mut having_aggs);
            for (func, column) in having_aggs {
                let key = (func, column.clone());
                if let std::collections::hash_map::Entry::Vacant(e) = name_map.entry(key) {
                    let name = canonical_agg_name(func, &column);
                    e.insert(name.clone());
                    agg_funcs.push(AggregateFunction {
                        func,
                        column,
                        alias: Some(name),
                    });
                }
            }
        }

        let aggregate = Aggregate::new(select.group_by.clone(), agg_funcs);
        let visible_cols = group_count + visible_agg_count;
        let mut result = Vec::new();

        for batch in batches {
            let mut agg_batch = aggregate.execute(&batch)?;

            if let Some(having) = select.having.as_ref() {
                let rewritten = self.rewrite_having(having, &name_map)?;
                agg_batch = Filter::new(rewritten).execute(&agg_batch)?;
            }

            // Drop hidden HAVING-only columns, keeping group keys + projection
            // aggregates.
            if agg_batch.columns.len() > visible_cols {
                agg_batch = self.keep_leading_columns(&agg_batch, visible_cols)?;
            }

            result.push(agg_batch);
        }

        Ok(result)
    }

    /// Recursively collect every aggregate function `(func, column)` appearing
    /// in `expr` (used to compute aggregates referenced only by `HAVING`).
    fn collect_aggregates(&self, expr: &Expr, out: &mut Vec<(AggregateFunc, String)>) {
        if let Some(agg) = self.extract_aggregate(expr) {
            out.push(agg);
            return;
        }
        match expr {
            Expr::BinaryOp { left, right, .. } => {
                self.collect_aggregates(left, out);
                self.collect_aggregates(right, out);
            }
            Expr::UnaryOp { expr, .. } => self.collect_aggregates(expr, out),
            Expr::Function { args, .. } => {
                for arg in args {
                    self.collect_aggregates(arg, out);
                }
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                self.collect_aggregates(expr, out);
                self.collect_aggregates(low, out);
                self.collect_aggregates(high, out);
            }
            Expr::InList { expr, list, .. } => {
                self.collect_aggregates(expr, out);
                for item in list {
                    self.collect_aggregates(item, out);
                }
            }
            Expr::Case {
                operand,
                when_then,
                else_result,
            } => {
                if let Some(op) = operand {
                    self.collect_aggregates(op, out);
                }
                for (when, then) in when_then {
                    self.collect_aggregates(when, out);
                    self.collect_aggregates(then, out);
                }
                if let Some(else_expr) = else_result {
                    self.collect_aggregates(else_expr, out);
                }
            }
            Expr::Cast { expr, .. } => self.collect_aggregates(expr, out),
            Expr::IsNull(inner) | Expr::IsNotNull(inner) => self.collect_aggregates(inner, out),
            _ => {}
        }
    }

    /// Whether `expr` contains any aggregate function call.
    fn expr_has_aggregate(&self, expr: &Expr) -> bool {
        let mut out = Vec::new();
        self.collect_aggregates(expr, &mut out);
        !out.is_empty()
    }

    /// Rewrite a `HAVING` predicate so aggregate function calls become column
    /// references into the aggregated batch. Group-by column references and
    /// literals are left unchanged.
    fn rewrite_having(
        &self,
        expr: &Expr,
        name_map: &HashMap<(AggregateFunc, String), String>,
    ) -> Result<Expr> {
        // Aggregate function -> reference to its computed output column.
        if let Some((func, column)) = self.extract_aggregate(expr) {
            let name = name_map.get(&(func, column.clone())).ok_or_else(|| {
                QueryError::internal(format!(
                    "HAVING references aggregate {} that was not planned",
                    canonical_agg_name(func, &column)
                ))
            })?;
            return Ok(Expr::Column {
                table: None,
                name: name.clone(),
            });
        }

        Ok(match expr {
            Expr::BinaryOp { left, op, right } => Expr::BinaryOp {
                left: Box::new(self.rewrite_having(left, name_map)?),
                op: *op,
                right: Box::new(self.rewrite_having(right, name_map)?),
            },
            Expr::UnaryOp { op, expr } => Expr::UnaryOp {
                op: *op,
                expr: Box::new(self.rewrite_having(expr, name_map)?),
            },
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => Expr::Between {
                expr: Box::new(self.rewrite_having(expr, name_map)?),
                low: Box::new(self.rewrite_having(low, name_map)?),
                high: Box::new(self.rewrite_having(high, name_map)?),
                negated: *negated,
            },
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                let mut new_list = Vec::with_capacity(list.len());
                for item in list {
                    new_list.push(self.rewrite_having(item, name_map)?);
                }
                Expr::InList {
                    expr: Box::new(self.rewrite_having(expr, name_map)?),
                    list: new_list,
                    negated: *negated,
                }
            }
            Expr::Case {
                operand,
                when_then,
                else_result,
            } => {
                let operand = match operand {
                    Some(op) => Some(Box::new(self.rewrite_having(op, name_map)?)),
                    None => None,
                };
                let mut new_when_then = Vec::with_capacity(when_then.len());
                for (when, then) in when_then {
                    new_when_then.push((
                        self.rewrite_having(when, name_map)?,
                        self.rewrite_having(then, name_map)?,
                    ));
                }
                let else_result = match else_result {
                    Some(e) => Some(Box::new(self.rewrite_having(e, name_map)?)),
                    None => None,
                };
                Expr::Case {
                    operand,
                    when_then: new_when_then,
                    else_result,
                }
            }
            Expr::Cast { expr, data_type } => Expr::Cast {
                expr: Box::new(self.rewrite_having(expr, name_map)?),
                data_type: *data_type,
            },
            Expr::IsNull(inner) => Expr::IsNull(Box::new(self.rewrite_having(inner, name_map)?)),
            Expr::IsNotNull(inner) => {
                Expr::IsNotNull(Box::new(self.rewrite_having(inner, name_map)?))
            }
            other => other.clone(),
        })
    }

    /// Return a new batch keeping only the first `count` columns.
    fn keep_leading_columns(&self, batch: &RecordBatch, count: usize) -> Result<RecordBatch> {
        let fields = batch.schema.fields[..count].to_vec();
        let columns = batch.columns[..count].to_vec();
        let schema = Arc::new(Schema::new(fields));
        RecordBatch::new(schema, columns, batch.num_rows)
    }

    /// Apply the SELECT projection list to non-aggregate result batches.
    fn execute_projection(
        &self,
        batches: Vec<RecordBatch>,
        projection: &[SelectItem],
    ) -> Result<Vec<RecordBatch>> {
        // A projection consisting solely of wildcards passes every column
        // through unchanged.
        if projection.iter().all(|item| {
            matches!(
                item,
                SelectItem::Wildcard | SelectItem::QualifiedWildcard(_)
            )
        }) {
            return Ok(batches);
        }

        let mut result = Vec::with_capacity(batches.len());
        for batch in batches {
            result.push(self.project_batch(&batch, projection)?);
        }
        Ok(result)
    }

    /// Build a projected batch from `batch` according to `projection`.
    fn project_batch(&self, batch: &RecordBatch, projection: &[SelectItem]) -> Result<RecordBatch> {
        let mut fields: Vec<Field> = Vec::new();
        let mut columns: Vec<ColumnData> = Vec::new();

        for item in projection {
            match item {
                SelectItem::Wildcard | SelectItem::QualifiedWildcard(_) => {
                    // No per-column table qualifier is tracked at this layer, so
                    // a qualified wildcard passes through all columns.
                    for (field, column) in batch.schema.fields.iter().zip(&batch.columns) {
                        fields.push(field.clone());
                        columns.push(column.clone());
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    if let Expr::Column { name, .. } = expr {
                        // Fast path: a plain column reference preserves the
                        // source column's exact type.
                        let idx = batch
                            .schema
                            .index_of(name)
                            .ok_or_else(|| QueryError::ColumnNotFound(name.clone()))?;
                        let src_field = &batch.schema.fields[idx];
                        let field_name = alias.clone().unwrap_or_else(|| name.clone());
                        fields.push(Field::new(
                            field_name,
                            src_field.data_type,
                            src_field.nullable,
                        ));
                        columns.push(batch.columns[idx].clone());
                    } else {
                        // General scalar expression: evaluate it per row.
                        let mut values = Vec::with_capacity(batch.num_rows);
                        for row in 0..batch.num_rows {
                            values.push(evaluate_expr_for_row(expr, batch, row)?);
                        }
                        let (column, data_type) = values_to_projection_column(&values)?;
                        let nullable = values.iter().any(|v| matches!(v, Value::Null));
                        let name = alias.clone().unwrap_or_else(|| expr.to_string());
                        fields.push(Field::new(name, data_type, nullable));
                        columns.push(column);
                    }
                }
            }
        }

        let schema = Arc::new(Schema::new(fields));
        RecordBatch::new(schema, columns, batch.num_rows)
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
            if let SelectItem::Expr { expr, .. } = item
                && self.extract_aggregate(expr).is_some()
            {
                return true;
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
            if let Some(rem) = remaining
                && rem == 0
            {
                break;
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

/// Canonical output name for an aggregate `(func, column)` without an explicit
/// alias, e.g. `COUNT(*)`, `AVG(score)`.
fn canonical_agg_name(func: AggregateFunc, column: &str) -> String {
    let fname = match func {
        AggregateFunc::Count => "COUNT",
        AggregateFunc::Sum => "SUM",
        AggregateFunc::Avg => "AVG",
        AggregateFunc::Min => "MIN",
        AggregateFunc::Max => "MAX",
    };
    format!("{}({})", fname, column)
}

/// Materialise a projected scalar expression's per-row [`Value`]s into a typed
/// [`ColumnData`] plus the corresponding [`scan::DataType`].
///
/// The target variant is chosen by scanning the value types (string > float >
/// int > bool), with numeric widening applied as needed. An all-NULL result
/// defaults to `Int64`. Geometry-valued expressions have no column storage and
/// produce an honest error rather than silently dropping data.
fn values_to_projection_column(values: &[Value]) -> Result<(ColumnData, scan::DataType)> {
    use scan::DataType as Dt;

    let mut has_string = false;
    let mut has_float = false;
    let mut has_int = false;
    let mut has_bool = false;

    for v in values {
        match v {
            Value::Null => {}
            Value::String(_) => has_string = true,
            Value::Float32(_) | Value::Float64(_) => has_float = true,
            Value::Int32(_) | Value::Int64(_) => has_int = true,
            Value::Boolean(_) => has_bool = true,
            Value::Geometry(_) => {
                return Err(QueryError::unsupported(
                    "projecting a geometry-valued expression is not supported (no geometry column storage)",
                ));
            }
        }
    }

    let mismatch = || QueryError::execution("projection expression produced mixed value types");

    if has_string {
        let col = values
            .iter()
            .map(|v| match v {
                Value::String(s) => Ok(Some(s.clone())),
                Value::Null => Ok(None),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((ColumnData::String(col), Dt::String));
    }
    if has_float {
        let col = values
            .iter()
            .map(|v| match v {
                Value::Float64(f) => Ok(Some(*f)),
                Value::Float32(f) => Ok(Some(*f as f64)),
                Value::Int64(i) => Ok(Some(*i as f64)),
                Value::Int32(i) => Ok(Some(*i as f64)),
                Value::Null => Ok(None),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((ColumnData::Float64(col), Dt::Float64));
    }
    if has_int {
        let col = values
            .iter()
            .map(|v| match v {
                Value::Int64(i) => Ok(Some(*i)),
                Value::Int32(i) => Ok(Some(*i as i64)),
                Value::Null => Ok(None),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((ColumnData::Int64(col), Dt::Int64));
    }
    if has_bool {
        let col = values
            .iter()
            .map(|v| match v {
                Value::Boolean(b) => Ok(Some(*b)),
                Value::Null => Ok(None),
                _ => Err(mismatch()),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((ColumnData::Boolean(col), Dt::Boolean));
    }

    // All NULL: type is indeterminate; default to a NULL Int64 column.
    Ok((
        ColumnData::Int64(values.iter().map(|_| None).collect()),
        Dt::Int64,
    ))
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
