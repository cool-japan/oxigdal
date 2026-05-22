# TODO: oxigdal-drivers/geojson

> **Purpose:** GeoJSON (RFC 7946) driver for OxiGDAL - Pure Rust vector data processing with streaming support
> **Status (2026-05-16):** 8,357 Rust LoC (incl. tests) - 175 tests - 0 source-code stubs (mature crate; gaps are forward-looking features)
> **Roadmap:** v0.1.5 (current slice) - v0.2.0 - v1.0.0

## High Priority (next slice - verified gaps)

- [ ] Add TopoJSON read support (shared-arc topology, complementary format to GeoJSON)
  - **Verified gap:** No TopoJSON module exists. `ls src/` shows `error.rs`, `reader.rs`, `types/`, `utils/`, `validation.rs`, `writer.rs` only. `rg -n "topojson|TopoJson" -g '*.rs' src/` returns no matches. The previous TODO entry "Add TopoJSON reading support (shared arc topology)" is genuine future work.
  - **Goal:** Open a `.topojson` file, materialize features by dereferencing the shared `arcs` table and applying delta-encoded quantization.
  - **Design:** Per TopoJSON spec v3.0.0 (<https://github.com/topojson/topojson-specification/blob/master/README.md>): root has `type:"Topology"`, `arcs:[[[dx,dy],...],...]`, `bbox:[...]`, `transform:{scale,translate}`, `objects:{<name>:Geometry|GeometryCollection}`. Arcs are integer-delta-encoded; reverse: `point[i] = transform.scale * (sum of deltas) + transform.translate`. Geometry indices: `arcs:[1, 2, -3]` means follow arc 1 forward, arc 2 forward, arc 3 in reverse (one's complement). Output as our existing `Feature`/`FeatureCollection`.
  - **Files:** (new) `src/topojson/mod.rs`, (new) `src/topojson/reader.rs`, (new) `src/topojson/arc_decode.rs`, `src/lib.rs` (`pub mod topojson;`)
  - **Tests:** (proposed) `test_topojson_simple_polygon_arc_dereference`, `test_topojson_negative_arc_index_reverses`, `test_topojson_transform_quantization`, `test_topojson_geometry_collection`, `test_topojson_round_trip_via_writer` (writer is a follow-up item)
  - **Risk:** Quantization parameters affect numerical precision; document round-trip not always exact.
  - **Prerequisites:** None.

- [ ] Add CRS transformation on read/write via `oxigdal-proj`
  - **Verified gap:** `src/types/crs.rs` (11.0K) defines `Crs` type as a tag only; `rg -n "transform.*coord|reproject|to_crs" -g '*.rs' src/` returns no transformation code. The default CRS in `src/lib.rs:131` is `"urn:ogc:def:crs:OGC:1.3:CRS84"` (WGS 84 lon/lat); files using non-default CRS are read and tagged but not transformed.
  - **Goal:** `GeoJsonReader::reproject_to(target_crs)` and corresponding writer side; lift legacy non-WGS84 GeoJSON into RFC 7946-conformant WGS84.
  - **Design:** Wire `oxigdal-proj::Transformer` (workspace dep). Per-feature coordinate traversal: each `Position` -> `Transformer::transform_2d` (or 3d when `has_z`). Update `Crs` tag on output. Spec context: RFC 7946 §4 says all GeoJSON is implicitly WGS84; CRS in the file is legacy GeoJSON 2008 behavior - rejecting it is also a valid choice.
  - **Files:** (new) `src/reproject.rs`, `Cargo.toml` (add `oxigdal-proj` under a `reproject` feature)
  - **Tests:** (proposed) `test_reproject_web_mercator_to_wgs84`, `test_reproject_strips_crs_on_write_per_rfc7946`, `test_reproject_polygon_with_holes`, `test_reproject_3d_position_preserves_z`
  - **Risk:** RFC 7946 strict mode forbids CRS member; users wanting to preserve might want raw mode. Provide both.
  - **Prerequisites:** None.

- [ ] Right-hand rule enforcement on polygon write (RFC 7946 §3.1.6)
  - **Verified gap:** `src/validation.rs:21` and :143 detect winding order; `src/writer.rs` does not force orientation on emit. `rg -n "force_winding|enforce_winding|ensure_ccw" -g '*.rs' src/writer.rs` returns no matches.
  - **Goal:** Writer reorients exterior rings counter-clockwise and interior rings clockwise per RFC 7946 §3.1.6 ("right-hand rule"), regardless of input winding.
  - **Design:** Add `WriterConfig.enforce_rfc7946_winding: bool` (default true in strict mode). On `write_geometry` for Polygon / MultiPolygon, compute signed area via shoelace; if exterior ring area < 0 (CW), reverse; if interior ring area > 0 (CCW), reverse. The shoelace formula is already in `src/validation.rs:447`.
  - **Files:** `src/writer.rs`, `src/types/geometry.rs` (already has shoelace - reuse)
  - **Tests:** (proposed) `test_writer_forces_ccw_exterior_ring`, `test_writer_forces_cw_interior_ring`, `test_writer_strict_mode_default_enforces`, `test_writer_legacy_mode_preserves_input_winding`
  - **Risk:** Some workflows (e.g., legacy GeoJSON 2008 compat) want preserved winding; gate behind config.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] Property type inference and schema extraction from FeatureCollection
  - **Goal:** Scan feature properties to produce a `{name -> JsonType}` schema for downstream Arrow / SQL.
  - **Files:** (new) `src/schema.rs`
  - **Why deferred:** Useful for conversion pipelines; not immediate.

