# TODO: oxigeo (umbrella crate)

> **Purpose:** Pure Rust geospatial data abstraction library — the Rust alternative to GDAL
> **Status (2026-07-28):** 5,639 Rust LoC · 261 tests · 0 real-code stubs (all `stub/placeholder` mentions are benign doc/comment text)
> **Recent fix (2026-07-28):** GitHub issue #12 — GeoTIFF georeferencing tag values (`ModelPixelScaleTag`, `ModelTiepointTag`, `GeoKeyDirectoryTag`) stored out-of-line beyond the initial header peek window were silently dropped; `open.rs` now falls back to a direct offset seek so metadata is no longer lost. Regression test: `test_issue_12_far_offset_georeferencing`.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority
- [x] Implement actual raster band reading in Dataset (currently returns stub metadata) (planned 2026-04-17)
- [x] Wire Dataset::open() to real driver crates (TIFF IFD parsing wired into build_dataset_info() — reads width/height/bands/GeoTransform from TIFF headers; GeoJSON sniffing, 8 tests)
- [x] Add magic-byte format detection (not just file extension) (planned 2026-04-17)
- [x] Implement Dataset::create() for writing new datasets (DatasetWriter with set_dimensions/set_data_type/set_geo_transform/write_band/write_all_bands/finalize for GeoJSON and OXIG binary, 11 tests)
- [x] Add raster band iterator (read band data as typed arrays) (completed 2026-04-18)
  - Implemented `BandIter<'a>` + `Dataset::bands()` + `Dataset::read_band(idx)`.
  - `BandIter` implements `ExactSizeIterator`; each `next()` call reads one band lazily.
  - GeoTIFF dispatch via `read_band_geotiff`; other formats return `NotSupported`.
  - Tests: `test_bands_iter_single_band`, `test_bands_iter_out_of_range_errors`, `test_bands_iter_size_hint`.
- [x] Implement vector layer iterator (read features with geometry + attributes) (planned 2026-04-17)

## Medium Priority
- [x] Add Dataset::reproject() convenience method via oxigeo-proj (planned 2026-04-17)
- [x] Implement Dataset::clip() for subsetting by bounding box (planned 2026-04-17)
- [x] Add Dataset::convert() for format translation (GeoTIFF to GeoJSON, etc.) (completed 2026-04-18)
  - Added `ConversionOptions` + `Compression` enums; `Dataset::convert(output_path, target_format, options)`.
  - GeoTIFF→GeoTIFF identity copy; GeoJSON→GeoJSON file copy; unsupported pairs return `NotSupported`.
  - Validates conversion pair via `convert::can_convert` before attempting any I/O.
  - Tests: `test_dataset_convert_raster_identity`, `test_dataset_convert_unsupported_pair_errors`.
