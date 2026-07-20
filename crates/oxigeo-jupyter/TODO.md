# TODO: oxigeo-jupyter

> **Purpose:** Jupyter integration — magic commands, rich display, interactive widgets, plotters visualization. Cargo deps include `evcxr`.
> **Status (2026-05-17):** 3,120 LoC · 104 #[test] attributes (per MEMORY) · multiple scaffolding stubs in magic execute path (no `TODO/FIXME/stub` literals — stubs hide behind `(example)` placeholders).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace `(example)` placeholder responses in magic-command execution with real OxiGeo calls
  - **Verified gap:** `src/magic.rs:262` — `format!("CRS for '{}': EPSG:4326 (example)", dataset),`; `src/magic.rs:274` — `format!("Bounds for '{}': [0.0, 0.0, 1.0, 1.0] (example)", dataset),`; `src/magic.rs:288-289` — `format!("Statistics for '{}'{}: min=0.0, max=1.0, mean=0.5 (example)", dataset, band_str)`. Magic commands `%crs`, `%bounds`, `%stats` accept a dataset name, look it up in the namespace, then return constant strings tagged `(example)` instead of actual data.
  - **Goal:** `%crs raster` returns the dataset's real CRS via `oxigeo-proj`; `%bounds raster` returns true `(minx, miny, maxx, maxy)` from the dataset's GeoTransform + dimensions; `%stats raster [band]` returns real `min/max/mean/stddev/median` computed by `oxigeo_algorithms::statistics`.
  - **Design:** Change `Value::Path(PathBuf)` (the current namespace entry from `%load_raster`) to `Value::Dataset(Arc<oxigeo_core::Dataset>)`. In `%load_raster`, open the file via `oxigeo_geotiff::GeoTiffReader::open` and store the opened dataset rather than just the path. For `%crs`: pull `Crs` from `Dataset::crs()`. For `%bounds`: compute `[geotransform.x_at(0,0), .y_at(0,h), .x_at(w,0), .y_at(0,0)]`. For `%stats`: call `oxigeo_algorithms::statistics::band_statistics(dataset, band, BandStatsOptions::default())`. Mirror the rasterio API where reasonable.
  - **Files:** `crates/oxigeo-jupyter/src/magic.rs` (replace 3 `(example)` arms); `crates/oxigeo-jupyter/src/kernel.rs` (extend `Value` enum); (new) `crates/oxigeo-jupyter/src/dataset_value.rs`.
  - **Tests:** (proposed) `test_crs_magic_returns_real_epsg_for_geotiff_with_known_crs`, `test_bounds_magic_returns_real_bbox_within_tolerance`, `test_stats_magic_returns_real_min_max_for_known_raster`, `test_load_raster_opens_dataset_into_namespace`, `test_crs_magic_returns_err_for_missing_dataset`.
  - **Risk:** Changing `Value` enum is a public-API change; bump minor; existing `Value::Path` callers must migrate.
  - **Prerequisites:** None — oxigeo-geotiff, oxigeo-proj, oxigeo-algorithms::statistics are workspace deps.

- [ ] Wire `evcxr` integration so the kernel is actually a Jupyter kernel
  - **Verified gap:** `Cargo.toml:25` — `evcxr.workspace = true`, but `rg evcxr crates/oxigeo-jupyter/src` returns zero usages. The current `OxiGeoKernel::execute` (in `src/kernel.rs:128`) parses `let name = value` lines manually and stores results in a HashMap — this is not the Jupyter messaging protocol (v5.4 ZMQ multipart) and cannot be launched as a real `jupyter console --kernel oxigeo` kernel.
  - **Goal:** Either (a) integrate `evcxr_jupyter` as the actual kernel binary so users get a real Rust Jupyter kernel with OxiGeo preloaded, or (b) implement the Jupyter messaging protocol v5.4 directly via `zmq` for our custom kernel. Path (a) is recommended — evcxr already speaks ZMQ.
  - **Design:** Add an `oxigeo-jupyter-kernel` binary target that runs `evcxr_jupyter::run_jupyter_kernel(args)` with our preloaded extern crate list. Provide a `kernel.json` at install time pointing to this binary. Keep the existing `OxiGeoKernel` struct as a *namespace cache* for the REPL — rename to `OxiGeoContext` and downgrade from the kernel role. Implement `jupyter messaging protocol` v5.4 message types: `kernel_info_request`/`reply`, `execute_request`/`reply`, `display_data`, `comm_open`/`comm_msg`/`comm_close` for widgets.
  - **Files:** (new) `crates/oxigeo-jupyter/src/bin/oxigeo_kernel.rs`; (new) `crates/oxigeo-jupyter/kernel/kernel.json` (template); rename `crates/oxigeo-jupyter/src/kernel.rs` → keep but downgrade type.
  - **Tests:** (proposed) `test_kernel_info_reply_has_protocol_5_4`, `test_execute_request_returns_execute_reply_with_count`, `test_display_data_emits_image_png`, `test_kernel_install_writes_kernel_json` (integration).
  - **Risk:** evcxr 0.16+ requires Rust nightly for some features; verify stable-channel compatibility. May need to vendor evcxr-jupyter binary or call out to it.
  - **Prerequisites:** None.

