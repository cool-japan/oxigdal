//! Pure metadata → byte-range planning for GeoParquet predicate pushdown.
//!
//! This module turns a parsed Parquet footer ([`ParquetMetaData`]) plus a
//! spatial/attribute query into a concrete [`PushdownPlan`]: the set of
//! surviving row groups and the exact `(start, length)` byte ranges of the
//! column chunks that must be fetched to answer the query.  It performs **no
//! I/O** — it only reads footer metadata already in memory.
//!
//! The plan drives two consumers:
//! * the local reader ([`crate::reader::GeoParquetReader::read_pushdown`]),
//!   which reads those row groups from an open file, and
//! * the remote / WASM path (`oxigdal-wasm-geoparquet`), which coalesces the
//!   byte ranges into HTTP Range requests before decoding.
//!
//! ## Pruning stages
//!
//! 1. **Spatial row-group pruning** ([`prune_row_groups`]): when a query bbox
//!    and GeoParquet 1.1 covering bbox columns are both present, row groups
//!    whose covering AABB is disjoint from the query are dropped.
//! 2. **Attribute stats pruning**: for each attribute filter, a row group is
//!    dropped when its column min/max statistics prove that no row can match.
//! 3. **Leaf-set selection**: the union of covering-bbox leaves, filter-column
//!    leaves, and requested output-column leaves determines which column chunks
//!    are actually needed — everything else is skipped.

use crate::covering::BboxColumns;
use crate::error::Result;
use crate::metadata::GeoParquetMetadata;
use crate::predicate::{AttributeFilter, CmpOp, ScalarValue};
use parquet::file::metadata::ParquetMetaData;
use parquet::file::statistics::Statistics;
use parquet::schema::types::SchemaDescriptor;
use std::collections::BTreeSet;

/// A single column-chunk byte range within one row group.
///
/// `start` is inclusive of the dictionary page (matching
/// [`parquet::file::metadata::ColumnChunkMetaData::byte_range`]); `length` is
/// the total compressed size of the chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnChunkRange {
    /// Row-group index this chunk belongs to.
    pub row_group: usize,
    /// Leaf column index (as in [`SchemaDescriptor::columns`]).
    pub leaf_column: usize,
    /// Byte offset of the chunk within the file (includes dictionary page).
    pub start: u64,
    /// Total compressed length of the chunk in bytes.
    pub length: u64,
}

/// The result of [`plan_pushdown`]: which row groups survive and exactly which
/// bytes need fetching to read them.
#[derive(Debug, Clone)]
pub struct PushdownPlan {
    /// Surviving row-group indices after spatial + attribute pruning, ascending.
    pub row_groups: Vec<usize>,
    /// Total number of row groups in the file (for progress / ratio display).
    pub total_row_groups: usize,
    /// Byte ranges of every column chunk that must be fetched, ordered by
    /// `(row_group, leaf_column)`.
    pub ranges: Vec<ColumnChunkRange>,
    /// Sum of `length` over [`Self::ranges`] — the number of compressed bytes
    /// the query will read (excluding the footer).
    pub estimated_bytes: u64,
    /// Detected covering bbox columns, when present.
    pub bbox_cols: Option<BboxColumns>,
}

