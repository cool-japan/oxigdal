# TODO: oxigeo-drivers/netcdf

> **Purpose:** NetCDF driver for OxiGeo - Pure Rust NetCDF-4 (HDF5, via `oxinetcdf`/`oxih5`) by default, with optional Pure Rust NetCDF-3 support
> **Status (2026-07-28):** 137 tests (all-features) / 88 tests (default-features), 0 failed - NetCDF-4 read/write now wired via `oxinetcdf` (no C deps, no feature gate) - variable slicing API remains the main verified gap
> **Roadmap:** v0.1.7 - v0.2.1 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Implement NetCDF-4 reader metadata extraction (currently returns `NetCdf4NotAvailable`)
  - **Resolved (2026-07-28):** Superseded by a different design than originally sketched below — instead of wiring the crate's internal experimental `src/netcdf4/` HDF5 parser (`Nc4Reader`, still non-functional and explicitly marked "do not use for real work"), NC4 support now comes from the external Pure-Rust `oxinetcdf`/`oxih5` crates. `NetCdfReader::open` (`src/reader.rs:88`) auto-detects and reads both NC3 and NC4 transparently, returning the same `NetCdfMetadata` shape either way; see `src/lib.rs` module docs for the architecture note. The design/verified-gap text below is kept for history but no longer reflects the current code path.
  - **Verified gap (historical):** `src/reader.rs:221` - `// NetCDF-4 support is placeholder for now` followed by `Err(NetCdfError::NetCdf4NotAvailable)`. The function `read_metadata_nc4` is the only entry point for NC4 reads and always fails. Crate has its own Pure-Rust HDF5 parser in `src/netcdf4/` (look at module exports in `src/lib.rs:248-251`: `Hdf5Superblock`, `Hdf5SuperblockVersion`, `Nc4Reader`, etc.) so the building blocks exist; they just are not glued into the high-level `NetCdfReader` path.
  - **Goal:** Opening an NC4 file (HDF5-backed) returns the same `NetCdfMetadata` shape as an NC3 file: dimensions list, variables list (including data type, dims, attributes), global attributes, CF metadata.
  - **Design:** Wire `Nc4Reader::open(path)` (already in `src/netcdf4/`) into `NetCdfReader`. Map HDF5 group hierarchy: groups -> Nc4Group, datasets -> Variable, dataset attributes -> Attribute. NetCDF-4 conventions on top of HDF5: dimensions encoded as `_DIM_*` datasets or via NetCDF4 dimension scales (HDF5 Spec Dimension Scale Conv §H5DS). Initial pass: flat (root group only); group hierarchy follow-up.
  - **Files:** `src/reader.rs` (rewrite `read_metadata_nc4`), `src/netcdf4/mod.rs` (expose required `Nc4Reader` API)
  - **Tests:** (proposed) `test_nc4_read_simple_dims`, `test_nc4_read_variable_attributes`, `test_nc4_read_global_attributes`, `test_nc4_read_with_chunked_variable`, `test_nc4_read_with_deflate_compression`
  - **Risk:** Pure-Rust HDF5 parser at `src/netcdf4/` may not yet cover all dataset layouts (chunked + filtered); items below depend on this.
  - **Prerequisites:** None (parser lives in same crate).

- [x] Implement NetCDF-4 writer (currently returns `NetCdf4NotAvailable`)
  - **Resolved (2026-07-28):** Same story as the reader item above — `NetCdfWriter::create_netcdf4`/`create` (`src/writer.rs:60,110`) writes real NC4/HDF5 files via `oxinetcdf::NcFileWriter`, not via the internal `src/netcdf4/` module. Writing is currently flat (root group only) — `oxinetcdf::NcFileWriter` has no group-scoped `def_var` API yet (see comment at `src/reader.rs:1181`) — and per-variable compression is not exposed through the public `NetCdfWriter` API despite `oxiarc-deflate` being a dependency.
  - **Verified gap (historical):** `src/writer.rs:116` - `// NetCDF-4 support is placeholder for now`. Symmetric to the reader gap.
  - **Goal:** Create new NC4 files with dimensions, variables, attributes; deflate compression supported (via `oxiarc-deflate` already in Cargo.toml).
  - **Design:** Use `Nc4Writer` (already exported in `src/lib.rs:250`); wire `NetCdfWriter::create_nc4` through it. Write minimal HDF5 superblock V0, root group, datasets per `add_variable` calls. Dimension scales attached to each dim coordinate per NetCDF-4 convention. Compression via `oxiarc-deflate`.
  - **Files:** `src/writer.rs` (rewrite NC4 path), `src/netcdf4/mod.rs`
  - **Tests:** (proposed) `test_nc4_write_minimal_file`, `test_nc4_write_with_dimensions`, `test_nc4_write_deflate_compressed`, `test_nc4_round_trip_with_external_reader_ncdump_skip_if_not_available`
  - **Risk:** Same as reader: depends on internal HDF5 implementation's completeness.
  - **Prerequisites:** Reader item above (testing requires reading back what was written).

