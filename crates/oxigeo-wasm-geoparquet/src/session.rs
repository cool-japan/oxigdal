//! `RemoteGeoParquet` — the JS-facing open / plan / query session.
//!
//! `open(url)` fetches the file's footer and decodes the Parquet +
//! GeoParquet metadata exactly once (`open_with_footer` accepts a
//! JS-cached footer to skip that download); `plan(...)` is a synchronous,
//! zero-fetch cost preview; `query(...)` runs the full pipeline —
//! plan → too-broad guard → coalesce → cache-aware HTTP range fetch →
//! [`SparseChunkReader`] → [`execute_pushdown`] → GeoJSON conversion —
//! reporting per-query byte and request accounting.
//!
//! Implemented by WP C4 (GeoParquet Live lane); stub created by WP W0.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use bytes::Bytes;
use js_sys::{Array, Object, Reflect, Uint32Array};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ArrowReaderMetadata;
use parquet::errors::ParquetError;
use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use oxigeo_geoparquet::arrow_ext::extract_geoparquet_metadata;
use oxigeo_geoparquet::metadata::GeoParquetMetadata;
use oxigeo_geoparquet::plan::{ColumnChunkRange, plan_pushdown};
use oxigeo_geoparquet::predicate::AttributeFilter;
use oxigeo_geoparquet::pushdown::execute_pushdown;

use crate::coalesce::{ChunkRange, coalesce};
use crate::convert::record_batches_to_geojson;
use crate::error::GpqLiveError;
use crate::fetch::{self, DEFAULT_CONCURRENCY, content_length, fetch_range, fetch_ranges};
use crate::filter_expr::parse_filter_expr;
use crate::sparse::{Segment, SparseChunkReader};

/// Parquet trailer magic (`bytes=size-4 .. size`).
const PARQUET_MAGIC: &[u8] = b"PAR1";

/// Attribute column totalled into `total_area_m2` for the demo badge.
const AREA_COLUMN: &str = "area_in_meters";

/// Upper bound on the per-session column-chunk cache, in bytes (256 MiB).
///
/// Fetched column chunks are memoised so tightening a filter or nudging the
/// query box re-uses already-downloaded bytes; once the cache would exceed
/// this many bytes the least-recently-inserted chunks are evicted.
const CHUNK_CACHE_CAP: usize = 256 * 1024 * 1024;

/// A live handle to a remote GeoParquet file, exported to JavaScript.
///
/// The heavy [`ParquetMetaData`] (tens of MB for a 5.9 GB file) is held behind
/// a single [`Arc`] and **never cloned**; `ArrowReaderMetadata` shares the same
/// `Arc` internally, so cloning it per query is cheap.
#[wasm_bindgen]
pub struct RemoteGeoParquet {
    url: String,
    size: u64,
    footer_len: u32,
    arrow_meta: ArrowReaderMetadata,
    parquet_meta: Arc<ParquetMetaData>,
    geo: GeoParquetMetadata,
    geometry_column: String,
    chunk_cache: BTreeMap<(usize, usize), Bytes>,
    cache_bytes: usize,
}

#[wasm_bindgen]
impl RemoteGeoParquet {
    /// Open a remote GeoParquet by probing its size and fetching + decoding the
    /// footer (last-8-bytes → footer length → footer thrift → metadata).
    ///
    /// # Errors
    /// Rejects with a `{code, message, detail}` payload on any fetch failure,
    /// a malformed trailer, or missing GeoParquet `geo` metadata.
    pub async fn open(url: String) -> Result<RemoteGeoParquet, JsValue> {
        let size = content_length(&url).await?;
        if size < 8 {
            return Err(GpqLiveError::Parquet(ParquetError::General(format!(
                "file too small to be Parquet: {size} bytes"
            )))
            .into());
        }
        // Trailer: [footer_len: u32 LE][b"PAR1"].
        let tail = fetch_range(&url, size - 8, 8).await?;
        if &tail[4..8] != PARQUET_MAGIC {
            return Err(GpqLiveError::Parquet(ParquetError::General(
                "missing PAR1 magic; not a Parquet file".into(),
            ))
            .into());
        }
        let footer_len = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
        let footer_start = (size - 8)
            .checked_sub(u64::from(footer_len))
            .ok_or_else(|| {
                GpqLiveError::Parquet(ParquetError::General(
                    "footer length exceeds file size".into(),
                ))
            })?;
        let footer = fetch_range(&url, footer_start, u64::from(footer_len)).await?;
        Self::finish_open(url, size, footer_len, &footer).map_err(JsValue::from)
    }

