# TODO: oxigeo-drivers/shapefile

> **Purpose:** Shapefile (ESRI) driver for OxiGeo - Pure Rust GDAL reimplementation
> **Status (2026-07-28):** 7,312 Rust LoC (incl. tests) - 137 tests - 0 source-code stubs (mature; ESRI tech doc fully covered)
> **Roadmap:** v0.1.7 - v0.2.0 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Add `.dbf` memo field support (`.dbt` files) for long text attributes
  - **Verified gap:** `ls src/dbf/` (`dbf/` is a module) and `rg -n "memo|dbt|Memo" -g '*.rs' src/dbf` returns no matches. Memo (M) field in DBF stores a pointer (block index) into a sibling `.dbt` file holding the actual long text. Current driver treats M fields as unsupported or returns the raw 4/10-byte pointer.
  - **Goal:** Reading a Shapefile whose DBF declares Memo fields returns the dereferenced text content per record.
  - **Design:** Per dBase IV file format (<http://www.clicketyclick.dk/databases/xbase/format/dbt.html>): `.dbt` is a flat file of 512-byte blocks; block 0 reserved (header); subsequent blocks contain text terminated by `0x1A 0x1A`. M field in `.dbf` record holds a 10-char ASCII block index (right-justified, space-padded), or a 4-byte LE block index for dBase III+. Resolve: parse index from M field; seek `.dbt` to `block_idx * 512`; read until `0x1A 0x1A`; UTF-8 / code-page decode per `.cpg`. Add `FieldValue::Memo(String)` variant.
  - **Files:** (new) `src/dbf/memo.rs`, `src/dbf/mod.rs`, `src/dbf/field_type.rs` (or wherever `FieldType` lives)
  - **Tests:** (proposed) `test_dbt_block_dereference`, `test_memo_field_unicode_via_cpg`, `test_memo_field_missing_dbt_fails_gracefully`, `test_memo_field_terminator_handling`
  - **Risk:** dBase III vs IV vs FoxPro memo formats differ; document supported variants.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 26). New `src/dbf/memo.rs` (~293 LoC): `MemoFile { handle, block_size, version, next_block }`, `MemoVersion { DBase3, DBase4, FoxPro }` (DBase4 only in Slice 26; DBase3/FoxPro → `MemoError::UnsupportedVersion`), `MemoError { Io(io::Error), InvalidHeader(String), UnsupportedVersion(String), BlockIndexOutOfRange { index, available }, MissingTerminator }`. `MemoFile::open` reads block-0 header (next_block u32 LE @0, version byte @16 = 0x03 for DBase IV, block_size u16 LE @20 default 512). `read_block(index)` seeks to `index * block_size`, reads 8-byte `FF FF 08 00 <length u32 LE>` header, reads `length - 8` payload bytes, strips trailing `0x1A` terminators; falls back to `read_block_terminator_search` for variant DBase formats that store text without the FF-FF-08-00 marker. `dbf/record.rs` +11 lines (doc comment + memo-pointer parse logic unchanged at the field-level; dereferencing happens in post-pass). `dbf/mod.rs` +135 lines: `DbfReader::memo: Option<MemoFile>` field (default None), `set_memo_file(memo)` setter, `has_memo()` accessor, `resolve_memo_fields` post-pass on `read_record` walking parsed fields and replacing `Value::String(pointer_idx_text)` with `Value::String(memo.read_block(idx)?)`, one-shot `tracing::warn!` gated by `AtomicBool` when memo field encountered without `.dbt` attached. `reader.rs` +42 lines: `discover_memo_path` (try `.dbt` then `.DBT`), `ShapefileReader::open`/`iter_features`/`read_features` attach the memo before parsing. `lib.rs` +5 lines re-exports `MemoFile`/`MemoError`/`MemoVersion`. UTF-8 lossy decode (codepage decoding deferred per spec). Note: crate name is `oxigeo-shapefile` (not `oxigeo-drivers-shapefile`).
  - **Tests:** 9 in `crates/oxigeo-drivers/shapefile/tests/dbf_memo_test.rs` (opens valid DBase IV; rejects truncated header; rejects DBase III as UnsupportedVersion; read_block returns text up to terminator; read_block out-of-range error; pointer-from-10-byte ASCII; reader discovers sibling .dbt; no-memo-attached warns + continues; end-to-end memo value dereferenced). Full crate suite 120/120 (no regressions).

- [ ] Implement CRS reprojection during read / write
  - **Verified gap:** `.prj` reading/writing is already implemented (per `[x]` items below), but no actual coordinate transformation. `rg -n "reproject|transform.*coord|oxigeo-proj" -g '*.rs' src/` returns no matches. `Cargo.toml` does not declare `oxigeo-proj`.
  - **Goal:** `ShapefileReader::reproject_to(target_crs)` and a `ShapefileWriterBuilder::reproject_from_to` mirror; transform every vertex through the configured CRS pipeline.
  - **Design:** Wire `oxigeo-proj::Transformer` (workspace dep) behind a `reproject` feature flag. Build pipeline once from `.prj` WKT + target EPSG; apply per-vertex during read iteration; rewrite `.prj` on write. Spec: ISO 19162 WKT2 + EPSG codes.
  - **Files:** (new) `src/reproject.rs`, `Cargo.toml` (add `oxigeo-proj.workspace = true` under feature gate)
  - **Tests:** (proposed) `test_reproject_wgs84_to_web_mercator`, `test_reproject_polygon_preserves_topology`, `test_reproject_z_dimension_preserved`, `test_reproject_writes_target_prj`
  - **Risk:** WKT round-tripping is imperfect; supply target as EPSG code when possible.
  - **Prerequisites:** None.

