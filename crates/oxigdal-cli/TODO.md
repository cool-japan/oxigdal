# TODO: oxigdal-cli

> **Purpose:** Command-line interface for OxiGDAL geospatial operations
> **Status (2026-05-16):** 13,397 Rust LoC · 258 tests · 3 real-code stubs (inspect/profile subcommands bail unconditionally; FlatGeobuf vector conversion unimplemented)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority
- [x] Implement actual raster I/O in `translate` command (currently stub)
- [x] Implement actual reprojection in `warp` command via oxigdal-proj
- [x] Wire `convert` command to real format drivers (GeoTIFF, GeoJSON, Shapefile, etc.) (done 2026-04-17)
  - **Goal:** Replace `anyhow::bail!("GeoTIFF conversion not yet implemented")` at `commands/convert.rs:121` with real GeoTIFF→GeoTIFF dispatcher honouring all CLI options (`--compression`, `--compression-level`, `--cog`, `--overviews`, `--tile-size`).
  - **Design:** Reuse `util::raster::read_raster_info` + `read_band_region` + `write_multi_band`. Add `write_raster_cog(path, &[RasterBuffer], ...)` helper via `CogWriter::create()`. If `--cog` → `write_raster_cog`; else → `write_multi_band`. Map CLI flags to `GeoTiffWriterOptions` / `CogWriterOptions`. Mark translate/warp TODO rows `[x]` (stale).
  - **Files:** `src/commands/convert.rs` (~60 new lines), `src/util/raster.rs` (`write_raster_cog` ~60 lines), `tests/convert_raster_integration.rs` (new, ~200 lines).
  - **Tests:** `test_convert_geotiff_identity`, `test_convert_cog_adds_overviews`, `test_convert_compression_deflate`, `test_convert_tile_size_respected`, `test_convert_rejects_unsupported_pair`.
