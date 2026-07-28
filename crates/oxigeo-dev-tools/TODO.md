# TODO: oxigeo-dev-tools

> **Purpose:** Developer utilities for OxiGeo — file inspection, profiling, benchmarking, debugging, test data generation, validation.
> **Status (2026-07-28):** 1,879 LoC · 186 tests (all-features and default-features equal) · 0 real-code stubs (GeoTIFF generator stub closed)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Replace `FileGenerator::generate_geotiff` stub with a real GeoTIFF writer
  - **Verified done:** `src/generator.rs:263-311` — `generate_geotiff(path, width, height)` now builds a real Float32 gradient raster, a north-up WGS84 `GeoTransform`, and writes it via `oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig}` with LZW compression and an EPSG:4326 code — no placeholder/gray-fill remains. `Cargo.toml` carries `oxigeo-geotiff.workspace = true` unconditionally (not behind a separate `geotiff` feature as originally sketched). Covered by `test_generate_geotiff_creates_nonempty_file` and `test_generate_geotiff_rejects_zero_dimensions` (`src/generator.rs:500,534`).
  - **Note:** shipped without the `pattern: RasterPattern` parameter the original design sketch proposed — always a linear gradient. COG compliance / round-trip-read-back / flat-pattern-specific tests were not added; revisit if a caller needs configurable patterns.

- [ ] Implement COG (Cloud-Optimized GeoTIFF) compliance validator
  - **Goal:** Given a GeoTIFF path, verify it satisfies COG spec v1.1: IFD ordered first (smallest tile size), tiled (not stripped), tile-size ≥ 256, overviews present (≥1 zoom level), overview ordering parent → children, RFC 1942/2483 directives. Output as `ValidationResult`.
  - **Design:** Open file as `oxigeo_geotiff::TiffReader`; enumerate IFDs; check (a) tile vs strip via `TileWidth`/`TileLength` tags; (b) IFD chronological order (file-offset ascending); (c) overview pyramid via `SubfileType` bit 0; (d) compression in {None, LZW, ZSTD, WebP, JPEG}. Emit `ValidationError` per rule failed (`category = Format`) and `ValidationWarning` for non-fatal (e.g., missing nodata).
  - **Files:** `src/validator.rs:165-306` (extend `DataValidator` with `validate_cog(&Path) -> ValidationResult`).
  - **Tests:** *(proposed)* `test_validate_cog_valid_passes`, `test_validate_cog_stripped_fails`, `test_validate_cog_missing_overviews_warns`, `test_validate_cog_small_tile_fails`, `test_validate_cog_unsupported_compression_warns`.
  - **Risk:** COG spec is informal; cite OGC 21-026 §6.3 in rustdoc as reference.
  - **Prerequisites:** Same `oxigeo-geotiff` dep as previous item.

- [ ] Implement GeoTIFF file format inspector (read IFD tags + overview structure)
  - **Goal:** Extend `FileInspector` to emit structured tag dump when `format == GeoTiff`: width, height, samples-per-pixel, bits-per-sample, compression, sample-format, tile vs strip, georef tags (ModelTiepoint, ModelPixelScale, GeoAsciiParams), overview count.
  - **Design:** Add `inspect_geotiff(path) -> Result<GeoTiffMetadata>` returning struct of tag values; integrate into `summary()` table via new rows when `format == GeoTiff`. Drive by `oxigeo_geotiff::TiffReader::metadata()`.
  - **Files:** `src/inspector.rs:77-201` (extend `FileInspector` with optional `geotiff_meta: Option<GeoTiffMetadata>` field; populate in `new()`).
  - **Tests:** *(proposed)* `test_inspect_geotiff_reads_dimensions`, `test_inspect_geotiff_reads_georef`, `test_inspect_geotiff_overview_count`, `test_inspect_non_geotiff_skips_extraction`.
  - **Risk:** None — `oxigeo-geotiff` API is mature.
  - **Prerequisites:** Same `oxigeo-geotiff` dep.

- [ ] Add statistical analysis (median, percentiles p50/p95/p99) to `Benchmarker`
  - **Goal:** `BenchmarkResult` already has mean/min/max/std-dev (`src/benchmarker.rs:21-40`). Add `median_ms`, `p50_ms`, `p95_ms`, `p99_ms`. Mean alone is misleading for skewed distributions; percentiles standard per criterion.rs conventions.
  - **Design:** After collecting `durations: Vec<i64>` (at `src/benchmarker.rs:78-85`), sort ascending. Compute median (linear interpolation between two middles); compute percentile via `(p/100 * (n-1))` index with linear interpolation. Extend `BenchmarkResult`. Update `report()` table at `:137-174` with new columns.
  - **Files:** `src/benchmarker.rs:21-174` (extend struct + bench + report).
  - **Tests:** *(proposed)* `test_benchmark_median_matches_known_data`, `test_benchmark_p95_p99_increasing`, `test_benchmark_skewed_data_median_lt_mean`, `test_benchmark_single_iteration_all_same_value`.
  - **Risk:** Backwards-compat — JSON export schema changes. Bump dev-tools `BenchmarkResult` version field or document as breaking.
  - **Prerequisites:** None.