/// Build a [`PushdownPlan`] from footer metadata and a query.
///
/// * `meta` — parsed Parquet footer.
/// * `geo` — the GeoParquet `geo` metadata (authoritative source of covering
///   column paths).
/// * `geometry_column` — primary geometry column name.
/// * `bbox` — optional query bounding box `(xmin, ymin, xmax, ymax)`.
/// * `filters` — attribute filters, combined conjunctively.
/// * `output_columns` — column names the caller wants back.  When empty, all
///   leaf columns are included in the fetch set.
///
/// # Errors
///
/// Currently infallible in practice; the `Result` is reserved for future
/// structural-integrity checks so callers stay forward-compatible.
pub fn plan_pushdown(
    meta: &ParquetMetaData,
    geo: &GeoParquetMetadata,
    geometry_column: &str,
    bbox: Option<(f64, f64, f64, f64)>,
    filters: &[AttributeFilter],
    output_columns: &[String],
) -> Result<PushdownPlan> {
    let schema_descr = meta.file_metadata().schema_descr();
    let bbox_cols = BboxColumns::detect_with_covering(schema_descr, geometry_column, geo);

    // ── 1. Spatial row-group pruning ────────────────────────────────────────
    let spatial_survivors = prune_row_groups(meta, bbox_cols.as_ref(), bbox);

    // ── 2. Attribute stats pruning ──────────────────────────────────────────
    let survivors: Vec<usize> = spatial_survivors
        .into_iter()
        .filter(|&rg| attribute_stats_allow(meta, schema_descr, rg, filters))
        .collect();

    // ── 3. Leaf-set = union(bbox ∪ filter ∪ output) ─────────────────────────
    let mut leaves: BTreeSet<usize> = BTreeSet::new();
    if let Some(bc) = bbox_cols.as_ref() {
        leaves.insert(bc.xmin_col);
        leaves.insert(bc.ymin_col);
        leaves.insert(bc.xmax_col);
        leaves.insert(bc.ymax_col);
    }
    for filter in filters {
        for leaf in leaves_for_root_name(schema_descr, filter.col_name()) {
            leaves.insert(leaf);
        }
    }
    for col in output_columns {
        for leaf in leaves_for_root_name(schema_descr, col) {
            leaves.insert(leaf);
        }
    }
    // No explicit leaves selected → fetch everything.
    if leaves.is_empty() {
        leaves.extend(0..schema_descr.num_columns());
    }

    // ── 4. Byte ranges per (row group, leaf) ────────────────────────────────
    let mut ranges = Vec::with_capacity(survivors.len() * leaves.len());
    let mut estimated_bytes = 0u64;
    for &rg in &survivors {
        let rgm = meta.row_group(rg);
        for &leaf in &leaves {
            let (start, length) = rgm.column(leaf).byte_range();
            estimated_bytes += length;
            ranges.push(ColumnChunkRange {
                row_group: rg,
                leaf_column: leaf,
                start,
                length,
            });
        }
    }

    Ok(PushdownPlan {
        row_groups: survivors,
        total_row_groups: meta.num_row_groups(),
        ranges,
        estimated_bytes,
        bbox_cols,
    })
}

/// Prune row groups by covering-bbox AABB intersection against a query bbox.
///
/// Returns the ascending list of surviving row-group indices.  When no query
/// bbox is supplied, or covering columns / statistics are unavailable, all row
/// groups survive (nothing can be pruned safely).
///
/// This is the single source of truth for spatial row-group pruning, shared by
/// [`plan_pushdown`] and [`crate::reader::GeoParquetReader::read_pushdown`].
pub fn prune_row_groups(
    meta: &ParquetMetaData,
    bbox_cols: Option<&BboxColumns>,
    bbox: Option<(f64, f64, f64, f64)>,
) -> Vec<usize> {
    let Some((qxmin, qymin, qxmax, qymax)) = bbox else {
        return (0..meta.num_row_groups()).collect();
    };
    (0..meta.num_row_groups())
        .filter(|&rg_idx| {
            let rg = meta.row_group(rg_idx);
            if let Some(bc) = bbox_cols
                && let Some((rxmin, rymin, rxmax, rymax)) = bc.row_group_bbox(rg)
            {
                // AABB intersection (inclusive on edges).
                return rxmax >= qxmin && rxmin <= qxmax && rymax >= qymin && rymin <= qymax;
            }
            // No stats → can't prune, keep the row group.
            true
        })
        .collect()
}

// ── Attribute statistics pruning ────────────────────────────────────────────────

/// Returns `true` if row group `rg_idx` could contain a row satisfying **all**
/// `filters`, judged from column min/max statistics alone.
///
/// Conservative: when statistics are missing, non-numeric, or ambiguous the row
/// group is kept.  Only numeric proof of non-overlap prunes it.
fn attribute_stats_allow(
    meta: &ParquetMetaData,
    schema_descr: &SchemaDescriptor,
    rg_idx: usize,
    filters: &[AttributeFilter],
) -> bool {
    let rgm = meta.row_group(rg_idx);
    for filter in filters {
        let Some(leaf) = leaf_for_top_level(schema_descr, filter.col_name()) else {
            continue;
        };
        let Some(stats) = rgm.column(leaf).statistics() else {
            continue;
        };
        let Some((min, max)) = stats_f64_range(stats) else {
            continue;
        };
        if !filter_could_match(filter, min, max) {
            return false;
        }
    }
    true
}

