# TODO: oxigeo-drivers/vrt

> **Purpose:** VRT (Virtual Raster) driver for OxiGeo - Pure Rust GDAL reimplementation
> **Status (2026-05-16):** 4,889 Rust LoC (incl. tests) - 64 tests - 0 source-code stubs (mature; lazy reading, pixel functions, mosaic compositing all wired)
> **Roadmap:** v0.1.7 - v0.2.0 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [ ] Implement warped VRT for on-the-fly reprojection
  - **Verified gap:** `src/dataset.rs:266` declares enum variant `VrtSubclass::Warped`, but the reader path does not perform reprojection. `rg -n "warp.*read|reproject.*source|VrtSubclass::Warped" -g '*.rs' src/reader.rs` shows no warp logic in the read path. Warped VRTs are read but treated as plain VRTs.
  - **Goal:** A `<VRTDataset subClass="VRTWarpedDataset">` file applies the warp matrix from source CRS to destination CRS during `read_window` / `read_band`.
  - **Design:** Parse `<GDALWarpOptions>` from XML: `<Transformer>` (typically `<GenImgProjTransformer>` with `SrcGeoTransform`, `DstGeoTransform`, `SrcSRS`, `DstSRS`), `<ResampleAlg>` (`NearestNeighbour` / `Bilinear` / `Cubic` / `Lanczos`). For each destination pixel, inverse-transform to source pixel via `oxigeo-proj`, read source value with chosen resampler. Spec: GDAL VRT Warped subclass documentation.
  - **Files:** (new) `src/warp.rs`, `src/reader.rs` (dispatch on `VrtSubclass`), `src/xml.rs` (parse `GDALWarpOptions`), `Cargo.toml` (add `oxigeo-proj.workspace = true` under a `warp` feature)
  - **Tests:** (proposed) `test_warp_identity_transform`, `test_warp_wgs84_to_web_mercator_nearest`, `test_warp_bilinear_resampling`, `test_warp_round_trip_via_inverse`, `test_warp_xml_parse_genimgproj`
  - **Risk:** Resampler accuracy at edges; reuse `oxigeo-warp` crate if it exists, otherwise inline.
  - **Prerequisites:** `oxigeo-proj` and possibly `oxigeo-warp` if separate.

- [ ] Implement multi-source compositing priority / overlap resolution
  - **Verified gap:** `src/mosaic.rs` (13.7K) exports `BlendMode`, `CompositeParams`, `MosaicCompositor`, `MosaicPlanner`. The blend modes are listed, but the question is which one is exercised when sources overlap in `read_window`. `src/reader.rs:159` comment `// Composite data from all contributing sources (no pixel function)` shows compositing happens, but the actual blend strategy (first-wins vs last-wins vs alpha vs max) is not clearly user-configurable. `rg -n "BlendMode::|blend_mode" -g '*.rs' src/reader.rs` returns no hits - the reader does not honour the `BlendMode` enum.
  - **Goal:** When two sources overlap, the caller can choose: `BlendMode::FirstWins` (default; current behavior), `LastWins`, `Max`, `Min`, `Mean`, `AlphaBlend` (using nodata or alpha mask).
  - **Design:** Plumb `CompositeParams` from `VrtBand` into `VrtReader::read_window`. Replace the simple "overwrite as we iterate sources" with a per-pixel reducer keyed on `BlendMode`. Honour nodata: pixels matching source nodata are skipped in blending. Spec: GDAL `<ComplexSource>` `<NODATA>` semantics.
  - **Files:** `src/reader.rs`, `src/mosaic.rs`, `src/band.rs` (`VrtBand` needs `blend_mode: BlendMode` field), `src/xml.rs` (parse blend mode from XML attribute)
  - **Tests:** (proposed) `test_blend_first_wins_default`, `test_blend_max_overlapping_sources`, `test_blend_alpha_with_nodata_mask`, `test_blend_mean_two_sources`
  - **Risk:** Data-type-specific reducer (integer max differs from float max with NaN); use `oxigeo-core::RasterDataType` dispatch.
  - **Prerequisites:** None.

- [ ] VRT validation (source-file existence check, dimension consistency check)
  - **Verified gap:** `src/xml.rs` parses VRT XML into typed structures. `rg -n "fn validate|check.*existence|verify.*dim" -g '*.rs' src/` returns no validation entry point.
  - **Goal:** `VrtDataset::validate() -> Result<ValidationReport>` reports: missing source files, mismatched source dimensions, overlapping destination windows beyond expected blend zone, invalid pixel-function names.
  - **Design:** Walk `VrtDataset.bands`; for each `VrtSource`: check `source_filename.exists()` (absolute or relative path); open the source via `oxigeo-geotiff` lazily; compare source `width`/`height` to declared `<SrcRect>`; tabulate issues in `Vec<ValidationIssue>`.
  - **Files:** (new) `src/validate.rs`, `src/lib.rs`
  - **Tests:** (proposed) `test_validate_missing_source_reported`, `test_validate_dim_mismatch_reported`, `test_validate_unknown_pixel_function_reported`, `test_validate_clean_vrt_passes`
  - **Risk:** I/O during validation; offer `validate_dry` (no source open) and `validate_full` modes.
  - **Prerequisites:** None.