- [ ] Variable slicing API: read sub-regions with start/count/stride
  - **Verified gap:** `src/reader.rs:231` defines `read_f32(var_name)` returning the entire `Vec<f32>`; no `read_f32_slice(var_name, start, count, stride)` exists. `rg -n "fn read_.*_slice|fn read_subset|fn read_hyperslab" -g '*.rs' src/` returns no hits.
  - **Goal:** Caller can read e.g. `temperature[0..10, 50:100, ::]` without materialising the full grid.
  - **Design:** Add `read_f32_slice(var: &str, start: &[usize], count: &[usize], stride: Option<&[usize]>) -> Result<Vec<f32>>`. NC3 path: compute file offsets for each row (NC3 stores variables in contiguous row-major order; `var_offset + sum(start_i * product(dim_size[i+1..]))`); read directly. NC4 path: HDF5 hyperslab dataspace selection. Spec: NetCDF Users Guide §4.7 (variables - reading subsets).
  - **Files:** `src/reader.rs` (add slice variants for f32/f64/i32/i16/i8), `src/nc3_compat.rs` (offset arithmetic)
  - **Tests:** (proposed) `test_slice_nc3_first_row`, `test_slice_nc3_2d_strided`, `test_slice_nc3_unlimited_dim`, `test_slice_nc4_hyperslab` (gated)
  - **Risk:** NC3 unlimited-dim record stride is variable_size, not 1; verify against NetCDF Users Guide §4.7.2.
  - **Prerequisites:** Helpful but not strictly blocked by NC4 items.

- [x] CF-1.8 coordinate variable auto-detection and axis interpretation
  - **Resolved (2026-07-28):** `CoordinateDetector::detect_axis` (`src/cf_conventions/coordinates.rs:152`) implements exactly the priority order sketched below: (1) explicit `axis` attribute, (2) `standard_name` (latitude/longitude/time/depth/height/altitude/air_pressure), (3) `units` tables (latitude/longitude/time-prefix/vertical), (4) `positive` attribute for vertical. `is_coordinate_variable` also implemented.
  - **Verified gap (historical):** `src/cf_conventions/` exists (verified via `ls`), but `rg -n "fn .*axis|standard_name|positive" -g '*.rs' src/cf_conventions/` shows it primarily parses global title/institution/Conventions strings, not coordinate-axis classification. The High-Priority TODO claim "Add CF-1.8 coordinate variable auto-detection" is correct.
  - **Goal:** Given a NetCDF file, classify each variable as X/Y/Z/T axis based on CF conventions (`axis` attribute, `standard_name`, `units`, `positive`).
  - **Design:** Priority order per CF §5.6: (1) explicit `axis` attribute (`"X"`, `"Y"`, `"Z"`, `"T"`); (2) `standard_name` recognition (`"longitude"`/`"latitude"`/etc.); (3) units detection (`udunits2` compatible parser - simplified internal table for `"degrees_east"`, `"degrees_north"`, `"Pa"`/`"hPa"`/`"m"`, `"days since ..."`); (4) `positive` attribute for vertical (`"up"`/`"down"`). Spec: CF Conventions 1.8 §5.
  - **Files:** (new) `src/cf_conventions/axis.rs`, `src/cf_conventions/mod.rs`
  - **Tests:** (proposed) `test_axis_explicit_attribute`, `test_axis_from_standard_name_latitude`, `test_axis_from_units_degrees_east`, `test_axis_vertical_positive_down`, `test_axis_ambiguous_returns_none`
  - **Risk:** Full udunits2 is huge; ship a minimal recogniser sufficient for the 4 axes.
  - **Prerequisites:** None.