- [ ] Add ASCII/terminal raster band visualization to `Debugger`
  - **Goal:** Render a 2D raster slice as Unicode block characters with brightness mapping; useful for terminal-only debugging.
  - **Design:** New `Debugger::render_raster_2d(&Array2<f64>, width: usize, height: usize) -> String`. Map each sample → 1 of `▁▂▃▄▅▆▇█` based on quantile; use ANSI 256-colour codes for elevation tints (via existing `colored` dep). Downsample to terminal width via box filter. Optional `--no-color` flag.
  - **Files:** `src/debugger.rs:100-433` (extend `Debugger` impl).
  - **Tests:** *(proposed)* `test_render_uniform_raster_single_glyph`, `test_render_gradient_increases_brightness`, `test_render_downsample_matches_target_width`, `test_render_nan_renders_as_question_mark`.
  - **Risk:** ANSI escapes confuse non-TTY consumers; auto-detect `is_terminal()` and disable colour when piped.
  - **Prerequisites:** None — `colored` already a dep.

## Medium Priority
- [ ] Profiler flamegraph-compatible output via existing `pprof` dep
  - **Goal:** `Profiler::start_flamegraph()` + `stop_flamegraph(path)` emits `.svg` via `pprof::ProfilerGuard::report().flamegraph(...)`.
  - **Files:** `src/profiler.rs:1-431` (existing, `pprof` already in `Cargo.toml`).
  - **Why deferred:** Sysinfo-based time/memory profiler in same file is functional; flamegraph is an add-on for CPU-bound profiling.

- [ ] Synthetic DEM generator (fractal terrain, Perlin noise)
  - **Goal:** Add to `DataGenerator` — diamond-square fractal (Fournier 1982) and Perlin/Simplex noise via `scirs2-core::random`.
  - **Files:** `src/generator.rs:9-130` (extend `DataGenerator`).
  - **Why deferred:** Existing `RasterPattern::Sine`/`Noise` cover most test needs; fractal DEM is more realistic but not blocking.

- [ ] CRS diagnostic tool (validate EPSG codes, show projection parameters)
  - **Goal:** Given an EPSG code, look up via `oxigeo-proj` and emit human-readable PROJ string + axis info + units.
  - **Files:** `src/validator.rs` (new method).
  - **Why deferred:** Use cases overlap with `oxigeo-cli`; defer until tooling consolidation.

- [ ] Format comparison tool (diff two GeoTIFFs pixel-by-pixel with tolerance)
  - **Goal:** `DataValidator::diff_rasters(a, b, tolerance) -> ValidationResult` emitting count of pixels exceeding tolerance.
  - **Files:** `src/validator.rs`.
  - **Why deferred:** Requires GeoTIFF reader integration (overlap with inspector task).

- [ ] Memory profiling helpers (track peak allocation per operation)
  - **Goal:** Wrap a closure with allocator tracking via `dhat` or `peak_alloc`; integrate into `ProfileSession.metrics`.
  - **Files:** `src/profiler.rs`.
  - **Why deferred:** sysinfo RSS tracking already approximates this; dedicated tracking allocator changes global state.

- [ ] Test fixture management (download/cache standard test datasets)
  - **Goal:** Function to fetch & cache canonical test files (NaturalEarth, Landsat sample, Sentinel-2 sample) under XDG cache dir.
  - **Files:** `src/generator.rs`.
  - **Why deferred:** Pulls in `reqwest`/network; opt-in feature.

- [ ] STAC catalog generator for test collections
  - **Goal:** Emit minimal STAC v1.0 catalog/collection/item JSON files for testing oxigeo-stac.
  - **Files:** `src/generator.rs`.
  - **Why deferred:** oxigeo-stac has its own builders; duplication risk.

## Low Priority / Future (one-liners)
- [ ] Interactive data explorer (TUI with ratatui)
- [ ] Documentation generator (extract examples from code, run them)
- [ ] Performance regression detection (compare benchmarks across commits via git)
- [ ] Fuzzing harness generator for format parsers (cargo-fuzz scaffolding)
- [ ] Dependency graph visualizer for the workspace (DOT/Mermaid)
- [ ] CI helper (generate test matrix, coverage reports)
- [ ] Random vector feature generator (lines, polygons with attributes — current is points-only)
- [ ] Regression test harness (compare outputs against golden files with tolerance)
- [ ] File size estimator for format conversion planning

## Cross-crate dependencies
- **Blocks:** None
- **Blocked by:** oxigeo-geotiff (for GeoTIFF writer, inspector, COG validator — all three High Priority items need it)

## Recently completed (verbatim)
- [x] FileInspector with magic-byte + extension format detection (GeoTIFF, GeoJSON, Shapefile, Zarr, NetCDF, HDF5, GeoParquet, FlatGeobuf) — `src/inspector.rs` (275 LoC)
- [x] Profiler with sysinfo-backed memory/CPU tracking + session collection — `src/profiler.rs` (431 LoC)
- [x] DataValidator with raster-dim, bounds, range, file-path checks — `src/validator.rs` (365 LoC)
- [x] DataGenerator with raster patterns (Flat, Gradient, Checkerboard, Noise, Sine) + point/grid generators — `src/generator.rs` (359 LoC)
- [x] FileGenerator for GeoJSON (functional) — `src/generator.rs:233-258`
- [x] Benchmarker with warmup/iterations/comparison/JSON export — `src/benchmarker.rs` (408 LoC)
- [x] Timer + TimerResult for quick measurements — `src/benchmarker.rs:243+`
- [x] Debugger with level-filtered DebugMessage collection + colored formatting — `src/debugger.rs` (433 LoC)
- [x] DevToolsError variants with `#[from]` IO/serde/oxigeo-core conversions — `src/lib.rs:49-82`

---
*Last audited: 2026-07-28 (status line refreshed: 43→186 tests, LoC 2,353→1,879, date bumped; `generate_geotiff` confirmed implemented via source read and flipped to done; COG validator, GeoTIFF tag inspector, benchmarker percentiles, and debugger raster visualization all re-checked and confirmed still absent — left open)*
