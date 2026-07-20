# TODO: oxigdal-core

> **Purpose:** Core abstractions for OxiGDAL — Pure Rust GDAL reimplementation with zero-copy buffers and cloud-native support
> **Status (2026-05-16):** 13,527 Rust LoC · 341 tests · 0 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority
- [x] Add `RasterBuffer` typed accessors (get_f32, get_f64, get_u16, etc.) with bounds checking
- [x] Implement `Dataset` trait as a unified interface for raster and vector drivers (planned 2026-04-17)
  - **Goal:** Composable trait family in `oxigdal-core::io` giving the workspace one canonical geospatial dataset abstraction. Three traits: `Dataset` (identity/metadata), `RasterDataset: Dataset` (band access), `VectorDataset: Dataset` (feature stream).
  - **Design:** `Dataset`, `RasterDataset`, `VectorDataset` traits in new file `src/io/dataset.rs`. `FieldType` enum (Null/Bool/Integer/UInteger/Real/String/Blob/Date/Object/Array) in same file. All traits object-safe: `features()` returns `Box<dyn Iterator + '_>`. `read_band` takes `&mut self`. Default `features_in_bbox` filters on `features()`. Default `description` from driver_name + path.
  - **Files:** `crates/oxigdal-core/src/io/dataset.rs` (new, ~280 lines), `src/io/mod.rs` (add module + re-exports), `src/lib.rs` (re-export at io block).
  - **Tests:** `test_dataset_trait_object_safety`, `test_raster_dataset_fake`, `test_vector_dataset_fake`, `test_default_bounds_from_geotransform`.
- [x] Add `BandIterator` for lazy per-band iteration over raster data (MultiBandBuffer + BandRef + BandIterator with BSQ/BIP interleave support, 16 tests)
- [x] Implement `GeoTransform::inverse()` for pixel-to-coordinate mapping
- [x] Add batched `GeoTransform::world_to_pixel_many` / `pixel_to_world_many` helpers (planned 2026-04-17)
  - **Goal:** Deepen the GeoTransform API with batch-transform helpers suitable for SIMD autovectorization. `inverse()` already ships; this adds 4 batched methods.
  - **Design:** `pixel_to_world_many(&self, pixels: &[(f64,f64)]) -> Vec<(f64,f64)>`, `world_to_pixel_many(&self, world: &[(f64,f64)]) -> Result<Vec<(f64,f64)>>` (computes inverse once then tight loop), plus no-alloc `_into` variants.
  - **Files:** `crates/oxigdal-core/src/types/geo_transform.rs` (~80 new lines).
  - **Tests:** `test_pixel_to_world_many_roundtrip`, `test_world_to_pixel_many_matches_singleton`, `test_batch_singular_returns_err`, `test_batch_into_length_mismatch_errs`.
- [x] Add `PixelLayout::BandInterleaved` (BIP) and `PixelLayout::LineInterleaved` (BIL) support (planned 2026-04-17)
  - **Goal:** Complete BIL roundtrip on `MultiBandBuffer` — `from_bil`/`to_bil` methods. BIP already exists at `buffer/band_iterator.rs:122-174,315-347`. BSQ exists. BIL missing.
  - **Design:** `MultiBandBuffer::from_bil<T: BandPixel>(data, width, height, bands, data_type) -> Result<Self>` (input: row-major BIL layout → internal BSQ via stride math). `MultiBandBuffer::to_bil<T: BandPixel>(&self) -> Result<Vec<T>>`.
  - **Files:** `crates/oxigdal-core/src/buffer/band_iterator.rs` (~120 new lines + inline tests).
  - **Tests:** `test_from_bil_roundtrip` (2×3×4 u8), `test_from_bil_to_bsq_equivalence`, `test_from_bil_mismatched_size`, `test_from_bil_various_types` (u16/i32/f32/f64).
