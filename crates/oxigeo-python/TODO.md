# TODO: oxigeo-python

> **Purpose:** PyO3 0.28 bindings exposing raster I/O, vector ops, and algorithms to Python with NumPy zero-copy where possible.
> **Status (2026-05-17):** 13,458 LoC · 78 #[test] attributes · 1 real-code TODO (numpy complex dtype).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Native complex dtype (`complex64`/`complex128`) for `RasterArray::to_numpy`
  - **Verified gap:** `src/numpy.rs:585-587` — `// For now, return as float array with interleaved real/imaginary` / `// TODO: Use proper complex dtype when pyo3 supports it` / `self.to_numpy_typed::<f64>(py)`
  - **Goal:** Return a `numpy.complex64` / `numpy.complex128` array directly instead of a flattened `float64` with interleaved real/imag — required for SAR products, radar interferometry, and Fourier-domain rasters.
  - **Design:** PyO3 0.28 has `numpy::Complex32` / `numpy::Complex64` as of `numpy` crate v0.27 (re-exported `num_complex::Complex<f32/f64>`). Use `PyArray2::<Complex64>::from_owned_array_bound` from a `ndarray::Array2<Complex64>` built from the existing interleaved buffer. Add a sniff step that detects `Complex32` / `Complex64` in the source `RasterDataType` enum (already exists in `oxigeo_core::types::RasterDataType::CFloat32`/`CFloat64`).
  - **Files:** `crates/oxigeo-python/src/numpy.rs` (`to_numpy_complex`); `crates/oxigeo-python/src/array.rs` (dtype mapping); `crates/oxigeo-python/oxigeo.pyi` (declare `complex64`/`complex128` return types).
  - **Tests:** (proposed) `test_to_numpy_complex64_roundtrip`, `test_to_numpy_complex128_dtype_matches`, `test_complex_array_real_imag_interleaved_matches_legacy`, `test_complex_array_arithmetic_in_python`.
  - **Risk:** `numpy` crate complex types depend on `num-complex` version alignment in the workspace — verify Cargo.lock before bumping.
  - **Prerequisites:** None — `RasterDataType` already has CFloat32/CFloat64.

- [ ] Promote `pyo3-asyncio` integration from TODO comment to real implementation
  - **Verified gap:** `crates/oxigeo-python/Cargo.toml:40-44` — `# TODO: Add async support when pyo3-asyncio 0.23 is released` / `# pyo3-asyncio = { version = "0.23", features = ["tokio-runtime"], optional = true }` / `# tokio = { workspace = true, optional = true }` / `# futures = { workspace = true, optional = true }` / `# async-trait = { workspace = true, optional = true }`
  - **Goal:** Expose `async def` functions to Python over `asyncio` for I/O-heavy operations (`open_raster_async`, `read_band_async`, `write_async`). Mirror the COG remote reader once landed.
  - **Design:** Switch to `pyo3-async-runtimes` (the maintained successor of pyo3-asyncio as of 2025-09; pyo3-asyncio is archived). Wire `tokio::runtime::Runtime` via `pyo3_async_runtimes::tokio::future_into_py`. Re-enable the dependency lines that are currently commented out. Add a `pyfunction` wrapper that takes an `asyncio.Future` and returns the awaitable. Keep async behind the existing (commented) `async` feature flag.
  - **Files:** `crates/oxigeo-python/Cargo.toml` (uncomment + change crate name); (new) `crates/oxigeo-python/src/async_io.rs`; modify `crates/oxigeo-python/src/dataset.rs` to expose `async fn` mirrors.
  - **Tests:** (proposed) `test_async_open_raster_returns_awaitable`, `test_async_read_band_concurrent_two_files`, `test_async_cancellation_propagates`, `test_async_no_runtime_panic_when_event_loop_missing`.
  - **Risk:** Tokio runtime inside maturin-built CPython extension can deadlock if the Python GIL holder calls back into Rust async — release the GIL with `py.allow_threads` around `block_on`.
  - **Prerequisites:** None — `pyo3-async-runtimes 0.28` is on crates.io.

