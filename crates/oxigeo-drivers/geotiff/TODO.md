# TODO: oxigeo-geotiff

> **Purpose:** GeoTIFF/COG driver for OxiGeo — Pure Rust GDAL reimplementation with cloud-optimized reading and writing
> **Status (2026-05-16):** 12,091 Rust LoC · 460 tests · 1 real-code stub (`CogConverter::convert` returns placeholder result), plus 2 documented partial features (FloatingPoint predictor branch, JPEG/WebP codecs already in TODO)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority
- [ ] Implement JPEG compression codec (currently placeholder, `jpeg` feature)
- [x] Implement WebP compression codec (TIFF tag 50001) — pure Rust via `image-webp`; decoder handles VP8/VP8L, encoder produces lossless VP8L; closes #6 (2026-05-22)
- [x] BigTIFF format writer — 64-bit offsets, >4GB output support (planned 2026-04-18)
  - **Goal:** GeoTIFF writer produces valid BigTIFF (magic `0x002B`, 8-byte offsets) when output would exceed the 4GB classic-TIFF limit or the caller explicitly requests it. Round-trippable via existing reader.
  - **Design:**
    - Pre-flight size projection: `width * height * bands * bytes_per_sample > CLASSIC_TIFF_LIMIT` → auto BigTIFF. `BigTiffMode::Auto` (default) / `Force` / `Disable` option on `GeoTiffWriter`.
    - BigTIFF header: bytes 0–1 = `II`/`MM` byte order; bytes 2–3 = `0x002B` (43, BigTIFF); bytes 4–5 = `0x0008` (offset size); bytes 6–7 = `0x0000` (constant); bytes 8–15 = first IFD offset as u64.
    - IFD entries: 20 bytes each (2-byte tag, 2-byte type, 8-byte count, 8-byte value/offset).
    - Strategy: introduce `enum TiffKind { Classic, Big }` into the writer's IFD-serialization path; route all offset writes through a `write_offset` helper dispatching u32 vs u64.
  - **Files:** bigtiff.rs (new), writer/mod.rs, tags.rs
  - **Tests:** 5 tests covering magic, auto mode, force mode, disable mode, roundtrip
  - **Risk:** u32 offsets may be hardcoded; refactor offset abstraction first
