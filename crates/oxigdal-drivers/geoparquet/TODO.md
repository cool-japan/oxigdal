# TODO: oxigdal-drivers/geoparquet

> **Purpose:** GeoParquet driver for OxiGDAL - Pure Rust GDAL reimplementation
> **Status (2026-05-16):** 11,925 Rust LoC (incl. tests) - 181 tests - 0 source-code stubs (mature crate; 0.1.5 already shipped covering bbox + native encodings + statistics)
> **Roadmap:** v0.1.7 (current slice) - v0.2.0 - v1.0.0

## High Priority (next slice - verified gaps)

- [ ] Implement column projection (read only selected columns)
  - **Verified gap:** `GeoParquetReader` (re-exported from `src/reader.rs` per `src/lib.rs:110`) has `with_bbox_filter` and `with_attribute_filter` (per MEMORY.md and audit of `src/reader.rs`), but no `with_columns(&[&str])` projection mask method. `rg -n "fn with_columns|projection|ProjectionMask" -g '*.rs' src/reader.rs` shows internal use of `ProjectionMask` only inside the bbox fast-path; not user-exposed.
  - **Goal:** `reader.with_columns(["population", "geometry"])` skips decoding all other columns at row-group level - significant speedup on wide tables.
  - **Design:** Build `parquet::arrow::ProjectionMask::columns(reader.metadata().schema(), names)`; pass to `ArrowReaderBuilder::with_projection`. Validate names exist in schema; error on unknown column. Geometry column must always be included (or explicitly opted out via `with_columns_no_geometry`).
  - **Files:** `src/reader.rs`, `src/lib.rs` (re-export new methods)
  - **Tests:** (proposed) `test_projection_excludes_unselected_columns`, `test_projection_includes_geometry_implicitly`, `test_projection_unknown_column_errors`, `test_projection_combined_with_bbox_filter`
  - **Risk:** Interaction with attribute filter - the filter columns must also be in projection mask; auto-include.
  - **Prerequisites:** None.

- [ ] Parallel row-group reading with rayon
  - **Verified gap:** `Cargo.toml` does not declare `rayon` dep. `rg -n "rayon|par_iter|parallel" -g '*.rs' src/` returns no matches. Current reader processes row groups sequentially via `ArrowReaderBuilder` iterator.
  - **Goal:** Reading a file with 100 row groups across 8 cores completes in ~1/8 wall time (modulo I/O).
  - **Design:** Survivor row-group indices from bbox/attribute pruning (already implemented per MEMORY.md and `src/reader.rs:235-252`) drive parallel decode. Use `rayon::iter::IntoParallelIterator` over the survivor list; each thread opens an independent `ArrowReaderBuilder` reading its row group only (Parquet allows this via `with_row_groups([i])`). Merge results into ordered `Vec<RecordBatch>`.
  - **Files:** `src/reader.rs`, `Cargo.toml` (add `rayon.workspace = true` under a `parallel` feature)
  - **Tests:** (proposed) `test_parallel_read_matches_sequential`, `test_parallel_read_with_bbox_filter`, `test_parallel_read_preserves_order`
  - **Risk:** File handle sharing - parquet readers need independent handles; open per thread. May not be available with non-Send underlying I/O.
  - **Prerequisites:** None.

- [ ] GeoParquet metadata validation against the GeoParquet 1.1 specification
  - **Verified gap:** `src/metadata.rs` has a `validate()` (per `src/metadata.rs:178-186` referenced in MEMORY.md) but full spec validation is partial - `rg -n "fn validate" -g '*.rs' src/metadata.rs` shows validation of encoding type matrix only. No structural validation of required fields per GeoParquet 1.1 §2 (`version`, `primary_column`, `columns.{column_name}.encoding`, `columns.{column_name}.geometry_types`).
  - **Goal:** Open a `.parquet` claiming to be GeoParquet, return a structured `ValidationReport` listing missing required fields, type mismatches, and recommended-but-missing fields.
  - **Design:** Walk the `geo` key in Parquet file-level metadata. Required: `version` (string, semver), `primary_column` (string), `columns` (object). For each column: `encoding` (one of `wkb` / `point` / `linestring` / ...), `geometry_types` (array of WKT type strings), and recommended `crs`, `edges`, `orientation`, `bbox`, `epoch`. Per GeoParquet 1.1 spec at <https://geoparquet.org/releases/v1.1.0/>.
  - **Files:** (new) `src/validation.rs`, `src/lib.rs`, `src/metadata.rs` (link)
  - **Tests:** (proposed) `test_validate_minimal_geoparquet_passes`, `test_validate_missing_primary_column_fails`, `test_validate_unknown_encoding_fails`, `test_validate_invalid_semver_version_fails`, `test_validate_reports_recommended_missing_as_warning`
  - **Risk:** Spec interpretation ambiguities; cite spec section per check.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] Spatial-partitioning writer (Hilbert curve / geohash row-group layout)
  - **Goal:** Sort features by Hilbert key before write so row-group bboxes are tight, enabling more pruning on read.
  - **Files:** `src/writer.rs`, `src/partitioning.rs` (20.6K - already has scaffolding)
  - **Why deferred:** Big design with sort cost trade-off.

