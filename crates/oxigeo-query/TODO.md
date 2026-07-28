# TODO: oxigeo-query

> **Purpose:** SQL-like query language + cost-based optimizer + parallel executor for geospatial data; sqlparser-based parser, custom AST, rstar index integration.
> **Status (2026-07-28):** 10,398 LoC (src) · 185 tests · 0 critical eval gaps (spatial predicates now evaluate real geometry; index-driven scan wiring and GROUP BY spatial aggregates remain open — see below)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Implement `Expr::Function` evaluation in the filter executor (currently unreachable for any spatial WHERE clause).
  - **Verified gap:** `src/executor/filter.rs:144-154` —
    ```rust
    _ => Err(QueryError::unsupported(
        OxiGeoError::not_supported_builder("Unsupported expression type in filter")
            .with_operation("filter_evaluation")
            .with_parameter("expression_type", format!("{:?}", expr))
            .with_suggestion(
                "Use simpler expressions: columns, literals, binary/unary operators, IS [NOT] NULL",
            )
            .build()
            .to_string(),
    )),
    ```
    `Expr::Function { name, args }` is recognised by the parser (`parser/sql.rs`) and by the index selector (`index/selector.rs:84-110` matches ST_INTERSECTS / ST_CONTAINS / ST_WITHIN) but **not by the filter evaluator**. Any query `WHERE ST_Intersects(geom, ?)` will plan, then fail at runtime with "Unsupported expression type".
  - **Goal:** Filter evaluator supports the SQL/MM Part 3 spatial predicate set: `ST_INTERSECTS`, `ST_CONTAINS`, `ST_WITHIN`, `ST_DISJOINT`, `ST_EQUALS`, `ST_TOUCHES`, `ST_OVERLAPS`, `ST_CROSSES`, `ST_DISTANCE`, `ST_BUFFER`, `ST_INTERSECTION`, `ST_UNION`, `ST_AREA`, `ST_LENGTH`, `ST_CENTROID`.
  - **Design:** Add `Expr::Function { name, args }` arm in `evaluate_expr`. Dispatch by uppercase function name to handlers that materialise arguments to `geo_types::Geometry` (already a dep) and call `geo` crate predicates (`geo::Intersects`, `geo::Contains`, `geo::EuclideanDistance`). For binary predicates returning bool, return `Value::Boolean`; for unary metrics return `Value::Float64`. Argument types validated; type mismatch → `QueryError`. Function registry pattern (`HashMap<&'static str, fn(&[Value]) -> Result<Value>>`) keeps the match table small.
  - **Files:** `src/executor/filter.rs:144` (replace catch-all); new `src/executor/spatial_funcs.rs` (~400 LoC of dispatcher + handlers); `src/parser/ast.rs` (verify `Expr::Function` field names).
  - **Tests:** (proposed) `test_filter_st_intersects_returns_matching_rows`, `test_filter_st_contains_polygon_point`, `test_filter_st_distance_within_predicate`, `test_filter_st_within_excludes_outside`, `test_filter_unknown_function_returns_error_with_name`, `test_filter_st_func_with_wrong_arity_errors`.
  - **Risk:** WKT vs WKB literal handling — `Literal::String("POINT(1 2)")` needs parsing into geometry; reuse `wkt` crate (already in `oxigeo-db-connectors` deps; promote to workspace).
  - **Prerequisites:** None.
  - **Done:** 2026-05-20 (Slice 24). Closes catch-all at `executor/filter.rs:144` with `Expr::Function { name, args }` arm + dispatch into `executor::spatial_funcs::evaluate_spatial_function`. Added 19 SQL/MM Part 3 functions: predicates (ST_INTERSECTS / ST_CONTAINS / ST_WITHIN / ST_DISJOINT / ST_EQUALS / ST_TOUCHES / ST_OVERLAPS / ST_CROSSES / ST_COVERS / ST_COVEREDBY), distance/metric (ST_DISTANCE / ST_DWITHIN / ST_AREA / ST_LENGTH), constructors (ST_CENTROID / ST_ENVELOPE / ST_BUFFER), and set-ops (ST_INTERSECTION / ST_UNION / ST_DIFFERENCE). Added `Value::Geometry(geo::Geometry<f64>)` runtime variant in `executor/filter.rs` (no exhaustive matches existed; safe to add). WKT literals (`Value::String`) parsed via `wkt = "0.14"` (added as workspace dep). BooleanOps falls back to `QueryError::Unsupported(...)` for operand types geo 0.33 doesn't implement (e.g., LineString set-ops).
  - **Tests:** 20 (4 inline unit + 16 in `crates/oxigeo-query/tests/spatial_funcs_test.rs`) — covers every predicate + metric + constructor + error path (unknown name, wrong arity, type mismatch) + end-to-end executor integration.

