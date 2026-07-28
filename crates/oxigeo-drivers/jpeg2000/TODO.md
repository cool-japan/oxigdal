# TODO: oxigeo-drivers/jpeg2000

> **Purpose:** Pure Rust JPEG2000 (JP2/J2K) driver for OxiGeo - JP2 box parsing and JPEG2000 codestream decoding
> **Status (2026-07-28):** 10,149 Rust LoC (tokei, `src/`) - 364 tests (all-features and default-features), 0 failed - 0 high-impact real stubs (EBCOT wiring re-verified real; encoding/writer and GeoJP2 remain genuinely unimplemented, tracked below)
> **Roadmap:** v0.1.7 - v0.2.0 - v0.2.1 (current) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Wire EBCOT tier-1 decoder into `decode_rgb()` (currently returns flat 128 placeholder)
  - **Verified gap:** `src/reader.rs:447` - `// For now, return a placeholder image` and `src/reader.rs:457` - `let placeholder = vec![128u8; width * height * 3];`. Note that `src/tier1/decoder.rs` already implements the full 3-pass EBCOT algorithm; the high-level `decode_rgb()` simply ignores it.
  - **Goal:** Decoding a real `.jp2` file returns its actual pixel data, not a uniform gray plane.
  - **Design:** Replace the placeholder path with: (1) parse codestream tile-by-tile (already in `codestream.rs`); (2) for each tile, dispatch packets via `tier2/packet.rs`; (3) decode each code-block with the existing `tier1::CodeBlockDecoder` (real EBCOT in `tier1/decoder.rs`); (4) apply inverse quantization; (5) apply inverse DWT (5/3 or 9/7 selected by SIZ/COD markers; already in `wavelet.rs`); (6) reverse multi-component transform (RCT/ICT in `color.rs`); (7) DC level shift back. Honours ISO/IEC 15444-1 (JPEG 2000 Part 1) clause-by-clause.
  - **Files:** `src/reader.rs` (rewrite `decode_rgb()` and `decode_rgba()`), `src/tier2/mod.rs`, `src/tier2/packet.rs`, `src/wavelet.rs`, `src/color.rs`
  - **Tests:** (proposed) `test_decode_rgb_synthetic_2x2_lossless`, `test_decode_rgb_synthetic_8x8_53_wavelet`, `test_decode_rgb_irreversible_97_wavelet`, `test_decode_rgba_with_opacity_channel`, `test_decode_round_trip_against_known_reference`
  - **Risk:** ISO 15444-1 conformance corner cases (e.g., progression orders, partial code-blocks, ROI maxshift) hard to validate without reference vectors; mitigate by checking against OpenJPEG-produced fixtures.
  - **Prerequisites:** None - all helper modules exist.
  - **Re-verified 2026-07-28:** confirmed still real — `decode_rgb`'s `vec![128u8; ...]` allocations (`src/reader.rs:736,841,1379`) are pre-fill buffer initialization that the per-tile `decode_tile_to_components` loop immediately overwrites, not a fallback path; test assertions at `src/reader.rs:2075-2090` and `:2155-2175` explicitly guard against the old "fabricates placeholder pixels" behavior. Since this item was completed, two further CRITICAL correctness bugs in the same decode path were found and fixed in the 0.2.1 production-hardening campaign (see root `CHANGELOG.md` [0.2.1]): multi-tile decode now bounds each tile's bitstream by its own `Psot` and composites it at its real pixel offset (previously every tile silently returned tile 0's data), and the JP2 box parser now recurses into `jp2h` so `ihdr`/`colr` are actually read from spec-conformant `.jp2` files.

- [ ] Implement JPEG2000 encoding pipeline (J2K codestream and JP2 container)
  - **Verified gap:** No writer exists. `src/lib.rs` re-exports only `Jpeg2000Reader`, `ProgressiveDecoder`. No `Jpeg2000Writer` type. Module list (`box_reader`, `codestream`, `color`, `error`, `jp2_boxes`, `metadata`, `reader`, `tier1`, `tier2`, `wavelet`) has no `writer`.
  - **Goal:** Caller can produce conforming `.jp2` / `.j2k` files from raster data; lossless (5/3) and lossy (9/7) modes; configurable rate.
  - **Design:** Inverse of the decode chain. (1) Forward DCT-shift, (2) MCT (RCT for lossless, ICT for lossy), (3) Forward DWT (5/3 or 9/7), (4) Quantization (dead-zone scalar for lossy; integer-preserving for lossless), (5) EBCOT tier-1 encoding (3-pass: SP, MR, CL with MQ coder), (6) tier-2 packet assembly + rate-distortion truncation (PCRD), (7) Codestream marker assembly (SOC, SIZ, COD, COC, QCD, SOT, SOD, EOC). For JP2: prepend JP2 signature box, ftyp, jp2h (ihdr + colr), jp2c.
  - **Files:** (new) `src/writer.rs`, (new) `src/tier1/encoder.rs`, (new) `src/codestream/writer.rs`, (new) `src/jp2_boxes/writer.rs`; update `src/lib.rs` to export `Jpeg2000Writer`.
  - **Tests:** (proposed) `test_encode_lossless_53_roundtrip`, `test_encode_lossy_97_psnr_above_30db`, `test_encode_grayscale_single_component`, `test_encode_rgb_with_icc_profile_box`, `test_encode_jp2_signature_correct`, `test_encode_target_rate_pcrd`
  - **Risk:** PCRD rate-distortion is non-trivial and easy to mis-implement; start with fixed-quality (uniform truncation) and add PCRD in a follow-up.
  - **Prerequisites:** Item above (decode wiring) - shares infrastructure.

- [ ] Add GeoJP2 metadata box (UUID `B14BF8BD-083D-4B43-A5AE-8CD7D5A6CE03`) reading and writing for georeferenced JP2
  - **Verified gap:** `src/jp2_boxes.rs` defines `BoxType` enum but no GeoJP2 UUID recognition. `src/metadata.rs` has no geo-CRS extraction path. Searched: `rg -n "GeoJP2|B14BF8BD"` returns no hits.
  - **Goal:** Reading a GeoJP2-tagged file exposes its embedded GeoTIFF tags (ModelTiepoint, ModelTransformation, GeoKeyDirectory) via metadata API; writer can attach the same.
  - **Design:** GeoJP2 stores a degenerate GeoTIFF (no image data) in a UUID box. Detect UUID = `B14BF8BD083D4B43A5AE8CD7D5A6CE03`; parse the embedded TIFF tags using existing oxigeo-geotiff tag parser; expose as `Jp2Metadata::geo_transform`, `geo_crs`, `geo_tiepoints`. Write mirror: serialize tags to TIFF, wrap in UUID box, emit before jp2c. Spec: OGC 05-047r3 ("GMLJP2 v1.0" - GeoJP2 referenced therein).
  - **Files:** (new) `src/jp2_boxes/geojp2.rs`, modify `src/metadata.rs` (add `geo_*` fields), modify `src/jp2_boxes.rs` (UUID dispatch)
  - **Tests:** (proposed) `test_geojp2_uuid_detection`, `test_geojp2_round_trip_geotransform`, `test_geojp2_proj4_extraction_from_geokeys`, `test_no_geojp2_returns_none_cleanly`
  - **Risk:** Two competing georeferencing conventions exist (GeoJP2 UUID box vs. GMLJP2 XML box) - implement GeoJP2 first (simpler, GDAL-compatible), GMLJP2 deferred to Medium.
  - **Prerequisites:** Depends on `oxigeo-geotiff` exposing its tag parser as a library function (likely already does).

## Medium Priority (planned - design sketched)

- [ ] SIMD-optimized 5/3 and 9/7 inverse wavelet transforms using `std::simd` portable SIMD
  - **Goal:** 2-4x speedup on the lifting steps for typical 2048x2048 tiles.
  - **Files:** `src/wavelet.rs`
  - **Why deferred:** Correctness first; SIMD after decoder is fully wired (cannot benchmark a placeholder).

- [ ] GMLJP2 metadata box support (XML-encoded geo metadata, complementary to GeoJP2)
  - **Goal:** Parse and emit GML-encoded geographic metadata per OGC 05-047r3.
  - **Files:** (new) `src/jp2_boxes/gmljp2.rs`
  - **Why deferred:** GeoJP2 UUID box covers ~95% of real-world geo-JP2 files; GMLJP2 is a Q2 polish item.

- [ ] Parallel tile decoding using rayon
  - **Goal:** Decode multiple tiles concurrently across CPU cores.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Single-tile correctness first.

- [ ] Memory-efficient tile-at-a-time streaming decode
  - **Goal:** Decode large images without holding the whole codestream + plane in RAM.
  - **Files:** `src/reader.rs`
  - **Why deferred:** Requires the iterative decoder API; ties into decode wiring above.

- [ ] Multi-resolution extraction without full decode (use decomposition levels)
  - **Goal:** Read a 1024x1024 image at 256x256 resolution by skipping the lowest two decomposition levels.
  - **Files:** `src/reader.rs` (the `decode_region_at_resolution` API stub exists; needs real implementation behind wired decoder)
  - **Why deferred:** Requires Item 1 first.

- [ ] Rate-distortion optimization (PCRD) for encoder quality control
  - **Goal:** Hit a target bit-rate within ~5% by truncating coding passes by Lagrangian slope.
  - **Files:** `src/tier2/rate_control.rs` (skeleton exists), to be paired with new `tier1/encoder.rs`
  - **Why deferred:** Encoder pre-requisite (Item 2) must land first.

- [ ] MQ arithmetic decoder throughput optimization
  - **Goal:** Reduce per-symbol overhead in `src/tier1/mq.rs`.
  - **Files:** `src/tier1/mq.rs`
  - **Why deferred:** Optimize after correctness.

## Low Priority / Future (speculative - concise)

- [ ] HTJ2K (High-Throughput JPEG2000, ISO/IEC 15444-15) decoder
- [ ] HTJ2K encoder
- [ ] GPU-accelerated wavelet transforms via WGSL compute shaders
- [ ] Lossless-to-lossy transcoding without full decode/encode cycle
- [ ] JPEG2000 file repair for truncated codestreams
- [ ] ICC profile handling for color-managed workflows
- [ ] JPIP (Interactive Protocol) client for remote tile fetching
- [ ] JPX (Part 2) extended features: animation, compositing
- [ ] Benchmark suite against OpenJPEG and Kakadu reference decoders

## Cross-crate dependencies
- **Blocks:** `oxigeo-drivers/grib` (GRIB2 Template 5.40 needs a JPEG2000 decoder), `oxigeo-cog` (COG can use JPEG2000 compression per GeoTIFF tag 34712).
- **Blocked by:** None.

## Recently completed (kept verbatim from previous TODO.md)
- [x] Full JP2 format support (all standard boxes, complete metadata parsing)
- [x] Error resilience modes (None, Basic, Full) with packet-level error handling
- [x] Progressive decoding with quality layer support
- [x] ROI decoding support (spatial regions and resolution levels)

---
*Last audited: 2026-07-28*