- [x] Implement `_FillValue` / `missing_value` attribute handling
  - **Resolved (2026-07-28):** `NetCdfReader::read_f32_cf`/`read_f64_cf` (`src/reader.rs:304,344`) apply CF §8.1 `scale_factor`/`add_offset` unpacking and CF §2.5.1 `_FillValue` (falling back to `missing_value`) masking to `NaN`; see `test_read_f64_cf_unpacks_scale_offset_and_fill_value`. Shipped as explicit opt-in methods rather than changing `read_f32`/`read_f64` behavior.
  - **Verified gap (historical):** `src/cf_conventions/` and `src/reader.rs` have no fill-value substitution path. `rg -n "fill_value|_FillValue|missing_value" -g '*.rs' src/` shows mentions only in comments and tests.
  - **Goal:** When reading a variable, values matching `_FillValue` or `missing_value` are exposed as `NaN` (for float) or `Option<T>::None` (for integer). API choice TBD.
  - **Design:** Two readback modes: (a) `read_f32_with_fill_as_nan(var)`; (b) `read_f32_with_mask(var) -> (Vec<f32>, Vec<bool>)`. Detect fill via attribute lookup, fallback to NC3 default fill values per NUG §6.3.
  - **Files:** `src/reader.rs`, `src/cf_conventions/`
  - **Tests:** (proposed) `test_fill_value_explicit_attribute`, `test_fill_value_nc3_default`, `test_missing_value_legacy_attribute`, `test_fill_with_mask_separate_output`
  - **Risk:** `_FillValue` vs `missing_value` (legacy) semantics differ; CF 1.8 §2.5.1.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] CF grid_mapping parsing for CRS extraction
  - **Goal:** Recognise `grid_mapping_name` attribute and extract a CRS (`"latitude_longitude"`, `"lambert_conformal_conic"`, etc.).
  - **Files:** `src/cf_conventions/` (new `crs.rs`)
  - **Why deferred:** Needs CF axis detection (above) first.

- [ ] Time coordinate decoding (calendar-aware, CF time units)
  - **Goal:** Parse `"days since 1970-01-01"` etc. into actual datetimes honouring Gregorian/proleptic/noleap calendars.
  - **Files:** `src/cf_conventions/time.rs` (new)
  - **Why deferred:** Calendar arithmetic is substantial; chrono workspace dep already present.

- [x] Variable packing/unpacking (scale_factor, add_offset)
  - **Resolved (2026-07-28):** Implemented as the explicit-opt-in `read_f32_cf`/`read_f64_cf` methods (`src/reader.rs:304,344`) — the "auto vs explicit" API decision was resolved in favor of explicit, alongside the `_FillValue`/`missing_value` masking item above.
  - **Goal:** Automatic decode of packed integer-stored floats per CF §8.1.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Easy to add but needs API decision (auto vs explicit).

- [ ] NetCDF-3 64-bit offset format support (writer)
  - **Goal:** Allow variables > 2 GB by emitting the offset64 magic.
  - **Files:** `src/writer.rs`
  - **Why deferred:** Reader already accepts; writer support is incremental.

- [ ] Multi-variable reading with shared dimension coordinates
  - **Goal:** Open dataset once, expose all variables sharing a dimension.
  - **Files:** `src/reader.rs`
  - **Why deferred:** API ergonomic; not correctness.

- [x] Coordinate bounds variable support (cell boundaries)
  - **Resolved (2026-07-28):** `BoundsVariable` (`src/cf_conventions/time.rs`) models a bounds variable (name, coordinate variable, vertex count) and `validate()` checks dimension/vertex-count consistency against CF §7.1.
  - **Goal:** Read `bounds` attribute pointing at NxK bounds array.
  - **Files:** `src/cf_conventions/`
  - **Why deferred:** Niche; specific user demand needed.

- [ ] OPeNDAP-style constraint expressions for remote subsetting
  - **Goal:** Parse DAP2/DAP4 constraint URLs into slice expressions.
  - **Files:** (new) `src/opendap.rs`
  - **Why deferred:** Requires HTTP layer; large feature.

- [ ] NetCDF-to-Zarr streaming conversion
  - **Goal:** One-shot tool to migrate.
  - **Files:** (new) `examples/nc_to_zarr.rs`
  - **Why deferred:** Cross-crate; needs zarr writer first.

## Low Priority / Future (speculative - concise)

- [x] Pure Rust NetCDF-4 reader _(resolved 2026-07-28 — done via the `oxinetcdf`/`oxih5` backend wired into `NetCdfReader::open`, not via `src/netcdf4/` as this line originally speculated; see the High Priority NC4 items above)_
- [ ] CDL (Common Data Language) text format import/export
- [ ] UGRID convention support for unstructured grids
- [ ] SGRID convention support for structured grids
- [ ] NetCDF-4 group hierarchy traversal
- [ ] Parallel variable reading for multi-core performance
- [ ] NetCDF file repair for truncated / corrupted files
- [ ] NcML aggregation support for multi-file datasets
- [ ] ACDD validation (Attribute Convention for Dataset Discovery)

## Cross-crate dependencies
- **Blocks:** `oxigeo-cli` (NC subcommand richness), `oxigeo-drivers/grib` (GRIB-to-NC conversion).
- **Blocked by:** None for NC3; NC4 items are self-contained (HDF5 parser lives inside this crate).

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries. The previous high-priority item "Update reader/writer to netcdf3 v0.6.0 API (FileReader/FileWriter/Dataset)" is actually done - see `src/nc3_compat.rs` which uses the v0.6 `DataSet` API throughout. Removed from the active list.)_

---
*Last audited: 2026-07-28*