- [x] Implement cloud URI support in Dataset::open() (s3://, gs://, az://) (completed 2026-04-18)
  - New `cloud_detect.rs` module: `is_cloud_uri()` + `open_cloud_dataset()`.
  - `Dataset::open()` checks `is_cloud_uri` first; without `cloud` feature returns `NotSupported`.
  - `is_cloud_uri` exported as `oxigeo::is_cloud_uri`.
  - Tests: `test_cloud_uri_detection`, `test_open_bare_path_still_works`, `test_open_s3_uri_parses_without_cloud_feature`.
- [ ] Add async variants of open/read/write operations
- [x] Implement Dataset::info() with actual metadata parsing (not just stubs) (completed 2026-04-18)
  - Added `feature_count: Option<u64>` and `bounds: Option<BoundingBox>` to `DatasetInfo`.
  - `extract_geojson_info` now counts `"type":"Feature"` occurrences and parses top-level `"bbox"`.
  - Added `Dataset::feature_count()` and `Dataset::bounds()` accessors.
  - Tests: `test_info_geojson_populated`, `test_info_geojson_empty_collection`, `test_info_geojson_bbox_parsed`.
- [x] Add virtual raster (VRT) creation from multiple datasets (completed 2026-04-18)
  - New `vrt_builder.rs`: `build_vrt(sources, output, VrtOptions)` generates GDAL-compatible VRT XML.
  - `VrtResolution::{Average,Highest,Lowest,User(f64)}` + `VrtOptions` with `no_data`, `separate_bands`, `srcnodata`.
  - Computes union bbox, resolves pixel size, emits `<SimpleSource>` per source per band.
  - `Dataset::build_vrt()` delegates to the free function.
  - Tests: `test_build_vrt_single_source`, `test_build_vrt_two_tiffs_union_extent`, `test_build_vrt_empty_sources_errors`.
- [x] Implement feature-flag documentation with docsrs cfg annotations (completed 2026-04-18)
  - `#![cfg_attr(docsrs, feature(doc_cfg))]` already present at lib.rs:1; all existing re-exports already annotated.
  - All new items added this session carry `#[cfg_attr(docsrs, doc(cfg(feature = "X")))]`.
  - Added `[package.metadata.docs.rs]` to `Cargo.toml` with `features = [...]` covering all public features.
  - New items: `cloud_detect` module (no cfg annotation needed — always public), `gdal_compat` module (annotated), `vrt_builder` (always public), `ConversionOptions`, `BandIter`, `Compression`.
- [x] Add Dataset::statistics() for quick raster min/max/mean/stddev (planned 2026-04-17)
- [x] Wire real feature streaming for GeoPackage / GeoParquet / STAC in OpenedDataset::features() (planned 2026-05-07)
  - **Goal:** Replace FeatureStream::empty() stub at streaming.rs:607 for OpenedDataset::{GeoPackage, GeoParquet, Stac, Unknown}. Make dataset.features() work for all 5+1 supported feature drivers, matching stream_geojson_features / stream_shapefile_features / stream_flatgeobuf_features siblings.
  - **Design:**
    1. GeoPackage: use oxigeo-gpkg OxiGpkgReader; iterate gpkg_contents feature tables; SELECT geom + attrs; decode GPKG WKB header + body; honour FeatureStreamConfig::chunk_size.
    2. GeoParquet: build on Item 1 pushdown reader; iterate ParquetRecordBatchReaderBuilder::build() over RecordBatches; decode WKB column per row; build Feature.
    3. STAC: walk links[rel=item] via oxigeo-stac reader; each Item → Feature (geometry from item.geometry, properties from item.properties + flattened assets).
    4. Unknown: emit tracing::warn! once, return FeatureStream::empty().
    5. Pattern: per-driver streaming_<driver>.rs module, feature-gated; dispatch from streaming.rs.
  - **Files:**
    - `crates/oxigeo/src/streaming.rs` (replace stub arms ~line 607)
    - `crates/oxigeo/src/streaming_geopackage.rs` (new, feature-gate geopackage)
    - `crates/oxigeo/src/streaming_geoparquet.rs` (new, feature-gate geoparquet)
    - `crates/oxigeo/src/streaming_stac.rs` (new, feature-gate stac)
    - `crates/oxigeo/src/lib.rs` (mod declarations with #[cfg(feature)])
  - **Tests:** test_stream_geopackage_basic, test_stream_geopackage_multi_table, test_stream_geoparquet_basic, test_stream_geoparquet_with_pushdown_filter, test_stream_stac_item_collection, test_stream_stac_catalog_with_collection_items, test_stream_unknown_returns_empty, test_features_dispatch_exhaustive
  - **Risk:** GeoPackage/STAC readers may be eager (Vec-returning) — accept "lazily chunked" streaming, document, note as future refinement. Do NOT refactor reader APIs in this slice.

## Low Priority / Future
- [x] Add GDAL compatibility shim (GDALOpen, GDALClose function aliases) (completed 2026-04-18)
  - New `gdal_compat.rs` behind `gdal-compat = []` feature (default: off), `#[doc(hidden)]`.
  - Functions: `GDALAllRegister`, `GDALVersionInfo`, `GDALOpen`, `GDALOpenEx`, `GDALClose`, `GDALGetDatasetDriver`, `GDALGetRasterXSize`, `GDALGetRasterYSize`, `GDALGetRasterCount`, `GDALGetProjectionRef`, `GDALGetGeoTransform`.
  - `#[allow(non_snake_case)]` applied to the whole module.
  - Tests (unit in `gdal_compat.rs`): `test_gdal_compat_nonexistent_path_errors`, `test_gdal_compat_all_register_noop`, `test_gdal_compat_version_info`, `test_gdal_compat_open_close_tiff`.
  - Integration tests: `test_gdal_compat_version_and_register`, `test_gdal_compat_open_nonexistent_errors`.
- [ ] Implement Python bindings via PyO3 (oxigeo-python subcrate)
- [ ] Add WASM bindings for browser use (oxigeo-wasm already exists, integrate)
- [ ] Implement streaming read for datasets larger than memory
- [ ] Add dataset comparison (semantic diff between two datasets)
- [ ] Implement plugin system for user-defined format drivers
- [ ] Add comprehensive migration guide from GDAL C/Python to OxiGeo

## Cross-crate dependencies
- **Blocks:** None (umbrella crate — downstream consumers re-export from here)
- **Blocked by:** `oxigeo-core`, every driver crate (`oxigeo-geotiff`, `oxigeo-geojson`, `oxigeo-shapefile`, `oxigeo-geoparquet`, `oxigeo-gpkg`, `oxigeo-pmtiles`, `oxigeo-mbtiles`, `oxigeo-stac`, `oxigeo-flatgeobuf`, `oxigeo-jpeg2000`, `oxigeo-vrt`, `oxigeo-netcdf`, `oxigeo-hdf5`, `oxigeo-zarr`, `oxigeo-grib`, `oxigeo-terrain`, `oxigeo-copc`, `oxigeo-index`), advanced crates (`oxigeo-cloud`, `oxigeo-proj`, `oxigeo-algorithms`, `oxigeo-analytics`, `oxigeo-streaming`, `oxigeo-ml`, `oxigeo-gpu`, `oxigeo-server`, `oxigeo-temporal`, `oxigeo-services`)

---
*Last audited: 2026-07-28*
