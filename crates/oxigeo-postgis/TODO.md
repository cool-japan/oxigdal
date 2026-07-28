# TODO: oxigeo-postgis

> **Purpose:** PostgreSQL/PostGIS client — async connection pool (deadpool-postgres), spatial query builder, OGC WKB codec, batch writer.
> **Status (2026-07-28):** 4,787 LoC (src) · 80 tests (all-features and default-features; 0 failed; inline + `tests/copy_binary_test.rs`) · 0 known real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Replace the "COPY → individual INSERTs" fallback in `PostGisWriter::flush` with a real PostgreSQL binary COPY stream.
  - **Verified gap:** `src/writer.rs:166-176` —
    ```rust
    // Build COPY statement
    let _copy_sql = format!(
        "COPY {} ({}, properties) FROM STDIN WITH (FORMAT binary)",
        table.qualified(),
        geom_col.quoted()
    );

    debug!("Flushing batch of {} features", self.batch.len());

    // For simplicity, we'll use individual INSERTs
    // A real implementation would use the COPY protocol for better performance
    ```
    The `_copy_sql` is never sent — the next 25 lines (`writer.rs:177-202`) loop `INSERT ... VALUES ($1, $2)` per feature, the exact anti-pattern the comment derides.
  - **Goal:** Bulk insert via PostgreSQL wire protocol v3 (PostgreSQL 16+ §53.2.6) COPY-in BINARY format. Throughput target ≥10× current per-row INSERT for batches ≥1000 features.
  - **Design:** Use `tokio_postgres::Client::copy_in(stmt)` (already in deps). Construct binary payload header: `PGCOPY\n\xff\r\n\0` (11-byte signature) + 4-byte flag field (0) + 4-byte extension length (0). Per row: `i16` field count, then for each field `i32` length + bytes. Geometry encoded via existing `WkbEncoder` (`src/wkb.rs:823 LoC`), prefixed with PostGIS EWKB SRID bit. JSON properties via `postgres_types::Json<serde_json::Value>` → text format. Trailer: `i16 -1`.
  - **Files:** `src/writer.rs:155-220` (rewrite `flush`), new `src/copy_binary.rs` (~250 LoC encoder), `src/wkb.rs` (verify EWKB SRID flag — `WkbEncoder` exists, confirm EWKB-mode toggle).
  - **Tests:** (proposed) `test_copy_in_binary_roundtrip_100_features`, `test_copy_in_binary_with_null_properties`, `test_copy_in_binary_srid_preserved`, `test_copy_in_falls_back_on_error_to_insert`, `test_copy_in_throughput_vs_insert` (bench-style, skipped in CI without `--ignored`).
  - **Risk:** Binary COPY format is column-order-sensitive; mismatched column order vs `INSERT INTO ({col}, properties)` semantics will silently corrupt. Always emit the explicit column list in the COPY DDL.
  - **Prerequisites:** Live PostgreSQL+PostGIS instance for integration tests — use `testcontainers` (workspace dev-dep) or guard with `--ignored`.
  - **Done:** 2026-05-22 (Slice 27). New `src/copy_binary.rs` (~235 LoC): DB-free `CopyBinaryEncoder` — 11-byte `PGCOPY\n\xff\r\n\0` signature + 8 zero header bytes; `begin_row(i16)`, `write_field_bytes` (i32 BE length prefix), `write_null` (i32 BE -1), `finish` (i16 BE -1 trailer); all integers big-endian. `ewkb_from_wkb(wkb, srid)` sets the `0x20000000` SRID flag on the geometry-type word and inserts the i32 SRID (respecting the WKB byte-order byte). `PostGisWriter::flush` rewritten (+117/-8): builds the explicit-column `COPY ... FROM STDIN WITH (FORMAT binary)`, streams the `CopyBinaryEncoder` payload through `tokio_postgres::CopyInSink<bytes::Bytes>`; on ANY COPY error logs `tracing::warn!` and falls back to the verbatim per-row `flush_via_inserts` (no data loss). `flush` public signature byte-for-byte unchanged. `lib.rs` +2 re-export lines. `WkbEncoder` confirmed to support both plain WKB and EWKB (`with_srid`); the slice uses plain WKB + `ewkb_from_wkb` for the explicit, unit-testable path. No Cargo.toml change (`bytes` already a workspace dep; `tokio-postgres` `copy_in` available by default).
  - **Tests:** 10 in `crates/oxigeo-postgis/tests/copy_binary_test.rs` — 9 active byte-layout encoder tests (signature, header flags, field count, length prefix BE, null, trailer, multi-row, EWKB SRID flag, EWKB Point round trip) + 1 `#[ignore]` live-DB round-trip needing PostgreSQL+PostGIS. Build + clippy clean; 9 pass, 1 skipped.