- [x] Add `ogr2ogr`-equivalent vector conversion with attribute filtering (done 2026-04-17)
- [x] Implement `calc` band math expressions with proper parser
- [x] Add progress bar integration for long-running operations (indicatif 0.18)
- [x] Implement `merge` command with proper overlap handling and nodata merging
- [x] Add cloud URI support (s3://, gs://, az://) via oxigdal-rs3gw (done 2026-04-17)
  - **Goal:** CLI reads from `s3://`, `gs://`, `az://`, `file:///` URIs transparently. Cloud write stays deferred (graceful error).
  - **Design:** Add `oxigdal-rs3gw = { workspace = true }` to `Cargo.toml`. New `src/util/cloud.rs`: `open_datasource(uri: &str) -> anyhow::Result<Box<dyn DataSource>>` dispatcher — file:// or bare path → `FileDataSource`, cloud schemes → `parse_url` + `Rs3gwDataSource` via cached `OnceLock<tokio::runtime::Runtime>`. `is_cloud_uri(uri)` + `error_for_cloud_write(uri)`. Extend `util/raster.rs` with `read_raster_info_uri` + `read_band_region_uri`. Rewire `commands/{info,translate,warp,convert}.rs` input side to accept URIs; output side → `error_for_cloud_write` on cloud URIs.
  - **Files:** `Cargo.toml`, `src/util/cloud.rs` (new), `src/util/mod.rs`, `src/util/raster.rs`, `src/commands/info.rs`, `translate.rs`, `warp.rs`, `convert.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_open_datasource_file_path`, `test_open_datasource_file_uri`, `test_open_datasource_s3_parse_only`, `test_error_for_cloud_write_ergonomic`, `test_is_cloud_uri_classification`.
- [x] Wire `inspect` subcommand to a real file-inspector implementation (audit-discovered 2026-05-16)
  - **Goal:** Replace unconditional bail at `src/commands/inspect.rs:22-24` so that `oxigdal inspect <file>` actually reports format/size/extension/structure summary. Original implementation referenced the (now disabled) `oxigdal_dev_tools::inspector::FileInspector`.
  - **Verified gap:** `anyhow::bail!("Inspect command is not yet implemented. oxigdal_dev_tools crate is currently disabled.");` (commands/inspect.rs:22-24). The remainder of the function body (lines 26-52) is commented-out scaffolding awaiting the dev-tools crate.
  - **Design:**
    1. Re-implement `FileInspector` inline (or in `crates/oxigdal-cli/src/util/inspector.rs`) using `oxigdal::Dataset::open` + `oxigdal::is_cloud_uri` + `util::detect_format`.
    2. Surface: file path, file size (bytes), detected format, extension, magic-byte sniff result, top-level driver metadata (width/height/bands for raster; feature_count/bounds for vector), CRS string if available.
    3. `--detailed` flag triggers additional info: GeoTransform, NoData, band data types, layer count.
    4. JSON output: serialize a typed struct via `serde_json` for `--format json`.
  - **Files:**
    - `crates/oxigdal-cli/src/commands/inspect.rs` (replace function body; delete commented-out scaffolding).
    - `crates/oxigdal-cli/src/util/inspector.rs` (new, ~150 LoC — extracted reusable helper).
    - `crates/oxigdal-cli/src/util/mod.rs` (re-export).
    - `crates/oxigdal-cli/tests/cli_test.rs` (3 new integration tests).
  - **Tests:** `test_inspect_geotiff_summary`, `test_inspect_geojson_detailed_flag`, `test_inspect_nonexistent_errors`, `test_inspect_json_output_schema`.
  - **Prerequisites:** None — `oxigdal::Dataset::open` and format-detection helpers already work.
  - **Risk:** API drift from disabled `oxigdal_dev_tools` — reimplement minimally; do not resurrect the disabled crate unless explicitly asked.
  - **Done:** 2026-05-22 (Slice 27). New `src/util/inspector.rs` (~296 LoC): `InspectionReport`/`RasterSummary`/`VectorSummary` (`#[derive(Serialize)]`), `inspect_file(path, detailed) -> Result<InspectionReport>` — opens GeoTIFF via `GeoTiffReader` / GeoJSON via `GeoJsonReader` (the CLI has no `oxigdal::Dataset` umbrella type; it uses per-format crates, mirroring `commands/info.rs`), `crate::util::cloud::is_cloud_uri`, `crate::util::detect_format`. `commands/inspect.rs` body swapped from the `anyhow::bail!` stub to a real call; honours the global `OutputFormat` (Json → `serde_json::to_string_pretty`, Text → human-readable); `--detailed` fills geo_transform/nodata/data_types. `util/mod.rs` +3 re-export lines. ALSO fixed pre-existing breakage (a): `oxigdal-rs3gw/src/error.rs` non-exhaustive match on `rs3gw 0.2.1` `StorageError` (+8 lines — the `ObjectLocked`/`InvalidBucketState` arms) which blocked `cargo build -p oxigdal-cli` entirely. ALSO fixed newly-surfaced breakage (d): `oxigdal-cli/Cargo.toml` `oxigdal-proj` dep now sets `features = ["std"]` — the workspace declares `oxigdal-proj` with `default-features = false` (added after 0.1.4) but `oxigdal-proj` is not no_std-clean, so any consumer not re-enabling `std` fails to build; `oxigdal-geojson` already does this, `oxigdal-cli` now does too.
  - **Tests:** 9 in `crates/oxigdal-cli/tests/inspect_test.rs` (GeoTIFF summary; GeoJSON summary; detailed flag; nonexistent-file error; JSON output valid + has `format` key; file-size; extension; unknown-format no-panic). Build + clippy clean.
- [ ] Wire `profile` subcommand to a real micro-benchmark harness (audit-discovered 2026-05-16)
  - **Goal:** Replace unconditional bail at `src/commands/profile.rs:30-32` so that `oxigdal profile <op> <input> -n N` measures and reports wall-clock for repeated operations.
  - **Verified gap:** `anyhow::bail!("Profile command is not yet implemented. oxigdal_dev_tools crate is currently disabled.");` (commands/profile.rs:30-32). Commented-out scaffolding (lines 34-73) references `oxigdal_dev_tools::profiler::Profiler`.
  - **Design:**
    1. New `crates/oxigdal-cli/src/util/profiler.rs` providing `Profiler::new(name)`, `Profiler::start()`, `Profiler::stop()`, `Profiler::report()` (text + JSON), `Profiler::export_json()`.
    2. `Operation` enum: `OpenDataset`, `ReadAllBands`, `ReadFeatures`, `ComputeStats`. Map `args.operation` string → enum via `FromStr`.
    3. For each iteration: call `Operation::execute(&input)`; record `std::time::Instant`; accumulate min/max/mean/median/p95.
    4. JSON export via `serde_json` to `args.output` if provided.
  - **Files:**
    - `crates/oxigdal-cli/src/commands/profile.rs` (replace function body; delete commented-out scaffolding).
    - `crates/oxigdal-cli/src/util/profiler.rs` (new, ~200 LoC).
    - `crates/oxigdal-cli/src/util/mod.rs` (re-export).
    - `crates/oxigdal-cli/tests/cli_test.rs` (3 new integration tests).
  - **Tests:** `test_profile_open_dataset_iterations`, `test_profile_unknown_operation_errors`, `test_profile_export_json_schema`.
  - **Prerequisites:** None.
  - **Risk:** Don't over-engineer — this is a CLI convenience, not a replacement for `criterion`. Keep median + p95 reporting simple.
- [ ] Implement FlatGeobuf vector conversion in `util::vector::convert_vector` (audit-discovered 2026-05-16)
  - **Goal:** `oxigdal convert input.geojson output.fgb` (and shapefile→fgb, fgb→anything) should produce valid FlatGeobuf instead of bailing.
  - **Verified gap:** `anyhow::bail!("FlatGeobuf vector conversion is not yet implemented (input: {}, output: {})", input.display(), output.display())` (util/vector.rs:592-596). The match arm covers `(GeoJson, FlatGeobuf)`, `(Shapefile, FlatGeobuf)`, and `(FlatGeobuf, _)` — i.e. every direction involving FlatGeobuf is unimplemented.
  - **Design:**
    1. Add `oxigdal-flatgeobuf = { workspace = true }` to `crates/oxigdal-cli/Cargo.toml` under a new `fgb` feature (or unconditional if the umbrella always pulls it).
    2. New helpers in `util/vector.rs`: `convert_geojson_to_fgb`, `convert_shapefile_to_fgb`, `convert_fgb_to_geojson`, `convert_fgb_to_shapefile`, `convert_fgb_to_fgb` (identity copy).
    3. Reuse `oxigdal_flatgeobuf::{FgbReader, FgbWriter}` (verify API surface before sliding); plumb attribute filter through reader iteration.
    4. Replace the bail arm with explicit dispatch.
  - **Files:**
    - `crates/oxigdal-cli/Cargo.toml`.
    - `crates/oxigdal-cli/src/util/vector.rs` (replace lines 589-597 + add ~250 LoC of helpers).
    - `crates/oxigdal-cli/tests/vector_convert_test.rs` (add 5 new tests).
  - **Tests:** `test_convert_geojson_to_fgb_roundtrip`, `test_convert_shapefile_to_fgb_roundtrip`, `test_convert_fgb_to_geojson_roundtrip`, `test_convert_fgb_attribute_filter_applied`, `test_convert_fgb_identity_copy`.
  - **Prerequisites:** `oxigdal-flatgeobuf` reader+writer must be feature-complete (verify via `cargo doc -p oxigdal-flatgeobuf`); if not, slice this into "reader only" first.
  - **Risk:** FlatGeobuf spatial index is mandatory in spec — writer must compute Hilbert R-tree; ensure writer API doesn't require all features in-memory at once.

## Medium Priority
- [x] `tileindex` command for generating tile index shapefiles (done 2026-04-18)
  - **Goal:** `oxigdal tileindex *.tif tile_index.shp` writes a Shapefile where each feature = one input tile's footprint (geometry = bbox polygon, attributes = filename + resolution).
  - **Design:**
    - New `crates/oxigdal-cli/src/commands/tileindex.rs` (~220 LoC).
    - `pub struct TileIndexArgs { inputs: Vec<PathBuf>, output: PathBuf, src_field: String (default "location") }`.
    - Output format inferred from extension: `.shp` → Shapefile, `.geojson` → GeoJSON.
    - For each input: open via `oxigdal::Dataset::open`, extract bounds + CRS, build rectangle polygon, add feature.
    - Skip inputs that error (warn but continue); final summary line.
  - **Files:** `crates/oxigdal-cli/src/commands/tileindex.rs` (new), `commands/mod.rs`, `src/main.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_tileindex_two_tiffs_shapefile`, `test_tileindex_empty_input_errors`, `test_tileindex_geojson_output`.
  - **Risk:** Confirm Shapefile polygon write support; fall back to GeoJSON-only if unavailable.
- [x] Add `polygonize` command (raster to vector conversion)
- [x] Implement `buildvrt` with proper XML VRT generation and relative paths
- [x] Add `clip` subcommand for clipping rasters/vectors by geometry or bbox
- [x] `reproject` shorthand subcommand for CRS transformations (done 2026-04-18)
  - **Goal:** `oxigdal reproject input.tif output.tif --to EPSG:3857` as thin wrapper over `warp`.
  - **Design:**
    - New `crates/oxigdal-cli/src/commands/reproject.rs` (~80 LoC).
    - `pub struct ReprojectArgs { input: PathBuf, output: PathBuf, to: String, from: Option<String>, resampling: Resampling (default Bilinear), resolution: Option<f64> }`.
    - Builds `WarpArgs` internally and delegates to `warp::execute`.
  - **Files:** `src/commands/reproject.rs` (new), `commands/mod.rs`, `src/main.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_reproject_epsg_4326_to_3857`, `test_reproject_missing_to_flag_errors`.
  - **Risk:** Trivial; thin delegate.
- [x] `--co key=value` (creation options) flag for all output commands (done 2026-04-18)
  - **Goal:** `oxigdal convert input.tif output.tif --co COMPRESS=LZW --co PREDICTOR=2`.
  - **Design:**
    - New `crates/oxigdal-cli/src/util/creation_options.rs` (~150 LoC): `parse_key_value` + GDAL-key mapper.
    - Add `#[arg(long = "co", value_parser = parse_key_value)] pub creation_options: Vec<(String, String)>` to `ConvertArgs`, `TranslateArgs`, `WarpArgs`, `MergeArgs`, `BuildVrtArgs`.
    - Map `COMPRESS=LZW` → `Compression::Lzw`, `TILED=YES` → tile_size, `BIGTIFF=YES` → BigTIFF; unknown keys → `tracing::warn!` + forward.
  - **Files:** `src/util/creation_options.rs` (new), `util/mod.rs`, `commands/{convert,translate,warp,merge,buildvrt}.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_co_compress_lzw`, `test_co_multiple_flags`, `test_co_invalid_no_equals_errors`.
  - **Risk:** Mapping drift vs GDAL; document only what we map.
- [x] Add `stats` command for raster/vector statistics summary
- [ ] Add YAML/TOML config file support for batch processing
- [ ] Implement `diff` command for comparing two datasets

## Low Priority / Future
- [ ] Add interactive mode (REPL) for exploratory data analysis
- [ ] Implement pipeline mode (stdin/stdout chaining between commands)
- [x] Man page generation via `clap_mangen` (done 2026-04-18)
  - **Goal:** `oxigdal man --out /tmp/man/` writes a man page per subcommand.
  - **Design:**
    - Add `clap_mangen` to workspace `Cargo.toml` and `crates/oxigdal-cli/Cargo.toml`.
    - New `Man { out: PathBuf }` subcommand in `main.rs`.
    - `clap_mangen::Man::new(cmd).render(&mut file)` for each subcommand.
  - **Files:** workspace `Cargo.toml`, `crates/oxigdal-cli/Cargo.toml`, `src/main.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_man_page_generation_all_subcommands` — N files, each starts with `.TH`.
  - **Risk:** Minor. Workspace Cargo.toml edit is cross-cutting but CLI-ergonomics owns it.
- [x] Nushell completion generator (Fish already ships) (done 2026-04-18)
  - **Goal:** `oxigdal completions nushell` writes Nushell completion script.
  - **Design:**
    - Add `clap_complete_nushell` to workspace `Cargo.toml` and `crates/oxigdal-cli/Cargo.toml`.
    - Extend shell enum in `completions` subcommand; delegate to `clap_complete_nushell::Nushell`.
  - **Files:** workspace `Cargo.toml`, `crates/oxigdal-cli/Cargo.toml`, `src/main.rs`, `tests/cli_test.rs`.
  - **Tests:** `test_nushell_completions_nonempty` — non-empty output containing `export extern "oxigdal"`.
  - **Risk:** Minor.
- [ ] Implement `benchmark` command for format read/write performance comparison
- [ ] Add `serve` subcommand to launch a local tile server (OGC Tiles)
- [x] `--parallel N` global flag with configurable thread count (done 2026-04-18)
  - **Goal:** All parallelizable commands respect `--parallel N`; default = `rayon::current_num_threads()`.
  - **Design:**
    - Add `#[arg(long, global = true, default_value_t = rayon::current_num_threads())] pub parallel: usize` to `Cli`.
    - In `main.rs`: `rayon::ThreadPoolBuilder::new().num_threads(cli.parallel).build_global().ok()` (`.ok()` to swallow double-init).
    - Add `rayon.workspace = true` to `crates/oxigdal-cli/Cargo.toml` if absent.
  - **Files:** `src/main.rs`, `crates/oxigdal-cli/Cargo.toml`, `tests/cli_test.rs`.
  - **Tests:** `test_parallel_flag_default_uses_num_cpus`, `test_parallel_flag_set_to_one_works`.
  - **Risk:** `build_global` can only be called once; `.ok()` swallows the error in tests.

## Cross-crate dependencies
- **Blocks:** Downstream packaging (Homebrew, conda-forge, Debian) — CLI is end-user surface
- **Blocked by:** `oxigdal` umbrella (all driver dispatch flows through it), `oxigdal-rs3gw` (cloud input via S3/GCS/Azure URIs), `oxigdal-flatgeobuf` (for the pending FlatGeobuf conversion item above), `oxigdal-terrain` (DEM/contour/fillnodata commands)

---
*Last audited: 2026-05-16*