    /// Open using a footer supplied by the JavaScript caller (Cache API hit),
    /// avoiding the multi-megabyte footer download.  The file size is still
    /// probed with a one-byte range request.
    ///
    /// # Errors
    /// Rejects on a size-probe failure, an undecodable footer, or missing
    /// GeoParquet `geo` metadata.
    pub async fn open_with_footer(
        url: String,
        footer: js_sys::Uint8Array,
    ) -> Result<RemoteGeoParquet, JsValue> {
        let size = content_length(&url).await?;
        let bytes = footer.to_vec();
        let footer_len = bytes.len() as u32;
        Self::finish_open(url, size, footer_len, &bytes).map_err(JsValue::from)
    }

    /// Footer-level facts for the load UI: `{footerBytes, rows, rowGroups,
    /// sizeBytes, columns:[{name, physicalType}]}`.
    #[must_use]
    pub fn footer_info(&self) -> JsValue {
        let obj = Object::new();
        let fm = self.parquet_meta.file_metadata();
        set(
            &obj,
            "footerBytes",
            &JsValue::from_f64(f64::from(self.footer_len)),
        );
        set(&obj, "rows", &JsValue::from_f64(fm.num_rows() as f64));
        set(
            &obj,
            "rowGroups",
            &JsValue::from_f64(self.parquet_meta.num_row_groups() as f64),
        );
        set(&obj, "sizeBytes", &JsValue::from_f64(self.size as f64));

        let cols = Array::new();
        let schema_descr = fm.schema_descr();
        for i in 0..schema_descr.num_columns() {
            let descr = schema_descr.column(i);
            let col = Object::new();
            set(&col, "name", &JsValue::from_str(descr.name()));
            set(
                &col,
                "physicalType",
                &JsValue::from_str(&format!("{:?}", descr.physical_type())),
            );
            cols.push(&col);
        }
        set(&obj, "columns", &cols);
        obj.into()
    }

    /// Synchronous, zero-fetch cost preview for a candidate query box + filter.
    ///
    /// Returns `{rowGroups:Uint32Array, totalRowGroups, estimatedBytes,
    /// requests}` where `requests` reflects the number of HTTP range requests
    /// that would actually be issued *after* cache hits are subtracted.
    ///
    /// # Errors
    /// Returns the filter-expression error when `filter_expr` cannot be lowered.
    pub fn plan(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        filter_expr: Option<String>,
        columns: JsValue,
    ) -> Result<JsValue, JsValue> {
        let filters = parse_filters(filter_expr.as_deref())?;
        let output_columns = self.output_columns(&columns);
        let bbox = Some((min_x, min_y, max_x, max_y));
        let plan = plan_pushdown(
            &self.parquet_meta,
            &self.geo,
            &self.geometry_column,
            bbox,
            &filters,
            &output_columns,
        )
        .map_err(GpqLiveError::from)?;

        // Cache-aware request preview: only the not-yet-fetched chunks cost a
        // request, and only after coalescing.
        let missing = self.missing_ranges(&plan.ranges);
        let coalesced = coalesce(&missing);

        let obj = Object::new();
        set(&obj, "rowGroups", &survivors_array(&plan.row_groups));
        set(
            &obj,
            "totalRowGroups",
            &JsValue::from_f64(plan.total_row_groups as f64),
        );
        set(
            &obj,
            "estimatedBytes",
            &JsValue::from_f64(plan.estimated_bytes as f64),
        );
        set(
            &obj,
            "requests",
            &JsValue::from_f64(coalesced.request_count() as f64),
        );
        Ok(obj.into())
    }