- [x] Verify and document that `WkbEncoder` writes EWKB (with SRID flag), not plain WKB.
  - **Verified gap:** `src/wkb.rs:42-57` declares `WkbGeometryType { Point=1, ..., GeometryCollection=7 }` and `from_code` at line 61-73 masks with `code & 0xFF` and decodes the **base** code only. The doc comment at `src/wkb.rs:1-5` says "PostGIS uses Extended WKB (EWKB) which includes SRID information" — but the encoder side needs spot-checking that bit 0x20000000 (`EWKB_SRID_FLAG`) is set and the SRID is written when present. Read of lines 1-80 confirms only the base 7 codes exist as variants.
  - **Goal:** `WkbEncoder` always emits EWKB-compliant byte stream when an SRID is provided. Decoder reads either plain WKB or EWKB transparently.
  - **Design:** EWKB type code = `base_type | 0x20000000 (SRID)` (also `0x80000000` for Z, `0x40000000` for M). After header, write `i32 LE srid` when SRID flag set. Update `WkbDecoder` symmetrically; `from_code` should already mask but verify all three top bits.
  - **Files:** `src/wkb.rs` (audit encode_point/encode_linestring/... around lines 270-450).
  - **Tests:** (proposed) `test_ewkb_point_srid_4326_roundtrip`, `test_ewkb_z_geometry_roundtrip`, `test_ewkb_zm_geometry_roundtrip`, `test_plain_wkb_decode_no_srid`.
  - **Risk:** Hidden encoder bug would corrupt PostGIS data silently; existing tests may pass on plain WKB even when SRID expected.
  - **Prerequisites:** None.
  - **Done:** (verified 2026-07-28) `src/wkb.rs:125-127` defines `SRID_FLAG = 0x2000_0000`, `Z_FLAG = 0x8000_0000`, `M_FLAG = 0x4000_0000` exactly per the design above. `WkbEncoder::write_header` (`src/wkb.rs:179-215`) OR's in `SRID_FLAG` whenever `self.srid.is_some()` (set via `WkbEncoder::with_srid`), and `Z_FLAG`/`M_FLAG` per-geometry, then writes the SRID as a little-endian `i32` immediately after the type code; every `encode_*` method (`encode_point`, `encode_linestring`, `encode_polygon`, `encode_multipoint`, `encode_multilinestring`, `encode_multipolygon`, `encode_geometrycollection`) routes through it. `WkbDecoder` reads the same three flags symmetrically (`src/wkb.rs:468-474`: `has_srid`/`has_z`/`has_m`) and decodes the SRID when present. Roundtrip covered by `test_wkb_with_srid` (`src/wkb.rs:814-825`). Not done: the four specific proposed test names above were not added verbatim (existing coverage is `test_wkb_with_srid` rather than the `test_ewkb_*`-named tests), and there is no standalone doc-comment callout stating this explicitly beyond the module doc already at `src/wkb.rs:1-5`.