- [ ] Index-driven spatial scan: connect `IndexSelection { usage: IndexUsage::Spatial }` to actual rstar scan.
  - **Verified gap:** `src/index/selector.rs:84-110` correctly detects when a spatial index applies (returns `IndexSelection` with `selectivity: 0.01`) — but the executor in `src/executor/scan.rs` does not consult `IndexSelection`; every scan is a full table scan. The selectivity is purely cost-model.
  - **Goal:** When the optimiser picks `IndexUsage::Spatial`, the executor performs an rstar bbox lookup (`RTree::locate_in_envelope_intersecting`) and only evaluates the predicate on the returned candidates.
  - **Design:** Extend `DataSource` trait with `fn supports_spatial_index(&self) -> bool` and `fn spatial_index_scan(&self, envelope: AABB) -> Pin<Box<dyn Stream<Item = RecordBatch>>>`. Optimiser produces an `IndexedScan { table, envelope, residual_predicate }` node; executor dispatches.
  - **Files:** `src/executor/scan.rs`, `src/executor/mod.rs`, `src/optimizer/rules/mod.rs`, `src/optimizer/cost_model.rs`.
  - **Tests:** (proposed) `test_indexed_scan_only_visits_candidates`, `test_residual_predicate_filters_after_index`, `test_no_index_falls_back_to_full_scan`, `test_index_selectivity_estimate_within_2x_of_actual`.
  - **Risk:** rstar bbox computation requires the column to expose its envelope cheaply; add `ColumnData::Geometry { bbox_cache: Vec<AABB> }`.
  - **Prerequisites:** Item 1 (so ST_INTERSECTS evaluates correctly post-index).

- [ ] Spatial aggregates (`ST_Union`, `ST_Collect`, `ST_Extent`) in `GROUP BY`.
  - **Goal:** `SELECT ST_Union(geom) FROM streets GROUP BY road_class` returns one merged geometry per group.
  - **Design:** Extend `src/executor/aggregate.rs:273 LoC` aggregator enum with `Union`, `Collect`, `Extent`. Implementations: `Union` calls `geo::BooleanOps::union` pairwise (O(n) per group); `Collect` builds `GeometryCollection`; `Extent` keeps running min/max corners.
  - **Files:** `src/executor/aggregate.rs`, `src/parser/sql.rs` (recognise aggregate functions in projection).
  - **Tests:** (proposed) `test_st_union_two_polygons`, `test_st_collect_preserves_inputs`, `test_st_extent_envelope_correctness`, `test_group_by_with_spatial_aggregate`.
  - **Risk:** Pairwise `union` is O(n²) worst case; document and provide `STR-tree`-based bulk union later.
  - **Prerequisites:** Item 1.

## Medium Priority
- [ ] OGC CQL2 (OGC 21-065) parser alongside SQL.
  - **Goal:** Accept CQL2 expressions as alternative input to `parse_sql`; useful for OGC API Features integration.
  - **Files:** New `src/parser/cql2.rs`. Sibling crate `oxigeo-services` already has a CQL subset (`ogc_features/cql.rs`); consider extracting.
  - **Why deferred:** SQL covers v0.1.5 needs; CQL2 is API-surface convenience.

