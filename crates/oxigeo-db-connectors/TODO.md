# TODO: oxigeo-db-connectors

> **Purpose:** Multi-backend spatial database connectors — MySQL/MariaDB, SQLite (pure-Rust, no SpatiaLite), MongoDB, ClickHouse, TimescaleDB, Cassandra/ScyllaDB — sharing a unified `DatabaseConnector` trait.
> **Status (2026-07-28):** 4,522 LoC (src) · 53 tests all-features / 36 default-features · 0 real-code stubs; many partial-coverage modules
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Flesh out the unified `DatabaseConnector` trait beyond `health_check / version / list_tables`.
  - **Verified gap:** `src/lib.rs:58-69` — `pub trait DatabaseConnector` declares only three methods (`health_check`, `version`, `list_tables`) and the doc comment marks it `(for future unified interface)`. Each connector currently exposes its own concrete API (e.g., `MySqlConnector`, `MongoDbConnector`, `SqliteConnector`) with non-uniform method names (`read_within`/`read_intersects`/`aggregate_near` etc.).
  - **Goal:** Trait suitable for `Arc<dyn DatabaseConnector>` polymorphism: `async fn read_bbox(&self, table: &str, bbox: BoundingBox, limit: Option<usize>) -> Result<Stream<Feature>>`, `async fn read_intersects(&self, table: &str, geom: &Geometry) -> Result<Stream<Feature>>`, `async fn write_batch(&self, table: &str, batch: &[Feature]) -> Result<usize>`, `async fn count(&self, table: &str, predicate: Option<&Predicate>) -> Result<i64>`.
  - **Design:** Define associated type `type Stream: Stream<Item = Result<Feature>>` to handle differing backend semantics. `Predicate` is a small enum (`Bbox(BoundingBox)`, `Intersects(Geometry)`, `Within(Geometry)`, `DWithin(Geometry, f64)`, `And(Vec<Predicate>)`, `Or(Vec<Predicate>)`). Each backend lowers predicates to native SQL/MQL/CQL — e.g., `Predicate::Intersects(g)` → MySQL `ST_Intersects(geom, ST_GeomFromText(?))`, MongoDB `{$geoIntersects: {$geometry: <GeoJSON>}}`, ClickHouse `pointInPolygon`.
  - **Files:** `src/lib.rs:58-69` (extend trait), each `src/{mysql,sqlite,mongodb,clickhouse,timescale,cassandra}/mod.rs` (impl trait).
  - **Tests:** (proposed) `test_connector_trait_dyn_dispatch`, `test_predicate_lowered_to_mysql_st_intersects`, `test_predicate_lowered_to_mongodb_geowithin`, `test_predicate_lowered_to_clickhouse_pointinpolygon`, `test_write_batch_returns_inserted_count_each_backend`.
  - **Risk:** Forced-into-one-trait API may lose backend-specific features; document `into_inner()` escape hatches.
  - **Prerequisites:** None.

- [ ] Unified connection pooling across backends (currently each backend's own pool API).
  - **Verified gap:** `src/connection/pool.rs` is only 110 LoC; the heavy lifting is delegated to each backend's vendor pool (`deadpool-postgres` for TimescaleDB, `mysql_async`'s built-in pool, MongoDB's connection pool). No common `PoolConfig` is enforced.
  - **Goal:** `PoolConfig { min, max, idle_timeout, acquire_timeout, health_check_interval }` honoured by all backends; metrics exposed (active/idle/wait_queue_depth) via single API.
  - **Design:** Each backend wraps its native pool with `oxigeo-db-connectors::connection::PoolAdapter` implementing `async fn acquire(&self) -> Result<Conn>` and `fn stats(&self) -> PoolStats`. ClickHouse has no native pool — use `bb8` (already in deps).
  - **Files:** `src/connection/pool.rs` (extend), each `src/<backend>/mod.rs` (rewire).
  - **Tests:** (proposed) `test_pool_acquire_respects_max_size`, `test_pool_idle_timeout_evicts_connection`, `test_pool_stats_active_in_use_count`.
  - **Risk:** Some backends (Cassandra/Scylla) handle pooling internally and resist external wrapping; expose `stats_only` mode in those cases.
  - **Prerequisites:** None.

- [ ] ~~SpatiaLite extension auto-load with multiple search paths~~ — **SUPERSEDED, not applicable.**
  - **What changed:** the `sqlite` backend was migrated off `rusqlite` entirely onto the pure-Rust `oxisql-sqlite-compat` (limbo engine) — verified via `Cargo.toml:25` (`sqlite = ["dep:oxisql-sqlite-compat", "dep:oxisql-core"]`, no `rusqlite` anywhere in the manifest) and `src/sqlite/mod.rs:1-4,21-22,124-138`, which now documents `spatialite: bool` as "no-op in pure-Rust mode — always false" and `has_spatialite()` as "Always returns `false` in pure-Rust mode (no SpatiaLite extension support)".
  - **Why it's not just "done":** the original goal (dynamically load the `mod_spatialite` C shared-library extension via `rusqlite`'s `LoadExtensionGuard`) is architecturally impossible with a pure-Rust SQLite engine — there is no `load_extension` hook for a C `.so`/`.dylib`. The project instead accepted a permanent pure-Rust fallback: spatial tables/queries go through `create_spatial_table`'s own WKT/WKB-based SQL rather than SpatiaLite's native geometry functions.
  - **If SpatiaLite compatibility is ever required:** it would need a from-scratch pure-Rust reimplementation of the SpatiaLite SQL function surface (`ST_*` UDFs) registered against `oxisql-sqlite-compat`, not extension loading. Not scoped here.

