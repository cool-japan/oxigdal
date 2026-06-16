# TODO: oxigdal-drivers-advanced

> **Purpose:** Advanced format drivers for OxiGDAL — JPEG2000 (JP2/J2K), GeoPackage (GPKG, SQLite-backed), KML/KMZ, GML — feature-gated for selective compile.
> **Status (2026-05-16):** 4,570 LoC · 80 tests · 2 real stubs (`jp2/codestream.rs:273` JPEG2000 decoder placeholder returns gray fill; `gpkg/spatial_index.rs:85` RTree built with placeholder bounds).
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real Pure-Rust JPEG2000 decoder
  - **Verified gap:** `src/jp2/codestream.rs:272-296` — `/// Decode to raw image data (simplified). / /// This is a placeholder for a full JPEG2000 decoder. … / let data = vec![128u8; size]; // Gray image as placeholder / tracing::warn!("JPEG2000 decoding is simplified - returning placeholder data ({}x{}, {} components)", …);`
  - **Goal:** Working JP2/J2K decoder in Pure Rust matching ISO/IEC 15444-1:2019 Part 1 (Annex A baseline): SIZ/COD/COC/QCD/QCC markers parsed, tile-parts assembled, DWT (CDF 9/7 lossy or CDF 5/3 lossless) inverse, EBCOT Tier-1 entropy decoder, dequantization, ICT/RCT colour-space inverse.
  - **Design:** Phased implementation:
    1. Marker parser (currently partial in `src/jp2/codestream.rs`) — finalize SIZ, COD, COC, QCD, QCC; expose `CodestreamHeader { num_components, decomposition_levels, code_block_width, code_block_height }` (lines 261-269 already populated with defaults; replace with parsed values from COD).
    2. EBCOT Tier-1 MQ-coder decoder (~600 LoC; reference: Taubman 2000 "EBCOT: Embedded Block Coding with Optimized Truncation").
    3. Inverse DWT — CDF 9/7 lifting (lossy) and CDF 5/3 lifting (lossless), per Annex F.3.
    4. ICT inverse (Y'CbCr → R'G'B') per Annex G.1.
  - **Files:** `crates/oxigdal-drivers-advanced/src/jp2/{codestream.rs, ebcot.rs (new), dwt.rs (new), color.rs (new)}`.
  - **Tests:** (proposed) `test_jp2_decode_5x5_grayscale_synthetic`, `test_jp2_decode_lossless_5_3_dwt_roundtrip`, `test_jp2_decode_lossy_9_7_psnr_above_30db`, `test_jp2_decode_geojp2_uuid_box_preserves_geokeys`, `test_jp2_decode_corrupt_marker_returns_error`, `test_jp2_decode_known_geojp2_fixture_matches_opj_baseline`.
  - **Risk:** Full decoder is multi-thousand-LoC; consider scoping to Profile-0 (simple JP2) for v0.2 and full Part-1 for v1.0. Cross-check output against `openjpeg` reference outputs (host-side only; never linked in Pure-Rust build).
  - **Prerequisites:** None.