/// Whether a filter *could* match some value in the numeric range `[min, max]`.
fn filter_could_match(filter: &AttributeFilter, min: f64, max: f64) -> bool {
    match filter {
        AttributeFilter::Eq { value, .. } => match scalar_f64(value) {
            Some(v) => min <= v && v <= max,
            None => true,
        },
        AttributeFilter::Range { lo, hi, .. } => {
            let lo_v = scalar_f64(lo);
            let hi_v = scalar_f64(hi);
            match (lo_v, hi_v) {
                // Overlap of [lo, hi] with [min, max].
                (Some(lo_v), Some(hi_v)) => !(hi_v < min || lo_v > max),
                _ => true,
            }
        }
        AttributeFilter::In { values, .. } => {
            let mut any_numeric = false;
            for v in values {
                match scalar_f64(v) {
                    Some(v) => {
                        any_numeric = true;
                        if min <= v && v <= max {
                            return true;
                        }
                    }
                    None => return true, // non-numeric member → can't prove no-match
                }
            }
            // If every member was numeric and none fell in range → prune.
            !any_numeric
        }
        AttributeFilter::Cmp { op, value, .. } => match scalar_f64(value) {
            Some(v) => match op {
                CmpOp::Gt => max > v,
                CmpOp::Ge => max >= v,
                CmpOp::Lt => min < v,
                CmpOp::Le => min <= v,
                // Only prunes the degenerate all-equal-to-v row group.
                CmpOp::NotEq => !(min == v && max == v),
            },
            None => true,
        },
    }
}

/// Extract a numeric `(min, max)` pair from Parquet statistics, if the physical
/// type is integer or floating point and both bounds are present.
fn stats_f64_range(stats: &Statistics) -> Option<(f64, f64)> {
    match stats {
        Statistics::Int32(t) => Some((f64::from(*t.min_opt()?), f64::from(*t.max_opt()?))),
        Statistics::Int64(t) => Some((*t.min_opt()? as f64, *t.max_opt()? as f64)),
        Statistics::Float(t) => Some((f64::from(*t.min_opt()?), f64::from(*t.max_opt()?))),
        Statistics::Double(t) => Some((*t.min_opt()?, *t.max_opt()?)),
        _ => None,
    }
}

/// Convert a numeric [`ScalarValue`] to `f64`; returns `None` for non-numeric
/// variants (e.g. `Utf8`, `Binary`).
fn scalar_f64(v: &ScalarValue) -> Option<f64> {
    match v {
        ScalarValue::Int32(x) => Some(f64::from(*x)),
        ScalarValue::Int64(x) => Some(*x as f64),
        ScalarValue::Float32(x) => Some(f64::from(*x)),
        ScalarValue::Float64(x) => Some(*x),
        _ => None,
    }
}

// ── Leaf lookup helpers ─────────────────────────────────────────────────────────

/// Find the leaf index of a top-level primitive column named `name`.
///
/// Only matches columns whose path is exactly `[name]` (top-level scalars),
/// which is the shape of all attribute filter columns.
fn leaf_for_top_level(schema_descr: &SchemaDescriptor, name: &str) -> Option<usize> {
    (0..schema_descr.num_columns()).find(|&i| {
        let col = schema_descr.column(i);
        let parts = col.path().parts();
        parts.len() == 1 && parts[0] == name
    })
}

/// All leaf indices whose root (first path component) equals `name`.
///
/// Resolves both scalar roots (`area_in_meters` → one leaf) and struct roots
/// (`bbox` → its four child leaves).
fn leaves_for_root_name(schema_descr: &SchemaDescriptor, name: &str) -> Vec<usize> {
    (0..schema_descr.num_columns())
        .filter(|&i| {
            let col = schema_descr.column(i);
            col.path().parts().first().is_some_and(|p| p == name)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_could_match_cmp_gt() {
        // area_in_meters > 1000, row group with values in [10, 500] → prune.
        let f = AttributeFilter::Cmp {
            col: "area".into(),
            op: CmpOp::Gt,
            value: ScalarValue::Float64(1000.0),
        };
        assert!(!filter_could_match(&f, 10.0, 500.0));
        assert!(filter_could_match(&f, 10.0, 5000.0));
    }

    #[test]
    fn test_filter_could_match_range() {
        let f = AttributeFilter::Range {
            col: "pop".into(),
            lo: ScalarValue::Int64(0),
            hi: ScalarValue::Int64(100),
        };
        assert!(filter_could_match(&f, 50.0, 200.0)); // overlaps
        assert!(!filter_could_match(&f, 500.0, 1000.0)); // disjoint above
        assert!(!filter_could_match(&f, -500.0, -1.0)); // disjoint below
    }

    #[test]
    fn test_filter_could_match_eq() {
        let f = AttributeFilter::Eq {
            col: "pop".into(),
            value: ScalarValue::Int64(42),
        };
        assert!(filter_could_match(&f, 0.0, 100.0));
        assert!(!filter_could_match(&f, 50.0, 100.0));
    }

    #[test]
    fn test_filter_could_match_utf8_is_conservative() {
        // Non-numeric filters are never pruned via numeric stats.
        let f = AttributeFilter::Eq {
            col: "name".into(),
            value: ScalarValue::Utf8("alpha".into()),
        };
        assert!(filter_could_match(&f, 0.0, 0.0));
    }
}
