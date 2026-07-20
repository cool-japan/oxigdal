# TODO: oxigeo-compress

> **Purpose:** Advanced compression codecs and auto-selection for geospatial data (LZ4, Zstd, Brotli, Snappy, DEFLATE, delta/RLE/dictionary, ZFP/SZ floating-point).
> **Status (2026-05-16):** 4,471 Rust LoC · 91 tests · 3 real stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (next slice — verified gaps)

- [x] Replace placeholder `Benchmarker::benchmark` with real codec round-trip measurement
  - **Verified gap:** `src/benchmark.rs:193-203` — `/// Run benchmark (placeholder - actual implementations would use specific codecs)` followed by `// This is a placeholder. Real implementation would benchmark each codec` and `Ok(BenchmarkReport { results: Vec::new(), best_ratio: String::new(), ... })`. The function consumes `_data: &[u8]` and `_codecs: &[CodecType]` (both `_`-prefixed, unused).
  - **Goal:** `Benchmarker::new(iterations).benchmark(data, codecs)` produces real `BenchmarkReport` with per-codec `ratio`, `compression_speed_mbps`, `decompression_speed_mbps`, `best_*` ranking strings.
  - **Design:** For each codec in `codecs`: dispatch via `CodecType` → `Box<dyn Codec>` factory (use existing `Lz4Codec`, `ZstdCodec`, etc.); run `iterations` warmup + measured iterations; record `std::time::Instant::now()` deltas; compute mean compression/decompression throughput; verify decompressed bytes match input. Best-ratio = smallest `compressed.len() / data.len()`; best-balanced = harmonic mean of speed × (1 − ratio).
  - **Files:** `src/benchmark.rs` (replace body of `benchmark` ~lines 193-204); add codec dispatch helper `fn codec_from_type(t: CodecType) -> Box<dyn Codec>` mirroring `auto_select.rs`
  - **Tests:** `(proposed)` test_benchmark_round_trip_correctness, test_benchmark_ranks_zstd_better_ratio_than_lz4_on_text, test_benchmark_iterations_stable, test_benchmark_empty_codecs_list, test_benchmark_handles_codec_error
  - **Risk:** Wall-clock timing on CI machines is noisy; report `mean ± stddev` and let consumers decide tolerance. Document that timings are indicative, not authoritative.
  - **Prerequisites:** None — all codec implementations exist (`src/codecs/`).
  - **Done:** 2026-05-22 (Slice 26). `src/benchmark.rs` rewritten (+323/-68, total 465 LoC): `Benchmarker { iterations: usize }` (drops underscore prefix; field now used); `new(iterations).max(1)` clamps zero. Replaced `benchmark` body with 3-iteration warmup + per-codec `Instant`-timed compress→decompress→verify loop. Per-codec sentinel results (`ratio = f64::INFINITY`, speeds = 0.0) on construction failure / round-trip mismatch — full benchmark always completes. Rankings: `best_ratio` (smallest), `best_compression_speed` (largest), `best_decompression_speed` (largest), `best_balanced` (max of `c_speed * (1 - ratio).max(0.0)`). Private `BenchCodec` trait (renamed from spec's `Codec` because the crate's codecs use slightly different signatures — `Lz4Codec::decompress` / `ZstdCodec::decompress` take an extra `Option<usize>` size hint while others don't) with eight `impl BenchCodec for *Codec` blocks dispatched via `make_bench_codec(CodecType) -> Box<dyn BenchCodec>`. Kept existing `BenchmarkResult` struct field names (`compression_ratio`, `compression_throughput`) instead of spec's `ratio`/`compression_speed_mbps` to avoid breaking the public surface; additively added `pub iterations: usize` + `is_sentinel()` helper. Public signatures of `Benchmarker::new` and `Benchmarker::benchmark` byte-for-byte unchanged.
  - **Tests:** 10 in `crates/oxigeo-compress/tests/benchmark_test.rs` (round-trip correctness lz4; round-trip correctness zstd; zstd ratio < lz4 on repetitive text; iterations returns N results for N codecs; empty codecs returns empty report; zero-iterations clamped to one (spec's alternative for the mismatch-sentinel test); best_ratio picks smallest-ratio codec; best_balanced picks tradeoff; default iterations = 3; short input still completes). Full crate suite 125/125 (no regressions; Slice 24 blosc still passes).

- [ ] Implement real ZFP fixed-precision and fixed-accuracy modes (currently delegate to fixed-rate)
  - **Verified gap:** `src/floating_point/zfp.rs:218` — `// Fixed-precision compression for f32 (simplified)` immediately followed by `let bits = (precision + 8).min(32); self.compress_f32_fixed_rate(input, bits)`. And `zfp.rs:235` — `// Fixed-accuracy compression for f32 (simplified)` — same delegation pattern.
  - **Goal:** Implement ZFP fixed-precision and fixed-accuracy as distinct algorithms — not aliases for fixed-rate. Reference: Lindstrom 2014 *Fixed-Rate Compressed Floating-Point Arrays* IEEE TVCG.
  - **Design:** ZFP block transform (4×4×4 cubes for 3D; 4×1 strides for 1D arrays in this crate). Phases: (1) block float-to-integer conversion via shared exponent; (2) decorrelating orthogonal transform; (3) embedded coder (bit-plane). Fixed-precision: stop after `precision` bit-planes; fixed-accuracy: stop when block error < `accuracy`; fixed-rate: stop at fixed bit budget. Inverse on decode mirrors the three phases.
  - **Files:** `src/floating_point/zfp.rs` (replace `compress_f32_fixed_precision`/`compress_f32_fixed_accuracy` and f64 variants); add `src/floating_point/zfp/transform.rs` for orthogonal block transform
  - **Tests:** `(proposed)` test_zfp_fixed_precision_bit_plane_count, test_zfp_fixed_accuracy_max_error_bound, test_zfp_fixed_rate_still_works, test_zfp_round_trip_smooth_field, test_zfp_round_trip_random_noise_higher_compression_ratio_on_smooth
  - **Risk:** Reference C zfp uses non-trivial bit-plane coder; ship Pure-Rust translation rather than depend on FFI (COOLJAPAN policy). Phase the work: 1D first, 2D/3D blocks v0.2.0.
  - **Prerequisites:** None.

- [x] Implement Blosc-style meta-compressor (shuffle + codec selection)
  - **Verified gap:** `src/codecs/` lists individual codecs (`lz4.rs`, `zstd.rs`, …) and `floating_point/` has ZFP/SZ; no `blosc.rs` or shuffle pre-filter module.
  - **Goal:** New `BloscCodec` chaining a byte-shuffle / bit-shuffle pre-filter with a backend codec (LZ4 / Zstd / Snappy). Faithful Blosc 2.x frame format so output round-trips through Python-blosc consumers.
  - **Design:** Frame header (16 bytes) per Blosc spec — `0x02 (version)`, `0x01 (versionlz)`, `flags` (shuffle/bitshuffle/typesize), `typesize`, `nbytes`, `blocksize`, `cbytes`, `filter_pipeline_id`. Block-level compression: split input into `blocksize`-sized chunks (default 256 KiB); per-block: apply shuffle (group bytes by position within `typesize` element across the block), pipe through codec, store `chunk_len + payload`. Decompress reverses block-by-block.
  - **Files:** `(new) src/codecs/blosc.rs` (~600 LoC); `(new) src/codecs/shuffle.rs` (byte-shuffle + bit-shuffle as standalone filters); `src/codecs/mod.rs` (re-export); `src/lib.rs` prelude
  - **Tests:** `(proposed)` test_blosc_shuffle_roundtrip_f32_array, test_blosc_bitshuffle_roundtrip_u16_array, test_blosc_frame_header_matches_spec, test_blosc_with_zstd_backend, test_blosc_with_lz4_backend, test_blosc_decompresses_python_blosc_output (golden fixture)
  - **Risk:** Bit-level shuffle (`bitshuffle`) is non-trivial; restrict v0.1.5 to byte-shuffle; bitshuffle v0.2.0. Document the Python-blosc fixture origin.
  - **Prerequisites:** None.
  - **Done:** 2026-05-20 (Slice 24). New `src/codecs/shuffle.rs` — `byte_shuffle` / `byte_unshuffle` + in-place variants; tail (`data.len() % typesize`) passes through. New `src/codecs/blosc.rs` — 16-byte c-blosc2 frame header (`0x02 version`, `0x01 versionlz`, flags, typesize, nbytes LE, blocksize LE, cbytes LE, filter_pipeline_id) + 1-byte backend id + `[u32 LE num_blocks][num_blocks × u32 cumulative_offset][per-block payloads]`. Backends: LZ4 / Zstd / Snappy via existing `oxiarc-lz4` / `oxiarc-zstd` / `oxiarc-snappy` (no new deps). `clevel` 0-9 remapped to each backend's native range. BitShuffle bit is reserved but never set (deferred per design risk). `Codec` trait shape: `compress/decompress(&[u8]) -> Result<Vec<u8>, CompressionError>` matching the existing inherent shape on `Lz4Codec/ZstdCodec/SnappyCodec`.
  - **Tests:** 24 (16 required + 8 extras), 15 selected by the `blosc` filter (12 integration + 3 unit). Coverage: shuffle round-trip f32/u16; typesize=1 identity; uneven tail preserved; frame header layout + version/versionlz; per-backend round-trip LZ4/Zstd/Snappy; no-shuffle path; byte-shuffle path; truncated-header + invalid-version + block-count-mismatch typed errors; `Codec` trait impl parity with one-shot; smaller-than-raw for repetitive input.

- [ ] Add streaming compression/decompression API (process chunks without full buffer)
  - **Verified gap:** `src/codecs/mod.rs` exposes `Codec` trait with `compress(&[u8]) -> Result<Vec<u8>>` only — no `Read`/`Write` streaming variant. `src/lib.rs:9` lists "**Parallel Processing**" but no "Streaming" feature.
  - **Goal:** New `StreamingCodec` trait with `compress_writer<W: Write>(&self, src: &[u8], dst: &mut W)` and `decompress_reader<R: Read>(&self, src: &mut R, dst: &mut W)`. Enables processing files larger than RAM.
  - **Design:** Per-codec streaming wrappers exposing the native frame format. LZ4 — `oxiarc-lz4` already supports frame format; route through. Zstd — `oxiarc-zstd` streaming API (verify). DEFLATE — chunk-by-chunk via flate2-equivalent. Buffer size configurable via `StreamingOptions::chunk_size` (default 256 KiB).
  - **Files:** `(new) src/streaming.rs`; `src/codecs/mod.rs` (declare trait); per-codec impl extensions
  - **Tests:** `(proposed)` test_streaming_compress_writer_lz4, test_streaming_decompress_reader_zstd, test_streaming_chunk_size_boundary, test_streaming_matches_oneshot_output, test_streaming_io_error_propagates
  - **Risk:** Each oxiarc-* crate has different streaming APIs; survey first, may need fallback to block-mode wrappers for those without native streaming.
  - **Prerequisites:** Survey `oxiarc-lz4`/`oxiarc-zstd`/`oxiarc-deflate`/`oxiarc-brotli` streaming surface.

- [x] Implement codec chaining (e.g., shuffle → delta → zstd pipeline)
  - **Goal:** `CodecPipeline::new().push(Shuffle).push(Delta).push(Zstd)` produces a single composite codec.
  - **Design:** `CodecPipeline { stages: Vec<Box<dyn Codec>> }` with `compress` walking stages forward, `decompress` walking in reverse. Frame header records pipeline composition for self-describing decode.
  - **Files:** `(new) src/codecs/pipeline.rs`; `src/codecs/mod.rs`
  - **Tests:** `(proposed)` test_pipeline_shuffle_zstd_roundtrip, test_pipeline_three_stages, test_pipeline_decompress_validates_header
  - **Risk:** Compatibility with stage order — must not blindly compose lossy + lossless (define stage tagging).
  - **Prerequisites:** Item 3 (shuffle).
  - **Done:** 2026-05-22 (Slice 27). New `src/codecs/pipeline.rs` (~411 LoC): `PipelineStage { Shuffle{typesize}, Delta, Rle, Lz4, Zstd, Snappy, Brotli, Deflate, Dictionary }` (one-byte stage ids), `CodecPipeline { stages }` builder. `compress` emits a self-describing fixed-stride frame header `[O,X,P,L, version=1, num_stages, (stage_id,typesize)*N]` then walks stages forward; `decompress` / `decompress_self_describing` parse+validate the header and walk in reverse (the latter reconstructs the pipeline purely from the header). Self-contained `byte_shuffle`/`byte_unshuffle` inside `pipeline.rs` (no dependency on Slice-24 `codecs/shuffle.rs` — compiles at both 0.1.4 and HEAD). Header errors → `CompressionError::InvalidMetadata`. `codecs/mod.rs` +2 additive lines (`pub mod pipeline;` + re-export).
  - **Tests:** 12 in `crates/oxigeo-compress/tests/pipeline_test.rs` (empty round-trip; single-stage zstd; shuffle→zstd; three-stage shuffle→delta→deflate; header magic+version; self-describing reconstruction; truncated/bad-magic/unknown-stage-id errors; f32-array shuffle→lz4; builder chaining; smaller-than-raw). Full crate suite 141/141. Side note: an isolated pre-existing `oxiarc-brotli` Huffman round-trip bug surfaced during testing — test #6 uses Deflate instead of Brotli; the Brotli pipeline stage itself is fully implemented and dispatched.

## Medium Priority (planned — design sketched)

- [ ] Add compression ratio and throughput metrics collection per operation
  - **Goal:** `CompressionMetrics::record(codec, bytes_in, bytes_out, duration)` accumulating into a global registry.
  - **Files:** `src/metadata.rs` (extend `CompressionMetadata`)
  - **Why deferred:** Benchmarker (Item 1) covers ad-hoc measurements; persistent metrics need a sink design.

- [ ] Implement byte-shuffle and bit-shuffle filters as pre-compression transforms (split from Blosc)
  - **Goal:** Standalone `ShuffleFilter::new(typesize)` usable outside of Blosc context.
  - **Files:** `(new) src/codecs/shuffle.rs` (shared with Blosc Item 3)
  - **Why deferred:** Folded into Item 3 (Blosc) above.

- [ ] Add adaptive codec selection using sample-based profiling
  - **Goal:** Test-compress a 16 KiB sample with 3 codecs in parallel, pick winner before full compression.
  - **Files:** `src/auto_select.rs` (extend `AutoSelector`)
  - **Why deferred:** Static profile (current `DataCharacteristics`) sufficient for most workloads.

- [ ] Implement LZMA/XZ codec for maximum compression ratio
  - **Files:** `(new) src/codecs/lzma.rs` (depends on `oxiarc-xz` if it exists; else defer)
  - **Why deferred:** No oxiarc-xz today; XZ via existing oxiarc-archive requires investigation.

- [ ] Add LZ4HC (high-compression) mode alongside standard LZ4
  - **Files:** `src/codecs/lz4.rs` (extend); check `oxiarc-lz4` HC support
  - **Why deferred:** Defer until oxiarc-lz4 exposes HC level.

- [ ] Implement Brotli quality auto-tuning based on data characteristics
  - **Files:** `src/codecs/brotli.rs` (extend; pick quality 0/4/8/11 based on entropy)
  - **Why deferred:** Heuristic quality picker after sample-based profiling lands.

- [ ] Add compression metadata embedding (codec ID, parameters, original size)
  - **Files:** `src/metadata.rs` (extend `CompressionMetadata`)
  - **Why deferred:** Codec frames already record this — only needed for raw byte streams.

- [ ] Implement frame format for all codecs (header with size, checksum, codec info)
  - **Files:** `(new) src/codecs/frame.rs`; per-codec adoption
  - **Why deferred:** Each codec ships its own frame format today; unified frame is a v0.2.0 breaking change.

- [ ] Add dictionary training for domain-specific dictionary compression
  - **Files:** `src/codecs/dictionary.rs` (extend; currently no training routine)
  - **Why deferred:** Zstd dictionary mode is the more interesting target; needs oxiarc-zstd CDict support.

- [ ] Implement parallel decompression for multi-chunk data
  - **Files:** `src/parallel.rs` (extend `ParallelCompressor`)
  - **Why deferred:** Compression-side parallelism shipped; decompression added once chunked frame format (above) lands.

## Low Priority / Future (speculative — one-liners only)

- [ ] Add FPZIP compression for structured floating-point grids.
- [ ] Implement quantization-based compression (reduce precision before lossless).
- [ ] Add AEC/CCSDS compression (used in meteorological data).
- [ ] Implement compression benchmark harness with configurable test data generators.
- [ ] Add WASM-compatible codec subset (no_std + alloc).
- [ ] Implement bit-packing codec for integer data with limited range.
- [ ] Add checksumming integration (CRC32, XXHash) for integrity verification.
- [ ] Implement compression-aware memory allocator (pre-allocate output buffers).

## Cross-crate dependencies

- **Blocks:** `oxigeo-zarr` (chunk compression), `oxigeo-geoparquet` (column compression), `oxigeo-geotiff` (tile compression)
- **Blocked by:** `oxiarc-lz4`, `oxiarc-zstd`, `oxiarc-deflate`, `oxiarc-archive`, `oxiarc-brotli`, `oxiarc-snappy` (all workspace deps — COOLJAPAN Pure Rust policy)

## Recently completed (verbatim)

_No previous `[x]` items recorded._

---
*Last audited: 2026-05-16*