- [x] GeoPackage RTree spatial index from real geometry bounds
  - Done: 2026-05-31 (Slice 28). Tests: 6 new (spatial_index_test) + 117 existing = 123 total.
  - **Verified gap:** `src/gpkg/spatial_index.rs:78-87` — `// Query all features with their bounding boxes / // This is a simplified version - in practice you'd extract bounds from WKB geometry / … // Placeholder bounds - real implementation would calculate from geometry / Ok((fid, 0.0, 0.0, 1.0, 1.0))`
  - **Goal:** Build the `rstar::RTree<GeomEntry>` from real per-feature envelopes decoded from the GeoPackage WKB geometry column (GPKG binary header per OGC GeoPackage 1.4 §2.1.3, followed by ISO 13249-3 WKB).
  - **Design:** Replace the `query_map` placeholder with `SELECT fid, geom FROM {table} WHERE geom IS NOT NULL`; for each row, parse the GPKG binary header (4-byte magic `GP`, version, flags, srs_id, optional 32/64-byte envelope), then read the WKB body. If the envelope flag is set, use the embedded envelope; else compute bounds by streaming WKB coordinates. Output `(fid, min_x, min_y, max_x, max_y)`.
  - **Files:** `crates/oxigdal-drivers-advanced/src/gpkg/spatial_index.rs` (replace lines 75-90), share WKB parser with `src/gpkg/geometry.rs`.
  - **Tests:** (proposed) `test_spatial_index_envelope_from_gpkg_header_flag`, `test_spatial_index_envelope_from_wkb_scan_when_flag_absent`, `test_spatial_index_handles_3d_xyz_geometry`, `test_spatial_index_skips_null_geom_rows`, `test_spatial_index_query_returns_intersecting_fids`.
  - **Risk:** WKB endianness byte handled correctly (`0x00`=big, `0x01`=little).
  - **Prerequisites:** None (WKB parser exists elsewhere in workspace; reuse).

- [ ] GeoPackage feature writing with WKB encoding
  - **Goal:** Symmetric writer for `gpkg_geometry_columns`/`gpkg_contents`-registered feature tables — accept `geo_types::Geometry`, prepend the GPKG binary header (with envelope), serialize WKB body, INSERT row including attribute columns.
  - **Design:** New `GeoPackageWriter { conn }` with `create_feature_table(name, columns, srs_id, geometry_type)` setting up the standard metadata rows (`gpkg_contents` `data_type='features'`, `gpkg_geometry_columns` entry), plus `insert_feature(table, attributes, geom)` encoding to WKB via `wkb::Writer`. Use `oxiarc-*` for any compression; do not introduce `flate2` or `bzip2`.
  - **Files:** `crates/oxigdal-drivers-advanced/src/gpkg/writer.rs` (new ~400 LoC).
  - **Tests:** (proposed) `test_gpkg_write_point_feature_roundtrip`, `test_gpkg_write_polygon_with_hole_roundtrip`, `test_gpkg_write_creates_metadata_rows`, `test_gpkg_write_envelope_flag_set`, `test_gpkg_write_3d_geometry_xyz_flag`.
  - **Risk:** GeoPackage 1.4 requires `application_id` PRAGMA `0x47504B47` (`GPKG`) and `user_version` `10400` — must set both.
  - **Prerequisites:** None.

- [ ] GeoPackage raster tile reading (tile matrix sets)
  - **Goal:** Read tile blobs from `gpkg_tile_matrix`-registered tables, decoding PNG/JPEG/WebP per the `image` crate (Pure Rust).
  - **Design:** Iterate `gpkg_tile_matrix_set` for the layer's extent + zoom-level matrix; for each requested `(zoom, x, y)`, `SELECT tile_data FROM {table} WHERE zoom_level=? AND tile_column=? AND tile_row=?`; sniff magic (PNG `89 50 4E 47`, JPEG `FF D8 FF`, WebP `52 49 46 46 ... 57 45 42 50`) and decode.
  - **Files:** `crates/oxigdal-drivers-advanced/src/gpkg/tiles.rs` (new).
  - **Tests:** (proposed) `test_gpkg_tiles_list_zoom_levels`, `test_gpkg_tiles_read_png_tile_decodes`, `test_gpkg_tiles_read_jpeg_tile_decodes`, `test_gpkg_tiles_read_webp_tile_decodes`, `test_gpkg_tiles_missing_tile_returns_none`.
  - **Risk:** WebP path requires the new lossless WebP encoder/decoder from the project (per project memory: "Implement lossless WebP encoding and integrate into image rendering pipeline" already landed).
  - **Prerequisites:** Workspace `image` crate version pin (already present).