- [x] Implement `From<RasterBuffer>` for Arrow `RecordBatch` (arrow feature)
- [x] Implement reverse `RasterBuffer::from_arrow_array()` (completed 2026-05-16)
  - **Goal:** Round-trip Arrow → RasterBuffer to match the existing forward path (`From<RasterBuffer>` for `RecordBatch`). Public API at `crates/oxigdal-core/src/buffer/mod.rs:1088` currently bails with `OxiGdalError::NotSupported { operation: "Arrow array conversion" }`.
  - **Verified gap:** `// This is a simplified implementation\n// A full implementation would handle all Arrow types` (buffer/mod.rs:1089-1090) — the function signature is `pub fn from_arrow_array<A: Array>(_array: &A, _width: u64, _height: u64) -> Result<Self>` with all parameters underscore-prefixed (i.e. unused).
  - **Design:**
    1. Downcast `&A` via Arrow's `as_any()` to concrete typed arrays (`UInt8Array`, `UInt16Array`, `Int16Array`, `Int32Array`, `UInt32Array`, `Int64Array`, `Float32Array`, `Float64Array`).
    2. Dispatch on Arrow `DataType` → `RasterDataType` mapping; fail with `NotSupported` only for unsupported Arrow types (lists/structs/strings).
    3. Validate `width * height == array.len()`; reject mismatch via `InvalidParameter`.
    4. Copy values into a `RasterBuffer` of the matching `RasterDataType`. For nullable Arrow arrays, populate `nodata` from `array.nulls()` if present.
  - **Files:**
    - `crates/oxigdal-core/src/buffer/mod.rs` (replace stub at line 1088, ~80 new LoC).
    - `crates/oxigdal-core/src/buffer/arrow_convert.rs` (existing test module — add roundtrip tests).
  - **Tests:** `test_from_arrow_uint8_roundtrip`, `test_from_arrow_float32_roundtrip`, `test_from_arrow_float64_roundtrip`, `test_from_arrow_size_mismatch_errors`, `test_from_arrow_unsupported_type_errors`, `test_from_arrow_nullable_propagates_nodata`.
  - **Prerequisites:** None — typed Arrow arrays already imported in `arrow_convert.rs`.
  - **Risk:** Endianness — Arrow LE matches RasterBuffer's `to_ne_bytes` only on LE hosts; document constraint or always normalise. Nullable arrays: nodata sentinel selection per dtype (e.g. `f32::NAN`, `i32::MIN`).
- [x] Implement `RasterBuffer::fill_value()` for complex dtypes `CFloat32`/`CFloat64` (completed 2026-05-16)
  - **Goal:** `RasterBuffer::fill_value()` currently no-ops for complex pixel types at `buffer/mod.rs:235-238`. Fill with `(value, 0)` per-pair to honour the documented "fill with (value, 0)" intent.
  - **Verified gap:** `RasterDataType::CFloat32 | RasterDataType::CFloat64 => { // Complex types: fill with (value, 0) // This is a simplified implementation }` (buffer/mod.rs:235-238) — empty match arm; callers receive an uninitialised buffer.
  - **Design:**
    1. `CFloat32`: pack 8 bytes per pixel = `[real_f32_ne, imag_f32_ne(=0.0)]`. Loop `chunks_exact_mut(8)` writing the pair.
    2. `CFloat64`: pack 16 bytes per pixel = `[real_f64_ne, imag_f64_ne(=0.0)]`. Loop `chunks_exact_mut(16)`.
    3. Match existing pattern of other branches (e.g. `RasterDataType::Float32` at the same site).
  - **Files:** `crates/oxigdal-core/src/buffer/mod.rs` (replace lines 235-238, ~14 new LoC).
  - **Tests:** `test_fill_value_cfloat32_writes_real_zero_imag`, `test_fill_value_cfloat64_writes_real_zero_imag`.
  - **Prerequisites:** None.
  - **Risk:** Minor. Endianness uses `to_ne_bytes` per existing convention.
- [ ] Add `no_std` + `alloc` support for `RasterBuffer` (currently std-only internals)
- [x] Implement `SpatialReference` type wrapping CRS info for core-level reprojection awareness