    /// Execute a spatial + attribute query, returning a GeoJSON result plus
    /// scan / accounting telemetry.
    ///
    /// The pipeline: plan → too-broad guard (`max_row_groups`) → partition
    /// chunk ranges into cache hits vs misses → coalesce + fetch the misses →
    /// assemble a [`SparseChunkReader`] → [`execute_pushdown`] over exactly the
    /// fetched bytes → convert to GeoJSON.  `limit == 0` means unbounded.
    ///
    /// # Errors
    /// Returns [`GpqLiveError::TooBroad`] when the plan exceeds `max_row_groups`,
    /// a fetch error on any range failure, or a filter/parquet/geo error.
    #[allow(clippy::too_many_arguments)]
    pub async fn query(
        &mut self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        filter_expr: Option<String>,
        columns: JsValue,
        limit: u32,
        max_row_groups: u32,
    ) -> Result<JsValue, JsValue> {
        let start_ms = js_sys::Date::now();
        let filters = parse_filters(filter_expr.as_deref())?;
        let output_columns = self.output_columns(&columns);
        let bbox = Some((min_x, min_y, max_x, max_y));

        let plan = plan_pushdown(
            &self.parquet_meta,
            &self.geo,
            &self.geometry_column,
            bbox,
            &filters,
            &output_columns,
        )
        .map_err(GpqLiveError::from)?;

        // ── Too-broad guard ─────────────────────────────────────────────────
        if plan.row_groups.len() > max_row_groups as usize {
            return Err(GpqLiveError::TooBroad {
                row_groups: plan.row_groups.len(),
                estimated_bytes: plan.estimated_bytes,
            }
            .into());
        }

        // Per-query accounting deltas snapshot the global fetch counters.
        let bytes_before = fetch::bytes_fetched_total();
        let requests_before = fetch::request_count_total();

        // ── Partition plan ranges into cache hits vs misses ─────────────────
        let mut query_chunks: HashMap<(usize, usize), Bytes> = HashMap::new();
        let mut missing: Vec<ChunkRange> = Vec::new();
        for range in &plan.ranges {
            let key = (range.row_group, range.leaf_column);
            if let Some(bytes) = self.chunk_cache.get(&key) {
                query_chunks.insert(key, bytes.clone());
            } else {
                missing.push(to_chunk_range(range));
            }
        }

        // ── Fetch the misses (coalesced) ────────────────────────────────────
        let mut fetched: Vec<((usize, usize), Bytes)> = Vec::new();
        if !missing.is_empty() {
            let coalesced = coalesce(&missing);
            let buffers = fetch_ranges(&self.url, &coalesced.fetches, DEFAULT_CONCURRENCY).await?;
            let segments = coalesced.segments(&buffers)?;
            let by_start: HashMap<u64, Bytes> =
                segments.into_iter().map(|s| (s.start, s.data)).collect();
            for m in &missing {
                if let Some(data) = by_start.get(&m.start) {
                    let key = (m.row_group, m.leaf_column);
                    query_chunks.insert(key, data.clone());
                    fetched.push((key, data.clone()));
                }
            }
        }

        // ── Assemble the sparse reader from the complete chunk set ───────────
        let segments: Vec<Segment> = plan
            .ranges
            .iter()
            .filter_map(|r| {
                query_chunks
                    .get(&(r.row_group, r.leaf_column))
                    .map(|data| Segment {
                        start: r.start,
                        data: data.clone(),
                    })
            })
            .collect();
        let reader = SparseChunkReader::new(self.size, segments);

        // Projection = the exact leaf set the plan fetched (never reads a
        // chunk we did not download).
        let leaves: BTreeSet<usize> = plan.ranges.iter().map(|r| r.leaf_column).collect();
        let schema_descr = self.parquet_meta.file_metadata().schema_descr();
        let projection = ProjectionMask::leaves(schema_descr, leaves.iter().copied());

        let limit_opt = (limit != 0).then_some(limit as usize);
        let batches = execute_pushdown(
            reader,
            self.arrow_meta.clone(),
            &self.geo,
            &self.geometry_column,
            bbox,
            &filters,
            plan.row_groups.clone(),
            Some(projection),
            limit_opt,
        )
        .map_err(GpqLiveError::from)?;

        let output = record_batches_to_geojson(&batches, &self.geometry_column, AREA_COLUMN)?;

        // Commit freshly fetched chunks to the cache (with eviction).
        for (key, data) in fetched {
            self.cache_insert(key, data);
        }

        let bytes_this_query = fetch::bytes_fetched_total().saturating_sub(bytes_before);
        let requests_this_query = fetch::request_count_total().saturating_sub(requests_before);
        let elapsed_ms = js_sys::Date::now() - start_ms;

        let obj = Object::new();
        set(&obj, "geojson", &JsValue::from_str(&output.geojson));
        set(&obj, "matched", &JsValue::from_f64(output.matched as f64));
        set(
            &obj,
            "totalAreaM2",
            &JsValue::from_f64(output.total_area_m2),
        );
        set(
            &obj,
            "rowGroupsScanned",
            &JsValue::from_f64(plan.row_groups.len() as f64),
        );
        set(
            &obj,
            "rowGroupsTotal",
            &JsValue::from_f64(plan.total_row_groups as f64),
        );
        set(&obj, "survivors", &survivors_array(&plan.row_groups));
        set(
            &obj,
            "bytesFetchedThisQuery",
            &JsValue::from_f64(bytes_this_query as f64),
        );
        set(
            &obj,
            "requestsThisQuery",
            &JsValue::from_f64(requests_this_query as f64),
        );
        set(&obj, "elapsedMs", &JsValue::from_f64(elapsed_ms));
        Ok(obj.into())
    }

