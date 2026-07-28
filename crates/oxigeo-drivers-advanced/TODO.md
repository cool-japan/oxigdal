# TODO: oxigeo-drivers-advanced

> **Purpose:** Advanced format drivers for OxiGeo — JPEG2000 (JP2/J2K), GeoPackage (GPKG, pure-Rust `oxisql-sqlite-compat`-backed), KML/KMZ, GML — feature-gated for selective compile.
> **Status (2026-07-28):** 5,846 LoC · 156 tests all-features / 98 default-features · 0 real stubs (both `jp2/codestream.rs` gray-fill placeholder and `gpkg/spatial_index.rs` placeholder-bounds RTree confirmed closed — see Recently completed).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Real Pure-Rust JPEG2000 decoder
  - **Verified done:** the `vec![128u8; size]` gray-fill placeholder is gone from `src/jp2/codestream.rs`. The file now explicitly documents and tests against its removal: `codestream.rs:484` ("as opposed to the removed `vec![128u8; ...]` placeholder") and two tests asserting `"decoded data must not be the removed uniform gray placeholder"`. Real tier-1 EBCOT + inverse 5/3 DWT decoding is delegated to a new sibling workspace crate, `oxigeo-jpeg2000` (`Cargo.toml:36` `oxigeo-jpeg2000 = { workspace = true, optional = true }`, gated by the `jpeg2000` feature which **is** in `default`), with `codestream.rs` doc comments confirming: "Delegates the actual wavelet/EBCOT/tier-2 decoding to … pure-Rust JPEG2000 decode chain (tier-1 EBCOT, inverse 5/3 DWT … in `oxigeo-jpeg2000`".
  - **Delta from original design:** the EBCOT/DWT/color-transform modules were NOT added directly under this crate's `src/jp2/` (no `ebcot.rs`/`dwt.rs`/`color.rs` here) — instead the heavy codec logic lives in the separate `oxigeo-jpeg2000` crate and this crate's `codestream.rs` is the JP2-container-format wiring/delegation layer. Scope (lossy 9/7 vs. lossless 5/3, Profile-0 vs. full Part-1) not independently re-verified in this audit — confirm against `oxigeo-jpeg2000`'s own TODO.md if precision claims matter for a release.

- [x] GeoPackage RTree spatial index from real geometry bounds
  - Done: 2026-05-31 (Slice 28). Tests: 6 new (spatial_index_test) + 117 existing = 123 total.
  - **Verified gap:** `src/gpkg/spatial_index.rs:78-87` — `// Query all features with their bounding boxes / // This is a simplified version - in practice you'd extract bounds from WKB geometry / … // Placeholder bounds - real implementation would calculate from geometry / Ok((fid, 0.0, 0.0, 1.0, 1.0))`
  - **Goal:** Build the `rstar::RTree<GeomEntry>` from real per-feature envelopes decoded from the GeoPackage WKB geometry column (GPKG binary header per OGC GeoPackage 1.4 §2.1.3, followed by ISO 13249-3 WKB).
  - **Design:** Replace the `query_map` placeholder with `SELECT fid, geom FROM {table} WHERE geom IS NOT NULL`; for each row, parse the GPKG binary header (4-byte magic `GP`, version, flags, srs_id, optional 32/64-byte envelope), then read the WKB body. If the envelope flag is set, use the embedded envelope; else compute bounds by streaming WKB coordinates. Output `(fid, min_x, min_y, max_x, max_y)`.
  - **Files:** `crates/oxigeo-drivers-advanced/src/gpkg/spatial_index.rs` (replace lines 75-90), share WKB parser with `src/gpkg/geometry.rs`.
  - **Tests:** (proposed) `test_spatial_index_envelope_from_gpkg_header_flag`, `test_spatial_index_envelope_from_wkb_scan_when_flag_absent`, `test_spatial_index_handles_3d_xyz_geometry`, `test_spatial_index_skips_null_geom_rows`, `test_spatial_index_query_returns_intersecting_fids`.
  - **Risk:** WKB endianness byte handled correctly (`0x00`=big, `0x01`=little).
  - **Prerequisites:** None (WKB parser exists elsewhere in workspace; reuse).