## Medium Priority
- [x] Add `RasterWindow` type for sub-region reads without full-band allocation
- [x] Implement `VectorDataset` trait (open/iterate features/spatial filter)
- [x] Add `Feature` and `FieldValue` types to core for driver-agnostic vector access (planned 2026-04-17)
  - **Goal:** Consolidate `PropertyValue` (in `vector/feature.rs:124`) + 3 driver-local FieldValue enums into canonical `oxigdal_core::vector::FieldValue`. Hard rename across all 25 reference sites. No deprecation alias (incompatible with `-D warnings`).
  - **Design:** Rename `PropertyValue` → `FieldValue` in `feature.rs`. Add `Blob(Vec<u8>)` + `Date(time::Date)` variants. Add `#[cfg(feature = "serde")]` custom serde for new variants (Blob=base64, Date=ISO-8601). Add `FieldValue::to_json_value(&self) -> serde_json::Value`. `From<serde_json::Value> for FieldValue`. Hard rename across all 25 workspace files (grep-verified). Delete local enums in gpkg/shapefile/mobile driver crates.
  - **Files:** `vector/feature.rs` (rename+new variants), `vector/mod.rs`, `src/lib.rs`, `Cargo.toml` (time dep), 25 call-site files across workspace.
  - **Tests:** `test_fieldvalue_variants_exhaustive`, `test_fieldvalue_blob_roundtrip_serde`, `test_fieldvalue_date_serde_iso8601`, `test_fieldvalue_to_json_value_all_variants`, per-driver roundtrip tests.
- [x] Add `Geometry::to_wkb(&self) -> Vec<u8>` OGC-compliant WKB serialization (planned 2026-04-17)
  - **Goal:** `oxigdal_core::vector::Geometry` emits spec-compliant OGC WKB (ISO/IEC 13249-3) bytes for all 7 variants (Point/LineString/Polygon/Multi*/GeometryCollection), little-endian. Required by `oxigdal::streaming::StreamingFeature::wkb`.
  - **Design:** `Geometry::to_wkb(&self) -> Vec<u8>` (little-endian alloc+write) + `write_wkb<W: Write>(&self, w: &mut W) -> io::Result<()>` (streaming). Type codes: Point=1, LineString=2, Polygon=3, Multi*=4-6, GeometryCollection=7. Count prefixes: u32 LE. Coordinate payload: f64 LE X then Y. No auto-close on rings.
  - **Files:** `crates/oxigdal-core/src/vector/geometry.rs` (~220 new lines + tests).
  - **Tests:** `test_wkb_point_2d`, `test_wkb_linestring_empty`, `test_wkb_linestring_3_points`, `test_wkb_polygon_with_hole`, `test_wkb_multipoint_3_points`, `test_wkb_multilinestring_2_lines`, `test_wkb_multipolygon_2_polygons`, `test_wkb_geometrycollection_mixed`, `test_wkb_length_matches_spec`, `test_wkb_write_equals_to_wkb`.
- [x] Implement SIMD-accelerated buffer operations in `simd_buffer.rs` (currently scaffolding)
- [x] Add `TileIndex` and `TileIterator` types for standardized tiled access
- [x] Implement memory-mapped I/O path in `io::DataSource` for large files
- [x] Add `Statistics` struct (min/max/mean/stddev/histogram) to `RasterMetadata`
- [x] Implement `AsyncDataSource` trait for cloud-native async reading

## Low Priority / Future
- [x] Add `Mask` type for nodata/validity bitmask operations
- [x] Arena allocator integration for zero-alloc tile processing pipelines (planned 2026-04-18)
  - **Goal:** `TileIterator` can allocate output tiles from a caller-provided arena instead of heap, enabling zero-allocation streaming pipelines.
  - **Design:**
    - `crates/oxigdal-core/src/memory/arena.rs` (existing) — add `ArenaPool` with `checkout()` / `return_arena()`, `ArenaVec` helper.
    - In `crates/oxigdal-core/src/buffer/tile_iterator.rs`: add `TileIterator::with_arena<'a>(self, arena: &'a Arena) -> TileIteratorArena<'a>` yielding `ArenaTile<'a>`.
    - `TileIteratorArena` yields `ArenaTile<'a>` with `data: ArenaSlice<'a, u8>`. Dropping the arena reclaims all tiles at once.
  - **Files:**
    - `crates/oxigdal-core/src/memory/arena.rs` — extend with `ArenaPool`, `ArenaVec` (~120 new LoC).
    - `crates/oxigdal-core/src/buffer/tile_iterator.rs` — new `TileIteratorArena` type (~100 new LoC).
    - `crates/oxigdal-core/src/buffer/mod.rs` — re-export.
  - **Tests:** `test_arena_tile_iterator_yields_arena_tiles`, `test_arena_pool_checkout_return_reuse`, `test_arena_tile_iterator_drops_with_arena`.
  - **Risk:** Lifetimes. Arena slice lifetimes must outlive the iterator; tests catch at compile time.