- [ ] WKB/WKT geometry codec shared across non-PostGIS backends.
  - **Goal:** MySQL stores geometries in WKB; SpatiaLite uses its own internal blob; ClickHouse uses `Tuple(Float64, Float64)` for points; MongoDB uses GeoJSON. Provide a single `to_native(geom, backend) → BackendBlob` / `from_native(blob, backend) → Geometry` round-trip.
  - **Design:** Re-export `oxigeo-postgis::wkb` (already in deps) plus add `src/wkb_common.rs` for backend-specific framing (MySQL prefix: `[4-byte SRID LE][1-byte byte-order][4-byte type][...]` per MySQL 8.0 ref §11.4.3; SpatiaLite blob: magic `0x00` + `[1-byte endian][4-byte SRID][8-byte mbr_x_min]...[envelope]...[wkb]...[0xFE]`).
  - **Files:** New `src/wkb_common.rs`; modify each `src/<backend>/reader.rs` to use shared codec.
  - **Tests:** (proposed) `test_wkb_mysql_format_with_srid`, `test_wkb_spatialite_blob_envelope_correct`, `test_geojson_mongodb_roundtrip_polygon`, `test_clickhouse_tuple_point_roundtrip`.
  - **Risk:** MySQL byte order vs SRID-prefix is a common bug; cite MySQL §11.4.3 in doc comments.
  - **Prerequisites:** None.

## Medium Priority
- [ ] ClickHouse `geoDistance` / `pointInPolygon` query helpers (currently raw SQL strings).
  - **Files:** `src/clickhouse/reader.rs:133 LoC`.
  - **Why deferred:** Reader exists; helpers are ergonomic wrappers.

- [ ] TimescaleDB hypertable creation + time-series spatial query helpers (`time_bucket` + `ST_Intersects`).
  - **Files:** `src/timescale/hypertable.rs:202 LoC` (scaffolding exists).

- [ ] Cassandra/ScyllaDB geohash-based partitioning utilities.
  - **Files:** `src/cassandra/types.rs:64 LoC` (only structs so far).

- [ ] Bulk insert via prepared-statement batching across all connectors.
  - **Files:** Each `<backend>/writer.rs`.

- [ ] Schema migration helpers: create spatial table + add spatial index.
  - **Goal:** `connector.create_spatial_table(name, srid, geometry_type)` produces idiomatic DDL.

- [ ] Connection health monitoring with automatic reconnect across all backends.
  - **Files:** `src/connection/health.rs:192 LoC`.

- [ ] Read-replica routing (writes to primary, reads to replica) for backends that support it.
  - **Files:** `src/connection/mod.rs`.

- [ ] GeoJSON ↔ backend geometry conversion for all backends (currently MongoDB-only natively).
  - **Files:** Each `<backend>/reader.rs`.

- [ ] Cursor-based pagination for large spatial result sets.

## Low Priority / Future (one-liners)
- [ ] DuckDB connector (embedded analytical SQL with `spatial` extension).
- [ ] Database-to-database ETL bridge (read source, write target).
- [ ] CockroachDB spatial support (PostGIS-compatible).
- [ ] Query-plan analysis (`EXPLAIN ANALYZE` parser per backend).
- [ ] Backup/restore utilities with spatial-data integrity check.
- [ ] Distributed-commit coordinator (2PC across backends).
- [ ] Pool metrics export to `oxigeo-observability` (Prometheus gauges per backend).
- [ ] `mongodb_async_std` alternative runtime.

## Cross-crate dependencies
- **Blocks:** `oxigeo-services` (WFS Transaction backends), `oxigeo-query` (alternative `DataSource` impls).
- **Blocked by:** `oxigeo-postgis` (shared WKB encoder).

## Recently completed (verbatim)
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-07-28 (status line refreshed: 43→53/36 tests, LoC 5,697→4,522, date bumped; SpatiaLite item marked superseded after confirming the crate migrated from rusqlite to pure-Rust `oxisql-sqlite-compat` — README's "SQLite has a C dependency" / "NOT in default features" claims were also stale and corrected: `sqlite` is pure-Rust and IS a default feature, while `mysql`/`mongodb`/`cassandra` are the non-default C/asm-pulling backends)*