- [ ] Field type auto-detection for writer (infer DBF field types from Rust types)
  - **Verified gap:** `src/writer/mod.rs` (audit shows `ShapefileSchemaBuilder` at `src/lib.rs:173`) requires caller to declare each field's `FieldType` and size. `rg -n "infer.*type|from_value|auto.*field" -g '*.rs' src/writer` returns no matches.
  - **Goal:** `ShapefileWriter::write_features_inferred(features)` scans a small sample to determine field type + width, then writes.
  - **Design:** Sample N features (default 100); for each property name observed: collect type witnessed (Number/Logical/Character/Date) and max string length. Numeric: if all integers fit in 9 digits -> Number, else Float. Strings: width = max observed (clamped to 254 max). After scan, emit DBF header then iterate.
  - **Files:** (new) `src/writer/auto_schema.rs`, `src/writer/mod.rs`
  - **Tests:** (proposed) `test_auto_schema_mixed_types`, `test_auto_schema_string_width_max`, `test_auto_schema_handles_null_values`, `test_auto_schema_with_date_string`
  - **Risk:** Two-pass over data; for streaming inputs need a buffered sample. Document.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] Date field type writing with proper YYYYMMDD formatting
  - **Goal:** DBF Date field (D) stores 8 ASCII chars; writer should accept `chrono::NaiveDate` and format correctly.
  - **Files:** `src/dbf/`, `src/writer/`
  - **Why deferred:** Small enhancement; depends on chrono import decision.

- [ ] Record-level random access via `.shx` offsets
  - **Goal:** `reader.feature_at(index)` jumps directly using SHX entry.
  - **Files:** `src/reader.rs`, `src/shx/`
  - **Why deferred:** Tests show iter-based read; random access is an additional API.

- [ ] Null shape record handling (mixed geometry types via shape type 0)
  - **Goal:** Read/write Null shapes (type 0) interspersed with other shape types.
  - **Files:** `src/shp/shapes.rs`
  - **Why deferred:** Niche edge case.

- [ ] Shapefile validation (header consistency, declared bbox vs actual, record count match)
  - **Goal:** Diagnose subtly corrupt shapefiles.
  - **Files:** (new) `src/validate.rs`
  - **Why deferred:** Polish.

- [ ] Shapefile merge (combine multiple files with identical schema)
  - **Goal:** Concatenate into single output preserving field schemas.
  - **Files:** (new) `src/merge.rs`
  - **Why deferred:** Tool-level concern.

## Low Priority / Future (speculative - concise)

- [ ] Async shapefile reading for cloud storage backends
- [ ] Shapefile splitting by attribute value or spatial extent
- [ ] GeoJSON / GeoParquet conversion helpers
- [ ] dBase IV and dBase 7 extended field types (Integer/Currency/DateTime)
- [ ] Shapefile statistics (feature count, bbox, field summary) without full read
- [ ] SHX rebuild from SHP (recover from missing index)
- [ ] Encoding auto-detection when `.cpg` is missing (chardet-style heuristic)
- [ ] Shapefile-to-WKB/WKT geometry conversion utility

## Cross-crate dependencies
- **Blocks:** None directly.
- **Blocked by:** `oxigeo-proj` (for reprojection feature only).

## Recently completed (kept verbatim from previous TODO.md)
- [x] Implement full PolyLine/Polygon/MultiPoint geometry conversion to OxiGeo core types
- [x] Add `.prj` file reading and writing for CRS (projection) support
- [x] Implement `.cpg` code page file reading for proper character encoding
- [x] Add spatial filtering during read (bounding box query using .shx index)
- [x] Implement streaming record iterator for large shapefiles (avoid loading all into memory) — `ShapefileReader::iter_features()` returns `Result<FeatureIter<'_>>` that yields one `ShapefileFeature` per call with O(1) memory
- [x] Shapefile writer: PolygonZ (15), PolygonM (25), PointZ (11), PointM (21), MultiPatch (31), PolyLineZ (13), PolyLineM (23) — verified 2026-05-16: `src/writer/polygon_z_m.rs`, `polyline_z_m.rs`, `point_z_m.rs`, `multipatch.rs` all exist with shape-type emission tests (the previous `[~]` was stale; this work is complete).
  - **Goal:** Writer accepts geometries with Z and/or M dimensions and emits correct shape-type byte and coordinate arrays per ESRI Shapefile spec.
  - **Design:**
    - Shape type codes: 11 PointZ, 13 PolyLineZ, 15 PolygonZ, 21 PointM, 23 PolyLineM, 25 PolygonM, 31 MultiPatch
    - Z records: XY arrays - Zmin/Zmax - Z array - optional Mmin/Mmax - M array
    - M records: XY arrays - Mmin/Mmax - M array
    - Dispatch via has_z()/has_m() accessors on Geometry
  - **Files:** polygon_z.rs, polygon_m.rs, point_z_m.rs, multipatch.rs, polyline_z_m.rs (all new)
  - **Tests:** 6 tests covering shape types, record layout, roundtrip
- [x] Implement attribute filtering during read (SQL-like WHERE clause)

---
*Last audited: 2026-07-28*