- [ ] KML Placemark geometry extraction into OxiGDAL core types
  - **Goal:** Parse KML `<Placemark>` blocks and emit `geo_types::Geometry` (Point, LineString, Polygon, MultiGeometry), preserving the `<name>`, `<description>`, and `<ExtendedData>` SimpleData/Data fields as a property map.
  - **Design:** Extend the existing `quick-xml` SAX-mode walker in `src/kml/`; on `</Placemark>`, emit a `KmlFeature { geom: Geometry, properties: HashMap<String, JsonValue> }`. Per OGC KML 2.3 §10.1.
  - **Files:** `crates/oxigdal-drivers-advanced/src/kml/parser.rs` (extend), `src/kml/types.rs` (define `KmlFeature`).
  - **Tests:** (proposed) `test_kml_point_placemark_lat_lng`, `test_kml_linestring_coords_parsed`, `test_kml_polygon_with_inner_ring`, `test_kml_multigeometry`, `test_kml_extended_data_typed`, `test_kml_namespace_kml_v2_2_compatible`.
  - **Risk:** KML coordinates are `lon,lat,alt` (not `lat,lon`) — explicit doc + test.
  - **Prerequisites:** None.

- [ ] KMZ archive reading
  - **Goal:** Treat `.kmz` as an OxiARC-compatible ZIP container, extract the first `*.kml` file via `oxiarc-archive`, then route to the KML parser (Item 5).
  - **Design:** `read_kmz(path) -> KmlDocument` uses `oxiarc_archive::ZipReader::open(path)`; iterates entries; reads the first non-directory entry whose name ends in `.kml` (KMZ convention: `doc.kml` at archive root); decompresses to `Vec<u8>`; passes to `kml::read_kml`.
  - **Files:** `crates/oxigdal-drivers-advanced/src/kmz/mod.rs` (extend).
  - **Tests:** (proposed) `test_kmz_reads_doc_kml_at_root`, `test_kmz_reads_first_kml_if_no_doc_kml`, `test_kmz_resolves_relative_image_assets`, `test_kmz_corrupted_archive_errors`.
  - **Risk:** COOLJAPAN Pure-Rust policy: must use `oxiarc-archive`, never `zip` crate. (`Cargo.toml:31` already only uses `oxiarc-*`.)
  - **Prerequisites:** Item 5.

- [ ] GML geometry parsing (gml:Point, gml:Polygon, etc.)
  - **Goal:** Parse OGC GML 3.2 geometries — `gml:Point`, `gml:LineString`, `gml:Polygon` (`gml:exterior`/`gml:interior` rings), `gml:MultiSurface`, `gml:MultiCurve`, `gml:MultiPoint` — into `geo_types::Geometry`.
  - **Design:** SAX walker in `src/gml/parser.rs`; track namespace (`gml = "http://www.opengis.net/gml/3.2"`); on `</gml:Polygon>`, assemble exterior ring + interior rings from accumulated `gml:posList` or `gml:coordinates` text. Default axis order: GML 3.2 declares **lat, lon** for EPSG geographic CRSes — handle via `srsName` lookup.
  - **Files:** `crates/oxigdal-drivers-advanced/src/gml/parser.rs` (extend).
  - **Tests:** (proposed) `test_gml_point_lat_lon_swap_when_geographic_crs`, `test_gml_polygon_exterior_only`, `test_gml_polygon_with_holes`, `test_gml_multisurface_aggregates_polygons`, `test_gml_unknown_geometry_returns_error`, `test_gml_3_2_namespace_required`.
  - **Risk:** GML axis-order confusion — explicit test for `urn:ogc:def:crs:EPSG::4326` swap.
  - **Prerequisites:** None.

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
- **Blocks:** oxigdal (umbrella streaming), oxigdal-services (driver coverage).
- **Blocked by:** `oxiarc-archive` (KMZ), `rusqlite` 0.37 pin (GeoPackage), workspace `image` crate.

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*