- [ ] GDAL VRT XML compatibility round-trip testing
  - **Verified gap:** No test file present that uses a GDAL-generated VRT as a fixture. `ls tests/` shows the test dir exists but `rg -n "gdal.*generated|reference.*vrt|gdal_translate" tests/` returns no matches.
  - **Goal:** Open VRT files emitted by GDAL `gdalbuildvrt` / `gdal_translate`; parse and re-emit producing semantically equivalent (not byte-equal - GDAL may reorder elements) XML.
  - **Design:** Bundle 5-10 small VRT fixtures generated by GDAL (committed to `tests/fixtures/`). Each test: open, read metadata, compare to expected, optionally re-write and re-open. No raster data needed - VRT XML alone.
  - **Files:** `tests/gdal_compat.rs` (new), `tests/fixtures/*.vrt` (committed; small text files)
  - **Tests:** (proposed) `test_gdal_buildvrt_simple_mosaic`, `test_gdal_translate_band_subset`, `test_gdal_complex_source_with_lut`, `test_gdal_warped_vrt`
  - **Risk:** GDAL XML uses ambiguous defaults (e.g., omitted SrcRect implies full source); document and handle.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] Pixel function: kernel-based (convolution, statistics in moving window)
  - **Goal:** Extend `PixelFunction` to accept kernel-style functions reading a window around each pixel.
  - **Files:** `src/band.rs`, `src/reader.rs`
  - **Why deferred:** Point-pixel functions already work (`src/reader.rs:243 apply_pixel_function`).

- [ ] Source band LRU caching for repeated tile access
  - **Goal:** Cache decoded source-band tiles across `read_window` calls.
  - **Files:** `src/reader.rs` (already uses `lru` crate per Cargo.toml)
  - **Why deferred:** Likely partial; needs audit.

- [ ] VRT from directory: auto-mosaic all GeoTIFFs in a folder
  - **Goal:** `MosaicBuilder::from_dir(path)` walks dir, sniffs each TIFF for geo info, builds grid.
  - **Files:** `src/builder.rs`
  - **Why deferred:** Convenience wrapper; users can do via existing API.

- [ ] Derived band: band-math expressions on source bands (e.g., NDVI = (NIR - RED)/(NIR + RED))
  - **Goal:** Compile a small expression DSL into a pixel function.
  - **Files:** (new) `src/derived.rs`
  - **Why deferred:** Requires expression parser; defer to v0.2.0.

- [ ] VRT update-in-place: add/remove sources without full rewrite
  - **Goal:** Mutate existing VRT XML and re-serialize.
  - **Files:** `src/builder.rs`, `src/xml.rs`
  - **Why deferred:** Workflow nicety.

- [ ] Multi-source nodata handling across boundaries (already partial via blend)
  - **Goal:** Honour each source's distinct nodata while compositing.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Folded into compositing item.

- [ ] Color table and color interpretation inheritance from sources
  - **Goal:** Propagate source band color tables / interpretations to VRT bands.
  - **Files:** `src/band.rs` (ColorTable exists per `src/lib.rs:138`)
  - **Why deferred:** Niche; some VRTs override.

- [ ] Overview-level VRT (source overview selection by resolution)
  - **Goal:** Pick the right overview level from source per requested resolution.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Requires overview API in source driver.

- [ ] Partial / window band reading on read API (already implemented as `read_window`; this entry is the documentation polish)
  - **Files:** `src/reader.rs`
  - **Why deferred:** Already works (verified `src/reader.rs:124`); just needs better user-facing docs.

## Low Priority / Future (speculative - concise)

- [ ] VRT for vector data (OGR VRT equivalent)
- [ ] VRT-based time series (temporal dimension from file list)
- [ ] Cloud-native VRT (reference HTTP/S3 sources with byte ranges)
- [ ] VRT diff (compare two VRT definitions)
- [ ] VRT optimization (merge adjacent sources, remove redundant bands)
- [ ] VRT-to-COG conversion (materialize virtual dataset as tiled GeoTIFF)
- [ ] Python-callable pixel functions via embedded interpreter
- [ ] VRT chaining (VRT referencing other VRTs - currently disallowed implicitly)

## Cross-crate dependencies
- **Blocks:** None directly.
- **Blocked by:** `oxigeo-proj` (for warp item only), `oxigeo-geotiff` (for source reading).

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries. The previous "Implement lazy tile reading from source rasters" and "Add pixel function evaluation" were already implemented and have been moved to the verified-as-done note below.)_

- [x] Lazy tile reading from source rasters — `VrtReader::read_window` at `src/reader.rs:124` reads only the requested window from each contributing source via `oxigeo_geotiff::read_band`; verified 2026-05-16.

- [x] Pixel function evaluation on source bands — `PixelFunction` struct (`src/band.rs`, re-exported `src/lib.rs:138`) plus `VrtReader::apply_pixel_function` at `src/reader.rs:243`; verified 2026-05-16.

---
*Last audited: 2026-05-16*