- [ ] CRS transformation on read/write via `oxigdal-proj`
  - **Goal:** Mirror `oxigdal-drivers/geojson` reprojection feature for parquet.
  - **Files:** (new) `src/reproject.rs`
  - **Why deferred:** Cross-cutting; tie to a workspace-wide reprojection helper.

- [ ] Streaming Parquet writer for unbounded feature streams
  - **Goal:** Write row groups incrementally as features arrive; useful for very large inputs.
  - **Files:** `src/writer.rs`
  - **Why deferred:** Existing writer batches all features first.

- [ ] Multi-geometry-column file support (per GeoParquet 1.1 §2.5)
  - **Goal:** Spec allows multiple geometry columns; we only handle a single primary.
  - **Files:** `src/metadata.rs`, `src/reader.rs`, `src/writer.rs`
  - **Why deferred:** Rare; ship after main features stable.

- [ ] Schema evolution (add / remove columns without full rewrite)
  - **Goal:** Append-only ALTER TABLE semantics on existing Parquet.
  - **Files:** (new) `src/evolution.rs`
  - **Why deferred:** Complex; rare ask.

- [ ] Delta Lake / Iceberg integration for versioned geospatial tables
  - **Goal:** Read tables that combine GeoParquet with Delta or Iceberg transaction logs.
  - **Files:** (new) `src/lakehouse/` module
  - **Why deferred:** Large; separate ecosystem dependency.

- [ ] Row-group compaction / optimization tool
  - **Goal:** Re-pack many small row groups into fewer larger ones.
  - **Files:** (new) `src/compact.rs`
  - **Why deferred:** Tool; not core driver.

## Low Priority / Future (speculative - concise)

- [ ] GeoArrow native zero-copy geometry array integration (eager decode already lands in 0.1.5; zero-copy is the v0.1.6 follow-up)
- [ ] Partitioned dataset reading (directory of `.parquet` files as one logical dataset)
- [ ] Cloud-native reading via object store (S3 / GCS / Azure Blob) - via `object_store` crate or equivalent
- [ ] DuckDB spatial extension bridge
- [ ] Geometry column statistics (centroid, bbox, hull) in file-level metadata
- [ ] Nested struct / list column support for complex properties
- [ ] File merge with spatial re-partitioning
- [ ] Mapping to / from PostGIS WKB encoding nuances

## Cross-crate dependencies
- **Blocks:** None directly.
- **Blocked by:** `oxigdal-proj` (for reprojection feature only).