- [ ] Implement Leaflet/MapLibre `MapWidget` rendering via Jupyter comm messages
  - **Verified gap:** Existing TODO line — `[ ] Add interactive map widget using Leaflet.js via comm messages`. `src/widgets.rs:Widget::render(&self) -> Result<String>` returns HTML (verified), but there's no comm-message bidirectional channel for the widget to receive updates from the kernel or send user-interaction events back.
  - **Goal:** `MapWidget` rendered in a Jupyter cell shows a real interactive Leaflet map (HTML/JS via `display_data` with `application/vnd.jupyter.widget-view+json`); panning/zooming user actions in the browser send `comm_msg` back to the kernel; Python/Rust code can call `widget.set_center(lat, lon)` to update the view.
  - **Design:** Use `ipywidgets`-compatible widget protocol: emit `application/vnd.jupyter.widget-state+json` payload (`widget_id`, `model_name="OxiGeoMapModel"`, initial state). Ship a small companion JS package `oxigeo-jupyter-widgets` (under `crates/oxigeo-jupyter/js/`) that registers the model + view. Backend `comm_open` creates the bidirectional channel; messages serialize `{ "method": "set_center", "lat": ..., "lng": ... }`. For static (non-Jupyter) rendering, fall back to a self-contained HTML string with embedded Leaflet from CDN.
  - **Files:** `crates/oxigeo-jupyter/src/widgets.rs` (extend `MapWidget::render` and add comm hooks); (new) `crates/oxigeo-jupyter/js/` (npm package scaffold); (new) `crates/oxigeo-jupyter/src/comm.rs` (comm-msg protocol).
  - **Tests:** (proposed) `test_map_widget_render_html_includes_leaflet_script`, `test_map_widget_comm_open_returns_widget_id`, `test_map_widget_set_center_emits_comm_msg`, `test_map_widget_receives_user_pan_event`.
  - **Risk:** Bundling a JS package alongside a Rust crate complicates publishing — keep it in a sibling `npm-publish.yml` flow (per CLAUDE.md, the only allowed npm yaml).
  - **Prerequisites:** Item 2 (real kernel) so comm messages have a transport.