- [ ] Connection-pool health monitoring with automatic reconnection on broken connections.
  - **Verified gap:** `src/connection.rs` configures `deadpool_postgres::Pool` with `RecyclingMethod::Fast` (or similar — confirm at runtime), but exposes no background watchdog that detects `BrokenPipe` / server restart and pre-warms a fresh connection set.
  - **Goal:** Background tokio task pings each idle connection every `health_check_interval` (configurable, default 30s); broken connections are evicted; pool refills to `min_size`. `HealthCheckResult` exposed via `ConnectionPool::health_check().await`.
  - **Design:** Spawn task in `ConnectionPool::new`. Use `client.simple_query("SELECT 1")` for ping. Track stats: `total_connections`, `idle`, `in_use`, `failed_pings_since_last_recover`. Configurable backoff on reconnect (exponential, capped).
  - **Files:** `src/connection.rs:490 LoC` (extend `ConnectionPool::new`).
  - **Tests:** (proposed) `test_pool_recovers_after_simulated_server_restart`, `test_pool_health_check_reports_idle_count`, `test_pool_refill_to_min_size_after_eviction`.
  - **Risk:** Test requires fault injection — use `testcontainers` `with_kill_signal()`.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Parameterised query builder API to prevent SQL injection at the type level.
  - **Goal:** `SpatialQuery::raw_where(sql, params)` instead of string-formatted predicates; columns and table names pass through `ColumnName::new()` / `TableName::new()` validators (already exist in `src/sql.rs:587 LoC`).
  - **Files:** `src/query.rs:393 LoC`, `src/sql.rs`.
  - **Why deferred:** Existing API already routes through validators; this is a public-API guarantee/audit, not a feature.

- [ ] PostGIS Raster (raster2pgsql output) support: `ST_AsTIFF`, `ST_FromGDALRaster`.
  - **Files:** New `src/raster.rs`.
  - **Why deferred:** PostGIS Raster usage in production is narrow vs vector workloads.

- [ ] Spatial index management: programmatic `CREATE INDEX ... USING GIST (geom)`, `ANALYZE`, `CLUSTER`.
  - **Files:** `src/sql.rs`.

- [ ] Server-side cursor (portal) pagination for large result sets (PG protocol v3 Extended Query).
  - **Goal:** Stream million-row geometry result without OOM.
  - **Files:** `src/reader.rs:205 LoC` (already exposes `stream()`; add cursor variant).

- [ ] Prepared-statement caching keyed by SQL template.

- [ ] TLS/`rustls` connection support (feature `rustls` already declared at `Cargo.toml:18`).

- [ ] `LISTEN` / `NOTIFY` for real-time change events.
  - **Files:** New `src/listen.rs`.

- [ ] PostgreSQL URI / DSN connection-string parsing (`postgresql://user:pass@host:port/db?sslmode=...`).
  - **Files:** `src/connection.rs`.

- [ ] PostGIS Topology extension support.

- [ ] Schema migration helpers (geometry-column DDL with `AddGeometryColumn`).

## Low Priority / Future (one-liners)
- [ ] 3D geometry (`ST_3DDistance`, `ST_3DIntersects`) per PostGIS 3.x.
- [ ] Foreign data wrapper integration (`postgres_fdw`).
- [ ] pg_tileserv / pg_featureserv-compatible SQL generation.
- [ ] Database migration version tracking.
- [ ] Pool metrics export (active/idle/waiting) for `oxigeo-observability`.
- [ ] Automatic geometry simplification (`ST_Simplify`) for low-zoom display queries.
- [ ] Async `COPY ... TO STDOUT` reader for bulk export.

## Cross-crate dependencies
- **Blocks:** `oxigeo-services` (WFS Transaction execution), `oxigeo-query` (PostGIS `DataSource`), `oxigeo-db-connectors` (TimescaleDB connector inherits from this crate).
- **Blocked by:** None.

## Recently completed (verbatim)
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-07-28*