- [ ] Add hugepage and NUMA-aware allocation policies for HPC workloads
- [x] `Display` and `Debug` formatting for all public types (planned 2026-04-18)
  - **Goal:** Every `pub` type in `oxigdal-core` has `Debug` and (where it makes sense) `Display`. User-facing types get `Display`.
  - **Design:**
    - Audit every `pub struct`/`pub enum` under `crates/oxigdal-core/src/**`.
    - Impl `Display` for: `BoundingBox`, `GeoTransform`, `Point2D`, `Point3D`, `FieldValue`, `FieldType`, `Geometry`, `NoDataValue`, `ColorInterpretation`, `RasterDataType`, `PixelLayout`, `RasterWindow`.
    - No changes to existing `Debug` where it's already there; only fill gaps.
  - **Files:**
    - `crates/oxigdal-core/src/types/*.rs` — 10–15 files, `impl Display` additions.
    - `crates/oxigdal-core/src/vector/{geometry,feature}.rs` — Display for `Geometry`, `FieldValue`, `FieldType`.
    - `crates/oxigdal-core/src/buffer/**/*.rs` — Debug coverage audit.
  - **Tests:** `test_display_bounding_box`, `test_display_geotransform`, `test_display_point2d`, `test_display_point3d`, `test_display_fieldvalue_each_variant`, `test_display_fieldtype_each_variant`, `test_display_geometry_point_wkt`, `test_display_nodata_value`, `test_display_colorinterpretation`, `test_display_rasterdatatype`, `test_display_pixellayout`, `test_display_rasterwindow`.
  - **Risk:** Small. Display format is stable API; lock it in tests.
- [ ] Add `serde` feature for serialization of metadata types
- [x] `ColorTable` type for palette-indexed raster support (planned 2026-04-18)
  - **Goal:** First-class palette / colormap type, usable by GeoTIFF / PMTiles / future MBTiles for indexed-color output.
  - **Design:**
    - New `crates/oxigdal-core/src/types/color_table.rs` (~220 LoC incl. tests).
    - `pub struct ColorTable { entries: Vec<ColorEntry>, interpretation: ColorInterpretation }`.
    - `pub struct ColorEntry { r: u8, g: u8, b: u8, a: u8 }` with `From<[u8; 4]>`, `From<(u8, u8, u8)>` (alpha = 255).
    - `pub enum ColorInterpretation { Rgba, Palette, Grayscale }`.
    - API: `new`, `with_capacity`, `push`, `get(index: usize) -> Option<&ColorEntry>`, `len`, `is_empty`, `as_slice`, `to_rgba_bytes(pixel: u8) -> [u8;4]`, `from_rgba_vec`, `grayscale_ramp(n: usize)` constructor.
    - `impl Default for ColorTable` → empty palette.
  - **Files:**
    - `crates/oxigdal-core/src/types/color_table.rs` (new, ~220 LoC).
    - `crates/oxigdal-core/src/types/mod.rs` — add `pub mod color_table; pub use color_table::{ColorTable, ColorEntry, ColorInterpretation};`.
    - `crates/oxigdal-core/src/lib.rs` — re-export at the `types` block.
  - **Tests:** `test_color_table_roundtrip`, `test_color_entry_from_tuples`, `test_to_rgba_bytes_indexed_u8`, `test_grayscale_ramp_linear`, `test_color_table_serde_roundtrip`.
  - **Risk:** Minor. No cross-crate dependencies.
- [ ] Add benchmark suite comparing buffer operations against raw slice ops

## Cross-crate dependencies
- **Blocks:** Every other crate in the workspace (78 crates depend on `oxigdal-core` for `RasterBuffer`, `Dataset`, `Result`/`Error`, `GeoTransform`, `BoundingBox`, `FieldValue`, `Geometry`, `DataSource`, etc.)
- **Blocked by:** None (foundation crate); pure Rust foundation libs only (`scirs2-core`, `oxiarc-*`, `oxicode`)

---
*Last audited: 2026-05-16*
