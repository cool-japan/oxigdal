# TODO: oxigeo-drivers/grib

> **Purpose:** GRIB1/GRIB2 meteorological data format driver for OxiGeo - Pure Rust implementation
> **Status (2026-05-16):** 4,753 Rust LoC (incl. tests) - 65 tests - 1 verified behavior gap (DRT 5.2/5.3/5.40/5.41 errors out)
> **Roadmap:** v0.1.7 - v0.2.0 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Implement GRIB2 Data Representation Template 5.2 (complex packing) and 5.3 (complex packing with spatial differencing)
  - **Verified gap:** `src/grib2/section5.rs:51` - `_ => Err(GribError::UnsupportedDataTemplate(template_number))`. The match in `DataRepresentationSection::from_bytes` only handles `0 | 40` (simple packing); every other DRT number including 2, 3, 40, 41 returns an error. Result: real-world GRIB2 files from ECMWF / NCEP fail to parse.
  - **Goal:** Decoding a GRIB2 file using DRT 5.2 or 5.3 produces correct `f32` values bit-for-bit comparable to wgrib2 / eccodes output.
  - **Design:** DRT 5.2 per WMO Manual on Codes Vol. I.2 §FM-92 Reg.1, Table 5.2: extends DRT 5.0 with group splitting parameters (group widths, group lengths, reference values per group). Algorithm: (1) read group reference values (with bits-per-value `nbits_ref`); (2) read group widths array (with `nbits_gw`); (3) read group lengths array (with `nbits_gl`); (4) for each group, decode `length` packed values with `width` bits, add group reference, then apply E/D scaling. DRT 5.3 adds first/second-order spatial differencing: after group-decoding, the values are second differences of first differences of original values; apply inverse: `v[i] = 2*v[i-1] - v[i-2] + d[i]`. Spec: WMO GRIB2 Code Table 5.0 / Templates 5.2 + 5.3.
  - **Files:** `src/grib2/section5.rs` (extend `DataRepresentationSection` with `Complex { group_*: ... }` variant or sibling struct), `src/grib2/decoder.rs` (new `decode_complex_packing`, `decode_complex_with_spatial_diff` functions)
  - **Tests:** (proposed) `test_drt52_uniform_group_widths`, `test_drt52_variable_group_widths`, `test_drt52_with_missing_values`, `test_drt53_first_order_spatial_diff`, `test_drt53_second_order_spatial_diff`, `test_drt52_against_wgrib2_fixture` (requires bundling a small test grib file)
  - **Risk:** Octet-level bit-packing arithmetic is the standard source of off-by-one errors; mitigation: small synthetic test fixtures with hand-verified expected values.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 27). `src/grib2/decoder.rs` extended +528 LoC (the file already held `Grib2Decoder` — extended, not overwritten): MSB-first bounds-checked `BitReader`, `ComplexPackingParams`, `SpatialDiffParams`, `decode_complex_packing` (DRT 5.2 — group reference values / group widths / group lengths bit-arrays, per-group width-bit decode, width-0 groups = all-equal-reference, missing-value management, `(R+X·2^E)/10^D` scaling), `decode_complex_with_spatial_diff` (DRT 5.3 — sign-magnitude extra descriptors, order-1 `v[i]=d[i]+v[i-1]` / order-2 `v[i]=d[i]+2v[i-1]-v[i-2]` inverse difference, overall-minimum add-back). `section5.rs` +132 LoC: parses templates 2/3 via additive `Option<ComplexPackingParams>` / `Option<SpatialDiffParams>` fields (the four flat DRT-5.0 fields kept byte-for-byte → simple-packing API unchanged); the `_ =>` rejection retained for DRT 40/41 (out of scope). Section-7 dispatch added in `Grib2Decoder::decode` (5.3 takes precedence over 5.2). No new `GribError` variants (reused `InvalidBitOperation`/`TruncatedMessage`/`InvalidDataRepresentation`). `grib2/mod.rs` +4 re-export lines.
  - **Tests:** 12 in `crates/oxigeo-drivers/grib/tests/complex_packing_test.rs` — hand-built synthetic byte fixtures with hand-verified expected f32 (bit reader MSB-first / overrun / byte-boundary-spanning; DRT 5.2 param parse, uniform/variable group widths, zero-width group, E/D scaling, single-group round trip; DRT 5.3 param parse, first-/second-order spatial diff). Full crate suite 77/77.