- [ ] GeoPackage feature writing with WKB encoding
  - **Goal:** Symmetric writer for `gpkg_geometry_columns`/`gpkg_contents`-registered feature tables — accept `geo_types::Geometry`, prepend the GPKG binary header (with envelope), serialize WKB body, INSERT row including attribute columns.
  - **Design:** New `GeoPackageWriter { conn }` with `create_feature_table(name, columns, srs_id, geometry_type)` setting up the standard metadata rows (`gpkg_contents` `data_type='features'`, `gpkg_geometry_columns` entry), plus `insert_feature(table, attributes, geom)` encoding to WKB via `wkb::Writer`. Use `oxiarc-*` for any compression; do not introduce `flate2` or `bzip2`.
  - **Files:** `crates/oxigeo-drivers-advanced/src/gpkg/writer.rs` (new ~400 LoC).
  - **Tests:** (proposed) `test_gpkg_write_point_feature_roundtrip`, `test_gpkg_write_polygon_with_hole_roundtrip`, `test_gpkg_write_creates_metadata_rows`, `test_gpkg_write_envelope_flag_set`, `test_gpkg_write_3d_geometry_xyz_flag`.
  - **Risk:** GeoPackage 1.4 requires `application_id` PRAGMA `0x47504B47` (`GPKG`) and `user_version` `10400` — must set both.
  - **Prerequisites:** None.

- [ ] GeoPackage raster tile reading (tile matrix sets)
  - **Goal:** Read tile blobs from `gpkg_tile_matrix`-registered tables, decoding PNG/JPEG/WebP per the `image` crate (Pure Rust).
  - **Design:** Iterate `gpkg_tile_matrix_set` for the layer's extent + zoom-level matrix; for each requested `(zoom, x, y)`, `SELECT tile_data FROM {table} WHERE zoom_level=? AND tile_column=? AND tile_row=?`; sniff magic (PNG `89 50 4E 47`, JPEG `FF D8 FF`, WebP `52 49 46 46 ... 57 45 42 50`) and decode.
  - **Files:** `crates/oxigeo-drivers-advanced/src/gpkg/tiles.rs` (new).
  - **Tests:** (proposed) `test_gpkg_tiles_list_zoom_levels`, `test_gpkg_tiles_read_png_tile_decodes`, `test_gpkg_tiles_read_jpeg_tile_decodes`, `test_gpkg_tiles_read_webp_tile_decodes`, `test_gpkg_tiles_missing_tile_returns_none`.
  - **Risk:** WebP path requires the new lossless WebP encoder/decoder from the project (per project memory: "Implement lossless WebP encoding and integrate into image rendering pipeline" already landed).
  - **Prerequisites:** Workspace `image` crate version pin (already present).

- [x] KML Placemark geometry extraction into OxiGeo core types
  - **Verified done:** `src/kml/features.rs` defines `Placemark { name: Option<String>, description: Option<String>, geometry: Option<Geometry>, style_url: Option<String>, extended_data: Vec<(String, String)> }` (derives `Serialize`/`Deserialize`), and `src/kml/parser.rs::parse_placemark` is a real `quick-xml` SAX-mode walker wired from `read_kml` in `src/kml/mod.rs:152`.
  - **Delta from original design:** the property bag is `Placemark.extended_data: Vec<(String, String)>`, not a separate `KmlFeature { geom, properties: HashMap<String, JsonValue> }` wrapper type as sketched — geometry and metadata live directly on `Placemark`. Typed (non-string) ExtendedData and explicit lon/lat-order regression tests not independently re-verified in this audit.

- [x] KMZ archive reading
  - **Verified done:** `src/kmz/mod.rs` provides `read_kmz`, `read_kmz_file`, `write_kmz`, `write_kmz_file`, built on `oxiarc_archive::zip::{ZipReader, ZipWriter}` (`use oxiarc_archive::zip::{ZipCompressionLevel, ZipReader, ZipWriter};`) — confirms the COOLJAPAN Pure-Rust policy requirement (never the `zip` crate) is honored.