- [ ] HTTP range-request COG reader exposed as `oxigeo.open(url)` for s3://, https://
  - **Verified gap:** Existing TODO line — `[ ] Add actual raster file I/O connecting to oxigeo-geotiff reader/writer` (partially done — `Dataset::open` works for local paths) and `[ ] Implement windowed reading API for processing rasters larger than memory`. No remote URL handling in `src/dataset.rs` today (verified — see `src/dataset.rs:1-80`).
  - **Goal:** `oxigeo.open("https://...")` and `oxigeo.open("s3://bucket/key")` return a Dataset backed by the same `AsyncDataSource` used by oxigeo-cloud, with windowed `ds.read_window(Window(col_off, row_off, w, h))` for >RAM rasters.
  - **Design:** Detect URL prefix in `Dataset::open` and dispatch to `oxigeo_cloud::HttpDataSource` (gated by the existing `cloud` feature in Cargo.toml). Provide a sync wrapper that runs a private `tokio::runtime::Runtime` inside the `Dataset` struct (one runtime per Python process, reused across calls). Windowed read API takes a Python `Window` namedtuple and returns a `ndarray::Array2` view.
  - **Files:** `crates/oxigeo-python/src/dataset.rs` (URL dispatch + windowed read); `crates/oxigeo-python/src/raster/operations.rs` (windowed plumbing); `crates/oxigeo-python/Cargo.toml` (enable `cloud` in default features once stable).
  - **Tests:** (proposed) `test_open_https_cog_returns_dataset`, `test_open_s3_raises_on_missing_credentials`, `test_read_window_returns_correct_shape`, `test_read_window_clamps_to_image_bounds`, `test_open_invalid_scheme_raises_value_error`.
  - **Risk:** Spawning a runtime per process risks deadlock under nested asyncio; use `Lazy<Runtime>` initialized at module import time.
  - **Prerequisites:** oxigeo-cloud `HttpDataSource` (already exists per workspace).

- [ ] Replace the EPSG parser regex matchstring with structured parse-and-error reporting
  - **Verified gap:** `src/raster/core_ops.rs:368` — `// Check for EPSG:XXXX format`; `src/raster/core_ops.rs:406` — `"Could not parse CRS string: {}. Expected EPSG:XXXX, WKT, or PROJ string"`; `src/raster/core_ops.rs:470` — `// Test EPSG:XXXX format`; `src/vector/helpers.rs:258` — `"CRS must be in format 'EPSG:XXXX', got '{}'"`. The CRS parser is silently lenient about leading zeroes, whitespace, and case ("epsg:4326" not accepted), and the error message uses `XXXX` placeholder which leaks to end users.
  - **Goal:** Robust `parse_crs("EPSG:4326" | "epsg:4326" | "EPSG : 4326" | "OGC:CRS84" | "<wkt2 string>" | "<proj4 string>")` with structured errors that name the supplied format and the first parse failure.
  - **Design:** Single dispatcher `enum CrsSpec { Epsg(u32), Ogc(String), Wkt(String), Proj4(String) }`. EPSG regex: `^\s*(?i:epsg)\s*:\s*([1-9]\d{0,5})\s*$`. WKT sniffed by `^\s*(GEOG|PROJ|COMPOUND|VERT|ENGINEERING|GEODETIC)CRS\b`. PROJ.4 by `^\s*\+proj=`. Map each variant through `oxigeo_proj::Crs::from_*`. Return `PyValueError` with a Hint message naming the detected format.
  - **Files:** `crates/oxigeo-python/src/raster/core_ops.rs` (replace stringly-typed parser); `crates/oxigeo-python/src/vector/helpers.rs` (delegate to shared parser); (new) `crates/oxigeo-python/src/crs_parse.rs` (single source of truth).
  - **Tests:** (proposed) `test_parse_crs_epsg_uppercase`, `test_parse_crs_epsg_lowercase_accepted`, `test_parse_crs_epsg_with_whitespace`, `test_parse_crs_ogc_crs84`, `test_parse_crs_wkt2_minimal`, `test_parse_crs_proj4`, `test_parse_crs_garbage_raises_with_format_hint`.
  - **Risk:** Changing the error message format may break Python tests that match exact strings — grep for `"format 'EPSG:XXXX'"` first.
  - **Prerequisites:** None.

- [ ] Auto-generate `oxigeo.pyi` type stubs from PyO3 signatures
  - **Verified gap:** Existing TODO — `[ ] Add type stub (.pyi) auto-generation for IDE completion support`. The current `oxigeo.pyi` (13.9K) is hand-maintained; drift from the actual Rust signatures is likely.
  - **Goal:** A `cargo xtask gen-pyi` step that re-emits `oxigeo.pyi` from PyO3 macros so the type stub never drifts. Run as a pre-publish gate.
  - **Design:** Use `pyo3-stub-gen 0.9` (the canonical generator; ships an attribute `#[gen_stub_pyfunction]` and a binary `pyo3-stub-gen`). Annotate every `#[pyfunction]` and `#[pyclass]`. Generated file lives at `crates/oxigeo-python/oxigeo.pyi` (overwrite). Add a CI guard that fails if `git diff` shows changes after running the generator.
  - **Files:** `crates/oxigeo-python/Cargo.toml` (add `pyo3-stub-gen` dep); annotate every `#[pyfunction]` site under `crates/oxigeo-python/src/`; (new) `crates/oxigeo-python/src/bin/gen_pyi.rs` runner.
  - **Tests:** (proposed) `test_gen_pyi_emits_oxigeo_pyi_with_open_signature`, `test_pyi_file_in_sync_with_macros` (integration), `test_open_signature_matches_rust`.
  - **Risk:** pyo3-stub-gen 0.9 requires Python 3.10+ at gen time but the runtime targets abi3-py39 — keep the generator out of the runtime feature graph.
  - **Prerequisites:** None.