- [ ] Implement GRIB2 DRT 5.40 (JPEG2000 compressed) and 5.41 (PNG compressed) data sections
  - **Verified gap:** Same match arm at `src/grib2/section5.rs:51` rejects DRT 40 and 41. (Note: the arm currently lists `0 | 40`, but the `40` here matches DRT 5.0 incorrectly used as a placeholder for satellite simulation - read the surrounding code; it should fall through to the error path for the JPEG2000 case.) The TODO claim "Template 5.40 JPEG2000" and "Template 5.41 PNG" align.
  - **Goal:** Decoding GRIB2 files that use JPEG2000 (very common in NCEP NDFD products) or PNG (less common but present) packing.
  - **Design:** Section 7 (data) is the entire JPEG2000 J2K codestream / PNG file. Forward to `oxigeo-jpeg2000::Jpeg2000Reader` (Pure-Rust J2K decoder; currently incomplete but exists - see `oxigeo-drivers/jpeg2000/TODO.md`) for DRT 5.40; for DRT 5.41 use a Pure-Rust PNG decoder (no PNG crate currently in workspace; introduce `oxiarc-png` if exists, else `image-png` per workspace policy). Output values mapped through Section 5 scaling (R, E, D) as for simple packing.
  - **Files:** `src/grib2/decoder.rs` (dispatch on DRT number), `src/grib2/section5.rs` (template 40/41 variants); `Cargo.toml` adds `oxigeo-jpeg2000.workspace = true` under a `jpeg2000` feature
  - **Tests:** (proposed) `test_drt540_jpeg2000_ndfd_sample`, `test_drt541_png_basic`, `test_drt540_with_bitmap_section`
  - **Risk:** DRT 5.40 is blocked by `oxigeo-jpeg2000::decode_rgb` placeholder (see jpeg2000 TODO); cannot fully test until that is fixed.
  - **Prerequisites:** `oxigeo-drivers/jpeg2000` decode wiring item.

- [ ] Implement GRIB2 writing support (at least DRT 5.0 simple packing)
  - **Verified gap:** `src/reader.rs` exposes `GribReader` / `GribRecord`; no `GribWriter` exists. `rg -n "pub struct.*Writer|pub fn.*write" -g '*.rs' src/` returns no public writer.
  - **Goal:** Caller can produce a valid GRIB2 file from gridded f32 data + parameter/grid metadata.
  - **Design:** Top-down: assemble Sections 0-8 byte-by-byte. Section 0 (indicator): magic "GRIB" + edition 2. Section 1 (identification): originating center, reference time. Section 3 (grid): use templates from existing `src/grid.rs`. Section 4 (product): use templates from `src/templates.rs`. Section 5 (data representation): emit DRT 5.0 with caller-chosen `bits_per_value` (default 16). Section 7 (data): forward-pack values via R + 2^E * D scaling. Section 8: "7777" end marker. Each section's length prefix is big-endian per WMO spec.
  - **Files:** (new) `src/grib2/writer.rs`, (new) `src/grib2/section_writers.rs`, `src/lib.rs` (re-export `Grib2Writer`)
  - **Tests:** (proposed) `test_grib2_write_minimal_simple_packing`, `test_grib2_write_lat_lon_grid`, `test_grib2_round_trip_through_grib_reader`, `test_grib2_write_matches_wgrib2_inventory`
  - **Risk:** Field length pre-computation order matters - many sections require knowing data section length first; mitigate via two-pass build (compute sizes, then emit).
  - **Prerequisites:** None for DRT 5.0; complex packing items above are independent.