## Recently completed (kept verbatim from previous TODO.md)
- [x] GeoParquet 1.1 row-group pruning + attribute predicate pushdown + covering.bbox column fast-path (done 2026-05-07)
  - **Goal:** Push spatial AND attribute filters into Parquet's row-group skipping path; on a 100M-row file with bbox query touching 5% of row groups, decode <5% of rows. Honour GeoParquet 1.1 `covering.bbox` columns when present so spatial bbox queries skip WKB decode entirely.
  - **Design:**
    1. Row-group statistics-based pruning: parse GeoParquet `geo` metadata; for each row group fetch covering.bbox column stats; compute row-group bbox (min_xmin, min_ymin, max_xmax, max_ymax); intersect with query bbox; pass survivor indices to `with_row_groups()`.
    2. Column-level predicate pushdown: `AttributeFilter` enum (`Eq(col, Scalar)`, `Range { col, lo, hi }`, `In(col, Vec<Scalar>)`); compile to `parquet::arrow::arrow_reader::ArrowPredicate`; feed `RowFilter::new(...)` into builder.
    3. covering.bbox fast-path: project ONLY xmin/ymin/xmax/ymax columns for bbox-only queries; ArrowPredicate via arrow::compute kernels skips WKB decode; decode WKB only for surviving rows.
    4. Spec-shape detection: detect both 1.1-spec shapes — (a) struct column `xmin/ymin/xmax/ymax`, (b) flat columns `<geomcol>_bbox_xmin` etc. — normalised via `BboxColumns` accessor.
    - Covers also: "Implement row group-level spatial filtering using bounding box metadata", "Add predicate pushdown for attribute filters", "Implement GeoParquet 1.1 covering column support (bbox columns)"
  - **Files:**
    - `crates/oxigdal-drivers/geoparquet/src/reader.rs` (extend with row-group survivor path)
    - `crates/oxigdal-drivers/geoparquet/src/covering.rs` (new — BboxColumns accessor + fast-path)
    - `crates/oxigdal-drivers/geoparquet/src/predicate.rs` (new — AttributeFilter + to_arrow_predicate)
    - `crates/oxigdal-drivers/geoparquet/src/lib.rs` (re-export AttributeFilter, BboxColumns)
  - **Tests:** test_row_group_pruning_disjoint_bbox, test_row_group_pruning_partial_overlap, test_predicate_pushdown_eq, test_predicate_pushdown_range, test_covering_bbox_struct_shape, test_covering_bbox_flat_columns_shape, test_no_covering_bbox_falls_back_to_wkb_bbox, test_predicate_combined_with_bbox
  - **Risk:** RowFilter predicates must reference projected columns only — compose via PredicateBuilder that owns projection mask.
- [x] Add native (GeoArrow) geometry encoding support — Point/LineString/Polygon arrays (done 2026-05-08)
  - **Goal:** Round-trip support for GeoParquet 1.1 native (GeoArrow) encoding for Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon — both reads and writes — alongside existing WKB. Default writer encoding stays WKB for back-compat; native is opt-in via builder.
  - **Design:** Implement from scratch using `arrow::array` primitives (no `geoarrow-rs` dep — Pure-Rust default). ~600 LoC budget. Three structural prereqs (all in this item per IMPLEMENT POLICY): (1) Extend `EncodingType` at `metadata.rs:222-227` from closed `{Wkb}` to add Point/LineString/Polygon/MultiPoint/MultiLineString/MultiPolygon; update `validate()` matrix at `metadata.rs:178-186`; add `GeometryColumnMetadata::new_native(EncodingType)`. (2) Bump `GEOPARQUET_VERSION` from `"1.0.0"` to `"1.1.0"` at `metadata.rs:14`; update test assertion at `lib.rs:117`. (3) Replace `GEOPARQUET_EXTENSION_NAME` constant at `arrow_ext/schema.rs:12` with `fn geoarrow_extension_name(EncodingType) -> &'static str` returning `"geoarrow.wkb"`/`"geoarrow.point"`/etc; add `create_geometry_field_for(name, encoding, dim, nullable)`. **GeoArrow shapes:** Point = `FixedSizeList<f64, N>` N∈{2,3,4} for xy/xyz/xym/xyzm interleaved; LineString = `List<Point>`; Polygon = `List<List<Point>>` (rings: exterior first); MultiPoint = `List<Point>`; MultiLineString = `List<List<Point>>`; MultiPolygon = `List<List<List<Point>>>`. **Coord-dim signaling:** field-level `ARROW:extension:metadata` JSON `{"crs":..., "edges":..., "coord_type":"interleaved"}`; fallback to FixedSizeList arity. **Read:** eager decode native to `Vec<Geometry>` at API boundary (uniform downstream behaviour). **Write:** default WKB; opt-in via `GeoParquetWriterBuilder::encoding(EncodingType)` and `coord_dim(CoordDim)`; no auto-detect. **`covering.bbox` interaction:** existing `read_pushdown` path works with native if `wkb_bbox_mask` (at `reader.rs:616-645`) gets a `native_bbox_mask` parallel; explicit gating at `reader.rs:245-252`. Mixed geometry types in native column → reject (spec forbids; only WKB allows mixing).
  - **Files:** New `crates/oxigdal-drivers/geoparquet/src/geometry/native.rs` (~600 LoC); modify `geometry/mod.rs`, `metadata.rs` (extend enum + version bump + validate + CoordDim + new_native), `arrow_ext/schema.rs` (function instead of const), `arrow_ext/mod.rs`, `reader.rs` (encoding dispatch in read_geometries/spatial_row_mask/read_pushdown gating), `writer.rs` (encoding selection + native flush_batch path), `lib.rs` (re-exports + bumped test assertion); new `tests/native_encoding_tests.rs`.
  - **Prerequisites:** All folded into this item per IMPLEMENT POLICY (no new workspace deps; arrow-array/buffer/schema already at workspace versions).
  - **Tests:** test_native_point_roundtrip_2d, test_native_point_roundtrip_xyz, test_native_linestring_roundtrip, test_native_polygon_with_holes_roundtrip, test_native_multipolygon_roundtrip, test_native_mixed_types_rejected_by_validate, test_wkb_writer_default_unchanged (back-compat regression), test_native_with_covering_bbox_pushdown. Plus 4 metadata unit tests for serde round-trip of `EncodingType::Point` etc.
  - **Risk:** GeoArrow spec drift — pin to specific spec revision in module docstring with citation https://geoarrow.org/format.html. Reader compat — older oxigdal will fail to load 1.1 native files; mitigation: WKB default + explicit opt-in. Read perf regression — eager decode slower than zero-copy WKB; accept for v0.1.5; flag lazy iterator for v0.1.6. Covering-bbox + native interaction — easy to forget the gate; lock with test_native_with_covering_bbox_pushdown.