## Medium Priority
- [ ] rasterio-compatible facade (`rasterio.open` → `oxigeo.open`)
  - **Goal:** Drop-in replacement layer for the most-used rasterio APIs (`open`, `Window`, `read`, `write`, `transform`, `crs`).
  - **Files:** (new) `crates/oxigeo-python/python/oxigeo/_rasterio_compat.py`.
  - **Why deferred:** Surface area is huge; build incrementally per real user request.

- [ ] geopandas `__geo_interface__` bidirectional conversion
  - **Goal:** Accept a GeoDataFrame in `oxigeo.write_geojson`; return GeoDataFrame from `read_geojson(as_gdf=True)`.
  - **Files:** `crates/oxigeo-python/src/vector/` (extend GeoJSON path).
  - **Why deferred:** Coordinated with item below (Arrow exchange).

- [ ] xarray DataArray adapter (`oxigeo.read(...).to_xarray()`)
  - **Goal:** Multi-band raster as a labeled (band, y, x) DataArray with CRS/transform on `.attrs`.
  - **Files:** (new) `crates/oxigeo-python/python/oxigeo/_xarray_adapter.py`.
  - **Why deferred:** Pure-Python wrapper; can ship without Rust changes.

- [ ] Point cloud LAS/LAZ bindings (via oxigeo-copc)
  - **Goal:** `oxigeo.open("file.las")` returns a Python PointCloud with NumPy structured array view.
  - **Files:** (new) `crates/oxigeo-python/src/pointcloud.rs`.
  - **Why deferred:** Lower demand than raster work.

- [ ] GeoPackage vector read/write bindings (via oxigeo-gpkg)
  - **Goal:** `oxigeo.read_gpkg(path, layer=...)` → Feature iterator, `write_gpkg(path, features, layer=...)`.
  - **Files:** (new) `crates/oxigeo-python/src/gpkg.rs`.
  - **Why deferred:** Pending oxigeo-gpkg API stabilization.

- [ ] Raster calculator NumPy-expression evaluation (`oxigeo.calc("A * 2 + B", A=arr1, B=arr2)`)
  - **Goal:** Lazy evaluation of NumPy ufunc expressions on out-of-core rasters.
  - **Files:** `crates/oxigeo-python/src/expression.rs` (extend; already 44.8K).
  - **Why deferred:** Expression core exists; needs xarray-style chunked execution.

- [ ] manylinux2014 + musllinux1_2 wheel matrix in CI (`maturin build --release --target ...`)
  - **Goal:** Pre-built wheels for `cp39-cp313` × `linux x86_64/aarch64` + `macos x86_64/arm64` + `windows x86_64`.
  - **Files:** `.github/workflows/pypi-publish.yml` (per CLAUDE.md, only pypi-publish.yml is allowed).
  - **Why deferred:** Blocked on stabilizing default feature set.

- [ ] matplotlib/folium visualization helpers (`oxigeo.plot(ds)`, `oxigeo.show_on_folium(ds)`)
  - **Goal:** Pure-Python helpers that consume an `oxigeo.Dataset` for quick map preview.
  - **Files:** (new) `crates/oxigeo-python/python/oxigeo/_viz.py`.
  - **Why deferred:** No Rust dependency.

## Low Priority / Future (one-liners)
- [ ] Jupyter magic commands `%oxigeo_load`, `%oxigeo_plot` (delegate to oxigeo-jupyter).
- [ ] Dask array backend for chunked out-of-core raster ops.
- [ ] STAC client `oxigeo.stac.search(catalog, bbox=...)`.
- [ ] scikit-learn `oxigeo.ml.RasterTransformer` for tabular pipelines.
- [ ] Apache Arrow zero-copy exchange (`oxigeo.to_arrow()` / `from_arrow()`).
- [ ] QGIS Processing provider plugin (`processing.run("oxigeo:hillshade", ...)`).
- [ ] conda-forge recipe (`feedstock` for `oxigeo-python`).

## Cross-crate dependencies
- **Blocks:** None directly (downstream is Python ecosystem).
- **Blocked by:** oxigeo-cloud (remote URL Dataset), oxigeo-proj (EPSG/OGC parser), oxigeo-copc (point cloud), oxigeo-gpkg (vector GPKG).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see MEMORY.md "pyo3 0.28 (recently migrated)" note)

---
*Last audited: 2026-05-17*