- [ ] Multi-message GRIB file scanning / inventory (current API only iterates linearly)
  - **Verified gap:** `src/reader.rs` defines `GribReader::open` returning `Iterator<Item = Result<GribRecord>>`. No `scan()` returning a vector of `(byte_offset, parameter, level, time)` tuples without decoding data. `rg -n "scan|inventory|index|offset_map" -g '*.rs' src/` returns no relevant matches.
  - **Goal:** Cheap inventory of large multi-message GRIB files: list all messages with metadata, then random-seek to decode specific ones.
  - **Design:** Implement `GribReader::scan(&mut self) -> Result<Vec<MessageDescriptor>>` where `MessageDescriptor` carries `byte_offset`, `total_length`, `edition`, `parameter`, `level`, `forecast_time`. Walk sections 0/1/3/4 only (skip data Section 7), advance by `section_length`. Add `read_message_at(offset: u64)` for random access.
  - **Files:** `src/reader.rs`, `src/message.rs`
  - **Tests:** (proposed) `test_scan_multi_message_file`, `test_scan_then_random_access_decode`, `test_scan_skips_truncated_message_gracefully`
  - **Risk:** Old GRIB1 messages embedded in mixed files have different section sizes; gate edition-by-edition.
  - **Prerequisites:** None.

## Medium Priority (planned - design sketched)

- [ ] WMO GRIB2 parameter Table 4.2 complete coverage (currently partial)
  - **Goal:** Recognise all Discipline x Category combinations from WMO Table 4.2.
  - **Files:** `src/parameter.rs`
  - **Why deferred:** Tedious table entry; not a blocker but a polish item.

- [ ] GRIB2 Section 2 (Local Use) parsing for ECMWF/NCEP extensions
  - **Goal:** Expose locally-defined attributes rather than ignoring Section 2.
  - **Files:** (new) `src/grib2/section2.rs`
  - **Why deferred:** Vendor-specific; lower priority than core templates.

- [ ] GRIB2 ensemble / probability product templates (PDT 4.1, 4.2, 4.11)
  - **Goal:** Decode ensemble member, probability-of-event products.
  - **Files:** `src/templates.rs` (PdtType T1, T2, T11 are declared but not deeply parsed)
  - **Why deferred:** Active research data use case; not a default workflow.

- [ ] Time range processing templates (PDT 4.8/4.9/4.10) - accumulation, average, max/min over interval
  - **Goal:** Decode accumulation periods etc.
  - **Files:** `src/templates.rs`
  - **Why deferred:** Niche.

- [ ] GRIB index file (.idx) generation and reading for fast multi-process access
  - **Goal:** Pre-built byte-offset index per message; share between processes.
  - **Files:** (new) `src/index.rs`
  - **Why deferred:** Belongs to operational workflow; build after scan() lands.

- [ ] GRIB2 Section 6 bitmap handling for sparse grids
  - **Goal:** Honour Section 6 bitmap to interpret missing-value semantics correctly.
  - **Files:** `src/grib2/mod.rs` (bitmap is already read into `Grib2Message.bitmap`; need ensure decoder uses it)
  - **Why deferred:** Decoder already handles bitmap in `src/grib2/decoder.rs:38-54`; this entry is now mostly a verification + extra-template task.

- [ ] WMO originating center / subcenter metadata tables (Table C-11)
  - **Goal:** Map center number to organization name.
  - **Files:** `src/parameter.rs`
  - **Why deferred:** Pure data table; tedious but easy.

- [ ] GRIB2 template 3.x grid definitions for rotated, stretched, irregular grids
  - **Goal:** Decode lat/lon, Lambert, polar stereographic, rotated lat/lon, irregular.
  - **Files:** `src/grid.rs` (partial - check which templates are wired)
  - **Why deferred:** Extension of existing infrastructure.

- [ ] GRIB-to-NetCDF/Zarr conversion tool
  - **Goal:** CLI to convert.
  - **Files:** (new) `examples/grib2zarr.rs`
  - **Why deferred:** Cross-crate; uses other drivers as dependencies.

## Low Priority / Future (speculative - concise)

- [ ] GRIB2 spectral data templates (DRT 5.50, 5.51)
- [ ] GRIB2 CCSDS / AEC compression (DRT 5.42)
- [ ] GRIB1 to GRIB2 conversion tool
- [ ] GRIB message editing (modify metadata without rewriting data)
- [ ] Parallel multi-message decoding for large GRIB files
- [ ] GRIB2 derived parameter computation (e.g., wind speed from U/V)
- [ ] GRIB inventory caching for repeated access to large files
- [ ] ecCodes-compatible key access for interoperability

## Cross-crate dependencies
- **Blocks:** Operational meteorology workflows.
- **Blocked by:** `oxigeo-drivers/jpeg2000` decode wiring (for DRT 5.40 only).

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries.)_

---
*Last audited: 2026-05-16*