- [x] Add Parquet statistics exposure (done 2026-05-08)
  - **Goal:** Expose per-column, per-row-group statistics (min, max, null_count, distinct_count) from underlying Parquet metadata via public `GeoParquetReader` API. Enable user-side pre-scan filtering / analytics without re-reading.
  - **Design:** Parquet stats are in `parquet::file::statistics::Statistics` accessible through `RowGroupMetaData::columns()` (already used by covering-bbox pushdown at `reader.rs:235-252`). New struct `ColumnStatistics { name: String, parquet_type: String, min: ScalarValue, max: ScalarValue, null_count: u64, distinct_count: Option<u64> }` reusing `ScalarValue` from existing `predicate.rs`. APIs: `GeoParquetReader::row_group_statistics(&self) -> Vec<Vec<ColumnStatistics>>` (outer = row group, inner = column; cached behind `OnceCell`); `column_statistics(&self, col_name: &str) -> Option<Vec<ColumnStatistics>>` (across all row groups for one column). Geometry column: return None unless `covering.bbox` columns exist (WKB blob has no meaningful min/max). Stats may be absent for some columns (writer-dependent); never panic, return None cleanly.
  - **Files:** New `crates/oxigdal-drivers/geoparquet/src/statistics.rs` (~300 LoC); modify `lib.rs` (`pub mod statistics; pub use statistics::ColumnStatistics`); modify `reader.rs` (add `row_group_statistics`, `column_statistics`).
  - **Prerequisites:** None — `ScalarValue` already exists in `predicate.rs`.
  - **Tests:** test_stats_int64_min_max, test_stats_string_min_max, test_stats_null_counts_per_row_group, test_stats_missing_returns_none, test_stats_by_column_name, test_stats_geometry_column_returns_none_without_bbox_columns.
  - **Risk:** Logical-type-to-`ScalarValue` mapping — handle top 6 (Int32/Int64/Float/Double/ByteArray/Bool) plus Decimal; mark unsupported as `ScalarValue::Other(String)` with debug output rather than panic.
- [x] GeoParquet WKB reader: nested GeometryCollection + Z/M/ZM variants (planned 2026-04-18)
  - **Goal:** `WkbReader::read_geometry` decodes all 3000-series WKB type codes: Z variants (1001–1007), M variants (2001–2007), ZM variants (3001–3007), plus recursive GeometryCollection (type 7).
  - **Design:** Extend dispatch match; add Z/M/ZM decoder helpers; recursive GeometryCollection with depth guard (max 64).
  - **Files:** geometry/wkb.rs (depth guard + has_z/has_m), geometry/types.rs (Geometry::has_z/has_m methods), geometry/wkb_extended.rs (wkb_bbox stride fix for M/ZM)
  - **Tests:** 6 tests covering PointZ, PointZM, flat collection, recursive collection, depth guard, MultiPolygonZM

---
*Last audited: 2026-05-16*
