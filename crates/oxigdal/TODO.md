# TODO: oxigdal (umbrella crate)

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
- [x] Add Dataset::reproject() convenience method via oxigdal-proj (planned 2026-04-17)
- [x] Implement Dataset::clip() for subsetting by bounding box (planned 2026-04-17)
- [x] Add Dataset::convert() for format translation (GeoTIFF to GeoJSON, etc.) (completed 2026-04-18)
  - Added `ConversionOptions` + `Compression` enums; `Dataset::convert(output_path, target_format, options)`.
  - GeoTIFF→GeoTIFF identity copy; GeoJSON→GeoJSON file copy; unsupported pairs return `NotSupported`.
  - Validates conversion pair via `convert::can_convert` before attempting any I/O.
  - Tests: `test_dataset_convert_raster_identity`, `test_dataset_convert_unsupported_pair_errors`.
- [x] Implement cloud URI support in Dataset::open() (s3://, gs://, az://) (completed 2026-04-18)
  - New `cloud_detect.rs` module: `is_cloud_uri()` + `open_cloud_dataset()`.
  - `Dataset::open()` checks `is_cloud_uri` first; without `cloud` feature returns `NotSupported`.
  - `is_cloud_uri` exported as `oxigdal::is_cloud_uri`.
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

## Low Priority / Future
- [x] Add GDAL compatibility shim (GDALOpen, GDALClose function aliases) (completed 2026-04-18)
  - New `gdal_compat.rs` behind `gdal-compat = []` feature (default: off), `#[doc(hidden)]`.
  - Functions: `GDALAllRegister`, `GDALVersionInfo`, `GDALOpen`, `GDALOpenEx`, `GDALClose`, `GDALGetDatasetDriver`, `GDALGetRasterXSize`, `GDALGetRasterYSize`, `GDALGetRasterCount`, `GDALGetProjectionRef`, `GDALGetGeoTransform`.
  - `#[allow(non_snake_case)]` applied to the whole module.
  - Tests (unit in `gdal_compat.rs`): `test_gdal_compat_nonexistent_path_errors`, `test_gdal_compat_all_register_noop`, `test_gdal_compat_version_info`, `test_gdal_compat_open_close_tiff`.
  - Integration tests: `test_gdal_compat_version_and_register`, `test_gdal_compat_open_nonexistent_errors`.
- [ ] Implement Python bindings via PyO3 (oxigdal-python subcrate)
- [ ] Add WASM bindings for browser use (oxigdal-wasm already exists, integrate)
- [ ] Implement streaming read for datasets larger than memory
- [ ] Add dataset comparison (semantic diff between two datasets)
- [ ] Implement plugin system for user-defined format drivers
- [ ] Add comprehensive migration guide from GDAL C/Python to OxiGDAL