- [x] GML geometry parsing (gml:Point, gml:Polygon, etc.) — PARTIAL
  - **Verified done:** `src/gml/geometry.rs` + `src/gml/parser.rs` real SAX-mode parser handles `gml:Point`/`GmlPoint` (2D and `with_z` 3D), `gml:LineString`/`GmlLineString`, and `gml:Polygon`/`GmlPolygon` with `exterior`/`interior` (and legacy `outerBoundaryIs`/`innerBoundaryIs`) rings, `posList`/`coordinates` text parsing, and `srsDimension` honored whether declared on the geometry or directly on `posList`. 7 tests in `parser.rs`, 3 each in `geometry.rs`/`features.rs`/`writer.rs`.
  - **Gap remaining:** no evidence found of the GML-3.2-declares-lat/lon axis-order swap for geographic CRSes (`rg "axis|lat.*lon|EPSG::4326|urn:ogc" src/gml/` — no matches), and `gml:MultiSurface`/`gml:MultiCurve`/`gml:MultiPoint` aggregation was not confirmed. Leaving the Medium-priority "GML namespace-aware parsing for complex feature types" item below open covers the remaining Multi*/axis-order work.

## Medium Priority
- [ ] KML style/icon extraction and mapping (StyleMap, IconStyle).
  - **Files:** `src/kml/styles.rs` (new).
  - **Why deferred:** Geometry first, styling later.
- [ ] KML NetworkLink following for remote-document aggregation.
  - **Files:** `src/kml/network_link.rs` (new).
  - **Why deferred:** Requires HTTP client wiring (optional `reqwest`).
- [ ] GeoPackage extension support (RTree, metadata, schema extensions per GPKG-EXT spec).
  - **Files:** `src/gpkg/extensions.rs` (new).
  - **Why deferred:** Core read/write first.
- [ ] GML schema (XSD) reading for typed feature access.
  - **Files:** `src/gml/schema.rs` (new).
  - **Why deferred:** Schema-less parse covers most users.
- [ ] KML → GeoJSON conversion helper.
  - **Files:** `src/kml/geojson.rs` (new).
  - **Why deferred:** Trivial once Item 5 lands.
- [ ] GeoPackage tile-matrix-set creation/writing (symmetric to Item 4 read).
  - **Files:** `src/gpkg/tiles_writer.rs` (new).
  - **Why deferred:** After tile read (Item 4).
- [ ] GML namespace-aware parsing for complex feature types (WFS schemas).
  - **Files:** `src/gml/parser.rs` (extend).
  - **Why deferred:** After simple-geometry path.
- [ ] GeoPackage related-tables extension.
  - **Files:** `src/gpkg/related.rs` (new).
  - **Why deferred:** Optional GPKG extension.

## Low Priority / Future (one-liners)
- [ ] CityGML reading (LOD0-LOD4 building models).
- [ ] IndoorGML for indoor navigation.
- [ ] GeoPackage vector-tile extension.
- [ ] KML tour/animation extraction.
- [ ] WFS (Web Feature Service) client using GML parser.
- [ ] GeoPackage validation tool (CLI).
- [ ] GML 3.2 advanced geometry types (curves, surfaces).
- [ ] KML region + LOD-based feature selection.

## Cross-crate dependencies
- **Blocks:** oxigeo (umbrella streaming), oxigeo-services (driver coverage).
- **Blocked by:** `oxiarc-archive` (KMZ), `oxisql-sqlite-compat` (GeoPackage — migrated off `rusqlite`, no C dependency), workspace `image` crate.

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-07-28 (status line refreshed: 80→156/98 tests, LoC 4,570→5,846, date bumped; JPEG2000 decoder, KML Placemark extraction, KMZ reading, and GML geometry parsing all re-verified against source and flipped to done — JPEG2000 and GML noted as delegated-to-sibling-crate / partial respectively; GeoPackage confirmed migrated from `rusqlite` to pure-Rust `oxisql-sqlite-compat`, correcting both the README's "GeoPackage format support (default)" claim — it is opt-in, not default — and this file's stale `rusqlite` cross-crate-dependency line)*