- [ ] Rich HTML/SVG/PNG display for raster bands (RichDisplay implementation)
  - **Verified gap:** Existing TODO line — `[ ] Implement rich HTML/SVG display for raster band visualization`. `src/display.rs` (17.4K) declares `RichDisplay` and `DisplayData` types but `rg "image/png" crates/oxigeo-jupyter/src/display.rs` likely returns zero — verify the actual data emission path is wired to `plotters` (which is a workspace dep).
  - **Goal:** `dataset.display()` emits a real PNG image of the raster (using plotters' Bitmap backend or oxigeo-webp encoder) via `application/vnd.jupyter.display_data` with mimetype `image/png`. Multi-band rasters render as RGB composites; single-band rasters apply a colormap (viridis default).
  - **Design:** Implement `RichDisplay::for_raster(dataset: &Dataset, options: RasterDisplayOptions) -> DisplayData`. Internals: read the raster bands → optionally apply colormap from `oxigeo_algorithms::colormap` (existing) → use `plotters::backend::BitMapBackend` to draw → encode to PNG via `image` crate (already in deps) → base64-encode → wrap in `{ "image/png": "<base64>", "text/plain": "<W>x<H> raster" }`. Provide SVG path for vector overlays via `plotters::backend::SVGBackend`. Provide low-res "thumbnail" mode for large rasters.
  - **Files:** `crates/oxigeo-jupyter/src/display.rs` (real PNG/SVG emission); `crates/oxigeo-jupyter/src/plotting.rs` (14.0K, extend); (new) `crates/oxigeo-jupyter/src/raster_display.rs`.
  - **Tests:** (proposed) `test_raster_display_emits_png_mimetype`, `test_raster_display_colormap_viridis_for_single_band`, `test_raster_display_rgb_composite_for_3_band`, `test_raster_display_thumbnail_caps_at_512px`, `test_vector_display_emits_svg_path`.
  - **Risk:** Encoding large rasters to PNG is slow; gate behind a max-pixel-count and prompt user to thumbnail.
  - **Prerequisites:** None.

- [ ] `%load_raster` actually opens a real Dataset (not just stores the path)
  - **Verified gap:** `src/magic.rs:213-219` — `Self::LoadRaster { path, name } => { let var_name = name.as_deref().unwrap_or("raster"); namespace.insert(var_name.to_string(), Value::Path(path.into())); output.insert("text/plain".to_string(), format!("Loaded raster from '{}' into '{}'", path, var_name)); }`. The "load" is misleading — only the path is stored, no I/O happens.
  - **Goal:** `%load_raster /path/to/file.tif as elev` actually opens the file, validates it's a readable GeoTIFF, stores an `Arc<Dataset>` in the namespace, and reports `Loaded 1024x1024 Float32 raster from '...' into 'elev' (took 12.3 ms)`. Returns a structured error if the file is missing, corrupt, or has an unsupported format.
  - **Design:** Couples to Item 1 (real magic commands). Open the file via `oxigeo::open` dispatcher (multi-format) so it works for GeoTIFF/GeoParquet/GeoPackage. Time the open. Report shape + dtype. On error, return `JupyterError::Magic` with the underlying `OxiGeoError` chain.
  - **Files:** `crates/oxigeo-jupyter/src/magic.rs` (LoadRaster arm); `crates/oxigeo-jupyter/src/kernel.rs` (`Value::Dataset` variant); modify `Cargo.toml` to add `oxigeo-oxigeo` (umbrella) workspace dep for the multi-format dispatcher.
  - **Tests:** (proposed) `test_load_raster_opens_real_geotiff`, `test_load_raster_reports_dimensions_in_output`, `test_load_raster_nonexistent_returns_err`, `test_load_raster_corrupt_file_returns_err`, `test_load_raster_overwrites_existing_var`.
  - **Risk:** Adding the umbrella `oxigeo-oxigeo` dep creates a tighter coupling — verify no cycle.
  - **Prerequisites:** Item 1 (Value::Dataset variant).

## Medium Priority
- [ ] Inline histogram + statistics tile rendered alongside `display(raster)`
  - **Goal:** Combined PNG showing the raster preview + a histogram + summary stats card.
  - **Files:** `crates/oxigeo-jupyter/src/display.rs` (extend).
  - **Why deferred:** Pending Item 4 (rich display) landing first.

- [ ] `%crs_info <dataset>` extended magic with PROJ.4 / WKT2 / EPSG fields
  - **Goal:** A verbose CRS dump (vs. terse `%crs`) showing PROJ.4 string, WKT2, EPSG codes, axis order.
  - **Files:** `crates/oxigeo-jupyter/src/magic.rs` (new variant).
  - **Why deferred:** Quick win after Item 1.

- [ ] Interactive polygon-drawing widget for ROI selection
  - **Goal:** User draws a polygon on `MapWidget`; kernel receives the GeoJSON via comm.
  - **Files:** `crates/oxigeo-jupyter/src/widgets.rs` (add `PolygonDrawWidget`).
  - **Why deferred:** Pending Item 3.

- [ ] Side-by-side comparison widget (before/after processing)
  - **Goal:** Synced pan/zoom across two `MapWidget`s.
  - **Files:** `crates/oxigeo-jupyter/src/widgets.rs`.
  - **Why deferred:** Pending Item 3.

- [ ] Progress-bar widget with `indicatif`-style updates
  - **Goal:** Long-running ops emit `comm_msg` progress; widget renders a bar.
  - **Files:** `crates/oxigeo-jupyter/src/widgets.rs`.
  - **Why deferred:** Coupled to oxigeo-node progress-callback work.

- [ ] `%export` magic for saving result variables to GeoTIFF/GeoPackage/GeoJSON
  - **Goal:** `%export elev to output.tif as gtiff`.
  - **Files:** `crates/oxigeo-jupyter/src/magic.rs` (new variant).
  - **Why deferred:** Pending Item 1.

- [ ] Tab-completion for OxiGeo functions and dataset properties
  - **Goal:** `complete()` in `kernel.rs` extended with dataset property names (`.crs`, `.bounds`, `.bands`).
  - **Files:** `crates/oxigeo-jupyter/src/kernel.rs` (`complete` method).
  - **Why deferred:** Coordinated with Item 2 (real kernel).

- [ ] Kernel interrupt handling for cancelling long ops
  - **Goal:** Catch `SIGINT` (or `interrupt_request` on the control socket) and propagate to running ops.
  - **Files:** `crates/oxigeo-jupyter/src/kernel.rs` (and new bin target).
  - **Why deferred:** Coupled to Item 2.

## Low Priority / Future (one-liners)
- [ ] JupyterLab extension for a dedicated geospatial sidebar panel.
- [ ] Voila dashboard mode for sharing read-only notebooks.
- [ ] nbconvert exporter for geospatial reports (custom template).
- [ ] Real-Time Collaboration support via Jupyter RTC.
- [ ] GPU memory monitoring widget for oxigeo-gpu sessions.
- [ ] Automatic code generation from widget state changes (`%capture`).
- [ ] Google Colab integration (`oxigeo-colab` package).
- [ ] AWS SageMaker kernel image.

## Cross-crate dependencies
- **Blocks:** oxigeo-python (`%oxigeo_load` would delegate here).
- **Blocked by:** oxigeo-algorithms::statistics (band stats), oxigeo-proj (CRS strings), oxigeo-oxigeo umbrella (multi-format `open()`).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the kernel + widget overview; main TODO.md tracks the 33→60+ tests planned for v0.2.0 — current 104 #[test] count is above target already)

---
*Last audited: 2026-05-17*