    /// Cumulative session accounting: `{bytesFetchedTotal, requestsTotal,
    /// cacheBytes}`.
    #[must_use]
    pub fn stats(&self) -> JsValue {
        let obj = Object::new();
        set(
            &obj,
            "bytesFetchedTotal",
            &JsValue::from_f64(fetch::bytes_fetched_total() as f64),
        );
        set(
            &obj,
            "requestsTotal",
            &JsValue::from_f64(fetch::request_count_total() as f64),
        );
        set(
            &obj,
            "cacheBytes",
            &JsValue::from_f64(self.cache_bytes as f64),
        );
        obj.into()
    }
}

// ── Private helpers (kept out of the `#[wasm_bindgen]` impl) ──────────────────

impl RemoteGeoParquet {
    /// Decode footer bytes into fully-formed metadata and construct the session.
    fn finish_open(
        url: String,
        size: u64,
        footer_len: u32,
        footer: &[u8],
    ) -> Result<RemoteGeoParquet, GpqLiveError> {
        let parquet_meta = Arc::new(ParquetMetaDataReader::decode_metadata(footer)?);
        let arrow_meta = ArrowReaderMetadata::try_new(parquet_meta.clone(), Default::default())?;
        let geo_json =
            extract_geoparquet_metadata(arrow_meta.schema().as_ref())?.ok_or_else(|| {
                GpqLiveError::Parquet(ParquetError::General(
                    "missing GeoParquet 'geo' metadata".into(),
                ))
            })?;
        let geo = GeoParquetMetadata::from_json(&geo_json)?;
        let geometry_column = geo.primary_column.clone();

        Ok(RemoteGeoParquet {
            url,
            size,
            footer_len,
            arrow_meta,
            parquet_meta,
            geo,
            geometry_column,
            chunk_cache: BTreeMap::new(),
            cache_bytes: 0,
        })
    }

    /// The output columns fed to the planner: always geometry + area, plus any
    /// JS-requested string column names.
    fn output_columns(&self, columns: &JsValue) -> Vec<String> {
        let mut names: BTreeSet<String> = BTreeSet::new();
        names.insert(self.geometry_column.clone());
        names.insert(AREA_COLUMN.to_string());
        if let Ok(arr) = columns.clone().dyn_into::<Array>() {
            for value in arr.iter() {
                if let Some(name) = value.as_string()
                    && !name.is_empty()
                {
                    names.insert(name);
                }
            }
        }
        names.into_iter().collect()
    }

    /// Chunk ranges from `ranges` that are not already cached.
    fn missing_ranges(&self, ranges: &[ColumnChunkRange]) -> Vec<ChunkRange> {
        ranges
            .iter()
            .filter(|r| !self.chunk_cache.contains_key(&(r.row_group, r.leaf_column)))
            .map(to_chunk_range)
            .collect()
    }

    /// Insert a fetched chunk into the cache, evicting until under the cap.
    fn cache_insert(&mut self, key: (usize, usize), data: Bytes) {
        let added = data.len();
        if let Some(old) = self.chunk_cache.insert(key, data) {
            self.cache_bytes = self.cache_bytes.saturating_sub(old.len());
        }
        self.cache_bytes += added;
        // Evict other entries until back under the cap (never the just-added key).
        while self.cache_bytes > CHUNK_CACHE_CAP {
            let victim = self.chunk_cache.keys().find(|k| **k != key).copied();
            match victim {
                Some(v) => {
                    if let Some(removed) = self.chunk_cache.remove(&v) {
                        self.cache_bytes = self.cache_bytes.saturating_sub(removed.len());
                    }
                }
                None => break,
            }
        }
    }
}

/// Set a property on a plain JS object, ignoring the (never-failing) result.
fn set(obj: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), value);
}

/// Build a `Uint32Array` of surviving row-group indices.
fn survivors_array(row_groups: &[usize]) -> JsValue {
    let survivors: Vec<u32> = row_groups.iter().map(|&r| r as u32).collect();
    Uint32Array::from(&survivors[..]).into()
}

/// Map a planner [`ColumnChunkRange`] to a coalescer [`ChunkRange`].
fn to_chunk_range(range: &ColumnChunkRange) -> ChunkRange {
    ChunkRange {
        row_group: range.row_group,
        leaf_column: range.leaf_column,
        start: range.start,
        len: range.length,
    }
}

/// Parse an optional `WHERE`-fragment into attribute filters.
///
/// `None`, empty, or whitespace-only input yields no filters.
fn parse_filters(filter_expr: Option<&str>) -> Result<Vec<AttributeFilter>, GpqLiveError> {
    match filter_expr {
        Some(expr) if !expr.trim().is_empty() => {
            parse_filter_expr(expr).map_err(|e| GpqLiveError::FilterExpr { msg: e.to_string() })
        }
        _ => Ok(Vec::new()),
    }
}