- [ ] Implement predictor support for writer (horizontal differencing, floating point)
- [ ] Add multi-band write support to `GeoTiffWriter` (currently single-band focus)
- [ ] Implement parallel tile encoding in `CogWriter` using rayon
- [ ] Add LERC codec full decoding (currently scaffolding in `lerc_codec.rs`)
- [ ] Implement proper GeoKey writing (ModelTiepointTag, ModelPixelScaleTag)
- [x] Wire `CogConverter::convert` to real input read + COG writer pipeline (audit-discovered 2026-05-16)
  - **Goal:** Public API `CogConverter::convert(&mut self) -> Result<ConversionResult>` at `src/cog/converter.rs` currently returns a fabricated `ConversionResult` without reading input data or writing output bytes. Replace placeholders with an end-to-end read-analyze-write pipeline that yields a valid COG on disk.
  - **Verified gap:** Two literal placeholder annotations in the same method:
    1. `// For now, we'll use placeholder data for analysis` (cog/converter.rs:217) followed by `let sample_data = vec![0u8; sample_size];` — analysis runs on synthetic zeros rather than real input bytes.
    2. `// For now, return a placeholder result // In a real implementation, we would: // 1. Read the input data // 2. Create a CogWriter // 3. Write tiles and overviews // 4. Validate the output` (cog/converter.rs:305-310), and `let output_size = (input_size as f64 * 0.8) as u64; // Placeholder` (cog/converter.rs:313) — the synthesized 80%-of-input figure is not a real measurement.
    The struct is publicly exposed via `cog/mod.rs:39-40` (`pub use converter::{BatchConversionConfig, BatchConversionResult, CogConverter, ConversionConfig, ...}`) and consumed by `cog/tools.rs:42,54` (`convert_to_cog`, `convert_to_cog_with_config` free functions). All callers are silently broken.
  - **Design:**
    1. Replace lines 217-225 with `read_tile_samples(&source, &tiff, width, height, sample_dtype, samples_per_pixel, photometric)?` — read a representative sub-grid of real tile data from the input TIFF for `analyze_for_cog` to inspect. Reuse existing tile-read infrastructure (`crates/oxigeo-drivers/geotiff/src/cog/reader.rs` if present, or factor a helper from there).
    2. Detect `data_type`, `samples_per_pixel`, `photometric` from IFD tags (`SampleFormat`, `BitsPerSample`, `SamplesPerPixel`, `PhotometricInterpretation`) instead of hard-coding `UInt8` / `1` / `BlackIsZero`.
    3. Replace lines 295-320 with: open a fresh `CogWriter` at `_output_path` using resolved `tile_width`, `tile_height`, `compression`, `overview_levels`; iterate over input tiles (via existing `RasterReader::read_tile` or strip-decoder); write to `CogWriter`; flush; then `std::fs::metadata` to get the real `output_size`.
    4. `ConversionResult` populated with measured `input_size`, `output_size`, `compression_ratio = input_size / output_size`, overview count, validation status.
    5. Optional final `validate_cog(&output_path)?` call using existing `crates/oxigeo-drivers/geotiff/src/cog/mod.rs:validate_cog` (note: that function has its own "simplified check" at line 146 — leave for a follow-up).
  - **Files:**
    - `crates/oxigeo-drivers/geotiff/src/cog/converter.rs` (replace lines 217-225 + 295-320, ~150 LoC net).
    - `crates/oxigeo-drivers/geotiff/src/cog/mod.rs` (no API change; re-exports stay).
    - `crates/oxigeo-drivers/geotiff/src/cog/tools.rs` (no API change; uses `CogConverter` already).
    - `crates/oxigeo-drivers/geotiff/tests/cog_converter_integration.rs` (new, ~200 LoC).
  - **Tests:** `test_cog_converter_classic_tiff_to_cog_roundtrip`, `test_cog_converter_auto_optimize_chooses_settings`, `test_cog_converter_explicit_tile_size_honoured`, `test_cog_converter_output_validates_as_cog`, `test_cog_converter_compression_deflate_actually_compresses`, `test_cog_converter_progress_callback_invoked_in_order`.
  - **Prerequisites:** `CogWriter::create` must accept all resolved options (verify; should be done — see existing `cog_writer.rs`). `analyze_for_cog` must accept real sampled data (already does — current placeholder feeds zeros).
  - **Risk:**
    - Large inputs: reading a 10 GB input synchronously would OOM. Stream tile-by-tile; never `Vec<u8>` the whole input.
    - Reader/writer compression mismatch: input may be `Lzw` and output must transcode to `Deflate`. Decompress on read, recompress on write.
  - **Done:** 2026-05-22 (Slice 27). `src/cog/converter.rs` rewritten (+114/-24, 569 LoC total): `CogConverter::convert` now runs a real pipeline — opens the input TIFF via `GeoTiffReader::open(FileDataSource)`, decodes the full band once via `read_band`, detects data-type / samples-per-pixel / photometric from real IFD tags (`crate::tiff::ImageInfo`), feeds REAL sampled data to `analyze_for_cog`, writes a genuine COG via `CogWriter::create` + `.write` with resolved tile size / compression / overview levels / geo-referencing, and reports the MEASURED `output_size` from `fs::metadata` (no more `input_size * 0.8` placeholder). `compression_ratio` is real input/output; `overview_count` from the written file's IFD count; validation status from the `CogValidation` returned by the writer. `report_progress` callbacks preserved in order; `convert` public signature byte-for-byte unchanged; `cog/mod.rs` re-exports untouched. Note: `CogWriter` takes the full image buffer (no tile-streaming API) — converter hands it the once-decoded band. `analyze_for_cog` errors on images too small to generate an overview (pre-existing constraint; tiny-image tests pass explicit settings to skip analysis).
  - **Tests:** 10 in `crates/oxigeo-drivers/geotiff/tests/cog_converter_test.rs` (classic-TIFF→COG round trip; output exists+nonempty; output_size measured not fabricated; compression_ratio reflects real sizes; explicit tile size honoured; auto-optimize path; progress callbacks in order; output validates as COG; Float32 input data-type detection; nonexistent-input error). Full crate suite 489/489.
    - Tile-grid mismatch: input may be striped, output is tiled. Re-tile via row-aligned decode buffer.

## Medium Priority
- [ ] Add async tile reading for cloud-native COG access via HTTP range requests
- [ ] Implement EXIF metadata preservation during read/write round-trip
- [ ] Add planar configuration support (separate planes vs. contiguous)
- [ ] Implement ICC color profile embedding and extraction
- [ ] Add per-band nodata value support (currently single nodata for all bands)
- [ ] Implement overview generation with configurable resampling in writer
- [ ] Add TIFF tag preservation for unknown/custom tags during round-trip
- [ ] Implement COG validation against OGC COG specification (stricter than current)

## Low Priority / Future
- [ ] Add JPEG-XL compression support when Pure Rust codec becomes available
- [ ] Implement tile cache with configurable eviction for repeated random access
- [ ] Add streaming COG generation from input iterators (constant memory)
- [ ] Implement GeoTIFF metadata editor (update CRS/transform without rewriting data)
- [ ] Add support for TIFF sub-IFDs (multi-page TIFF beyond overviews)
- [ ] Implement 12-bit and 1-bit sample format support
- [ ] Add TIFF strip layout writer (not just tiled)
- [ ] Implement GDAL PAM (.aux.xml) sidecar metadata reading

## Cross-crate dependencies
- **Blocks:** `oxigeo` (re-exported via `geotiff` default feature), `oxigeo-cli` (translate/warp/convert/info/stats subcommands), `oxigeo-pmtiles` (tile encoding consumers), `oxigeo-mbtiles` (raster tile producers), `oxigeo-services` (OGC tile endpoints)
- **Blocked by:** `oxigeo-core` (RasterBuffer, DataSource, GeoTransform); `oxiarc-deflate`/`oxiarc-lzw`/`oxiarc-zstd` (decompression — already wired via per-codec features)

---
*Last audited: 2026-05-16*
