//! Generic pushdown execution over any [`ChunkReader`].
//!
//! [`execute_pushdown`] is the reader-agnostic core of GeoParquet predicate
//! pushdown.  Given a Parquet input that implements [`ChunkReader`] — a local
//! `File`, an in-memory `Bytes`, or the sparse remote reader used by the WASM
//! query engine — plus pre-decoded [`ArrowReaderMetadata`], it applies covering
//! bbox and attribute row filters, an optional projection and limit, and a WKB
//! post-filter fallback for files lacking covering columns.
//!
//! Row-group pruning is *not* performed here: callers pass the already-pruned
//! `row_groups` (typically from [`crate::plan::plan_pushdown`] or
//! [`crate::plan::prune_row_groups`]).  This keeps I/O planning and decode
//! cleanly separated so the remote path can fetch only the surviving chunks
//! before ever constructing the reader.

use crate::covering::BboxColumns;
use crate::error::{GeoParquetError, Result};
use crate::filter::filter_batch_by_mask;
use crate::geometry::native::native_bbox_mask;
use crate::geometry::wkb_bbox;
use crate::metadata::{EncodingType, GeoParquetMetadata};
use crate::predicate::{AttributeFilter, CoveringBboxPredicate, col_name_from_leaf};
use arrow_array::{Array, BinaryArray, RecordBatch};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{
    ArrowPredicate, ArrowReaderMetadata, ParquetRecordBatchReaderBuilder, RowFilter,
};
use parquet::file::reader::ChunkReader;

/// Execute a pushdown read over `input`, returning matching record batches.
///
/// * `input` — any [`ChunkReader`] (local file, `Bytes`, sparse remote reader).
/// * `arrow_meta` — decoded Arrow+Parquet metadata (constructed once, reused).
/// * `geo` — GeoParquet `geo` metadata (covering column source + encoding).
/// * `geometry_column` — primary geometry column name.
/// * `bbox` — optional query bounding box `(xmin, ymin, xmax, ymax)`.
/// * `filters` — attribute filters combined conjunctively via `RowFilter`.
/// * `row_groups` — the (already pruned) row groups to scan.
/// * `projection` — optional column projection; `None` reads all columns.
/// * `limit` — optional maximum row count.
///
/// Applies, in order:
/// 1. covering.bbox `ArrowPredicate` (row-level, no WKB decode) when covering
///    columns are present and a `bbox` is set;
/// 2. attribute `ArrowPredicate`s;
/// 3. a WKB/native bbox post-filter fallback when covering columns are absent
///    but a `bbox` is set.
///
/// # Errors
///
/// Propagates Parquet, Arrow, and predicate-compilation errors.
#[allow(clippy::too_many_arguments)]
pub fn execute_pushdown<R: ChunkReader + 'static>(
    input: R,
    arrow_meta: ArrowReaderMetadata,
    geo: &GeoParquetMetadata,
    geometry_column: &str,
    bbox: Option<(f64, f64, f64, f64)>,
    filters: &[AttributeFilter],
    row_groups: Vec<usize>,
    projection: Option<ProjectionMask>,
    limit: Option<usize>,
) -> Result<Vec<RecordBatch>> {
    let builder = ParquetRecordBatchReaderBuilder::new_with_metadata(input, arrow_meta);
    let parquet_schema = builder.parquet_schema().clone();
    let arrow_schema = builder.schema().clone();

    // Detect covering bbox columns (covering metadata first, then heuristics).
    let bbox_cols = BboxColumns::detect_with_covering(&parquet_schema, geometry_column, geo);
    let has_covering_bbox = bbox_cols.is_some();

    let mut builder = builder.with_row_groups(row_groups);
    if let Some(proj) = projection {
        builder = builder.with_projection(proj);
    }

    // ── RowFilter predicates ─────────────────────────────────────────────────
    let mut predicates: Vec<Box<dyn ArrowPredicate>> = Vec::new();

    // Covering bbox fast-path (GeoParquet 1.1): skip WKB decode entirely.
    if let (Some((qxmin, qymin, qxmax, qymax)), Some(bc)) = (bbox, &bbox_cols) {
        let xmin_name = col_name_from_leaf(&parquet_schema, bc.xmin_col);
        let ymin_name = col_name_from_leaf(&parquet_schema, bc.ymin_col);
        let xmax_name = col_name_from_leaf(&parquet_schema, bc.xmax_col);
        let ymax_name = col_name_from_leaf(&parquet_schema, bc.ymax_col);

        let bbox_proj = ProjectionMask::leaves(
            &parquet_schema,
            [bc.xmin_col, bc.ymin_col, bc.xmax_col, bc.ymax_col],
        );
        let bbox_pred = CoveringBboxPredicate::new(
            xmin_name, ymin_name, xmax_name, ymax_name, qxmin, qymin, qxmax, qymax, bbox_proj,
        );
        predicates.push(Box::new(bbox_pred));
    }

    // Attribute predicates.
    for filter in filters {
        let pred = filter
            .clone()
            .to_arrow_predicate(arrow_schema.clone(), &parquet_schema)?;
        predicates.push(pred);
    }

    if !predicates.is_empty() {
        builder = builder.with_row_filter(RowFilter::new(predicates));
    }
    if let Some(l) = limit {
        builder = builder.with_limit(l);
    }

    let mut reader = builder.build()?;

    // ── Collect + optional WKB/native post-filter fallback ──────────────────
    let wkb_bbox_tuple = if bbox.is_some() && !has_covering_bbox {
        bbox
    } else {
        None
    };
    let encoding = geo
        .get_column(geometry_column)
        .map(|c| c.encoding)
        .unwrap_or(EncodingType::Wkb);

    let mut results = Vec::new();
    for batch_result in &mut reader {
        let batch = batch_result?;
        if let Some((qxmin, qymin, qxmax, qymax)) = wkb_bbox_tuple {
            let mask = match encoding {
                EncodingType::Wkb => {
                    wkb_bbox_mask(&batch, geometry_column, qxmin, qymin, qxmax, qymax)?
                }
                native => {
                    let geom_col = batch
                        .column_by_name(geometry_column)
                        .ok_or_else(|| GeoParquetError::missing_field(geometry_column))?;
                    native_bbox_mask(geom_col.as_ref(), native, qxmin, qymin, qxmax, qymax)?
                }
            };
            if mask.iter().any(|&b| b) {
                let filtered = filter_batch_by_mask(&batch, &mask)?;
                if filtered.num_rows() > 0 {
                    results.push(filtered);
                }
            }
        } else if batch.num_rows() > 0 {
            results.push(batch);
        }
    }

    Ok(results)
}

/// Builds a WKB-based boolean mask checking each row's geometry bbox against
/// `(qxmin, qymin, qxmax, qymax)`.
fn wkb_bbox_mask(
    batch: &RecordBatch,
    geom_col: &str,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let col = batch
        .column_by_name(geom_col)
        .ok_or_else(|| GeoParquetError::missing_field(geom_col))?;

    let binary = col.as_any().downcast_ref::<BinaryArray>().ok_or_else(|| {
        GeoParquetError::type_mismatch("BinaryArray", format!("{:?}", col.data_type()))
    })?;

    let mut mask = vec![false; binary.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if binary.is_null(i) {
            continue;
        }
        let wkb = binary.value(i);
        if let Some((xmin, ymin, xmax, ymax)) = wkb_bbox(wkb)
            && xmax >= qxmin
            && xmin <= qxmax
            && ymax >= qymin
            && ymin <= qymax
        {
            *m = true;
        }
    }
    Ok(mask)
}