- [x] Window functions: `ROW_NUMBER`, `RANK`, `LAG`, `LEAD` (SQL:2003).
  - **Files:** New `src/executor/window.rs`.
  - **Done:** 2026-05-22 (Slice 25). New `src/executor/window.rs` (~741 LoC) — self-contained evaluation kernel, NOT wired into the SQL parser/planner (the kernel + programmatic API are this slice's deliverable; parser/planner wiring is Slice 26 follow-up). Implements ROW_NUMBER, RANK (competition: ties share rank with gap), DENSE_RANK, LAG/LEAD (offset + default; out-of-range → default or `Value::Null`), plus FIRST_VALUE/LAST_VALUE/NTH_VALUE. `WindowFunction` enum, `WindowSpec { partition_by, order_by }`, `OrderKey { column, ascending }`. Algorithm: stable partition by first-seen order; stable sort each partition by ORDER BY using a comparator that matches `sort.rs` semantics (NULLs last in ascending, cross-width numeric promotion as in `filter.rs`); scatter results back to input order. Operates on `crate::executor::filter::Value` with NO exhaustive matches (all use `_ =>` arms — forward-compatible with `Value::Geometry` added by Slice 24). Integer outputs use `Value::Int64`. `evaluate_window(num_rows, value_at_closure, ...)` core + `evaluate_window_batch(RecordBatch, ...)` convenience. `executor/mod.rs` +3 additive lines (`pub mod window;` + re-export). Parser/planner untouched.
  - **Tests:** 17 integration in `crates/oxigeo-query/tests/window_test.rs` + 3 inline unit. Coverage: row_number sequential/reset-per-partition, rank with gap, dense_rank no-gap, lag prev/first-default/offset-2, lead next/last-null-default, partition-by-two-keys, order-by-desc, empty-input, first/last/nth, no-ORDER-BY all-tie, RecordBatch wrapper, out-of-bounds-column error, NULLs-last ordering.

- [ ] `INSERT` / `UPDATE` / `DELETE` for mutable `DataSource` (currently read-only).
  - **Files:** `src/executor/mod.rs`, `DataSource` trait.

- [ ] Query plan visualisation (DOT graph export).
  - **Files:** `src/explain.rs:294 LoC` (extend `ExplainPlan::to_dot()`).

- [ ] Cost-model calibration from execution statistics (feedback loop).
  - **Files:** `src/optimizer/cost_model.rs:417 LoC`.

- [ ] Prepared statements with parameter binding (`?N` placeholders).
  - **Files:** `src/parser/sql.rs`, `src/executor/mod.rs`.

- [ ] CTE (`WITH`) and subquery support.
  - **Files:** `src/parser/sql.rs`, `src/parser/ast.rs`.

- [ ] Streaming cursors for `LIMIT`/`OFFSET` over large result sets.
  - **Files:** `src/executor/mod.rs` (currently materialises full result).

- [ ] `EXPLAIN ANALYZE` with real timing capture.

## Low Priority / Future (one-liners)
- [ ] Distributed query execution across multiple nodes (`oxigeo-cluster` integration).
- [ ] Cross-source federated queries (PostGIS ∪ GeoParquet ∪ STAC).
- [ ] Materialised view caching with invalidation.
- [ ] Adaptive optimisation from query history (learn cost-model coefficients).
- [ ] GeoJSON / GeoParquet output sink for `SELECT INTO`.
- [ ] User-defined function registration.
- [ ] Catalog-managed statistics (histograms) for selectivity.

## Cross-crate dependencies
- **Blocks:** `oxigeo-services` (when WFS GetFeature dispatches via query engine).
- **Blocked by:** `oxigeo-index` (provides rstar wrappers), `oxigeo-postgis` (provides PostGIS `DataSource`).

## Recently completed (verbatim)
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-07-28*