- [ ] GeoJSON-to-Shapefile / GeoJSON-to-GeoParquet conversion helpers
  - **Goal:** One-shot conversion entry points.
  - **Files:** (new) `src/convert.rs`
  - **Why deferred:** Cross-crate; better as `oxigdal` umbrella CLI subcommand.

- [ ] Antimeridian-crossing geometry splitting (180° split)
  - **Goal:** Per RFC 7946 §3.1.9, split geometries crossing 180°.
  - **Files:** (new) `src/antimeridian.rs`
  - **Why deferred:** Edge case for global datasets only.

- [ ] FeatureCollection merge from multiple files
  - **Goal:** Concatenate while preserving distinct CRS / foreign members.
  - **Files:** (new) `src/merge.rs`
  - **Why deferred:** Trivial to do client-side; not value-add as library API.

- [ ] On-the-fly geometry simplification during write (Douglas-Peucker)
  - **Goal:** Reduce vertex count for size optimization.
  - **Files:** `src/utils/simplify.rs` (exists!) - check what it currently does.
  - **Why deferred:** Tied to writer config polish.

- [ ] GeoJSON diff (compare two FeatureCollections, report changes)
  - **Goal:** Structural diff at feature level.
  - **Files:** (new) `src/diff.rs`
  - **Why deferred:** Tool concern.

- [ ] Round-trip foreign-member preservation test suite (struct fields already in place per `src/types/feature.rs`)
  - **Goal:** Verify unknown properties survive read-modify-write.
  - **Files:** `tests/round_trip_foreign.rs` (new)
  - **Why deferred:** Verification, not implementation.

- [ ] RFC 7946 strict-mode validator
  - **Goal:** Reject GeoJSON 2008 quirks (top-level CRS, non-RFC types, etc.).
  - **Files:** `src/validation.rs`
  - **Why deferred:** Polish on top of existing validator.

## Low Priority / Future (speculative - concise)

- [ ] GeoJSON-T (temporal) extension support
- [ ] Parallel feature parsing for large files
- [ ] Coordinate rounding to snap near-equal vertices
- [ ] GeoJSON tiling (split large collections into spatial tiles)
- [ ] GeoJSON statistics (feature count, geometry types, bbox) without full parse
- [ ] Nested property object/array handling
- [ ] GeoJSON-to-MVT (Mapbox Vector Tile) conversion

## Cross-crate dependencies
- **Blocks:** None.
- **Blocked by:** `oxigdal-proj` (for reprojection only).

## Recently completed (kept verbatim from previous TODO.md)
- [x] Implement streaming writer for FeatureCollection (write features one-at-a-time) — `GeoJsonWriter::write_features`, `start_feature_collection`, `write_feature_streaming`, `finish_feature_collection`
- [x] Add GeoJSON-seq (newline-delimited GeoJSON / GeoJSONL) support — `read_geojsonl`, `write_geojsonl`, `open`, `open_geojsonl`, `write_geojsonl_to_file`
- [x] Implement spatial filtering during streaming read (bbox predicate pushdown) — `features_in_bbox`, `geometry_bbox`, `feature_bbox_intersects`
- [x] Add coordinate precision control in writer (configurable decimal places) — `WriterConfig.coordinate_precision` is now fully wired: `apply_precision_to_geometry` rounds all `Position` values before serialization in `write_geometry`, `write_feature`, and `write_feature_collection`
- [x] Implement bounding box calculation and injection during write — `WriterConfig.write_bbox` / `compute_bbox`

---
*Last audited: 2026-05-16*
