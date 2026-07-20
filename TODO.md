# OxiGDAL TODO

> Version: 0.1.7 (production-hardening complete, validated 2026-07-20, not yet published) | 76 crates | 16,909 tests, 0 failures | ~747K Rust SLoC | clippy clean (`--all-features --all-targets`)

---

## v0.1.7 — Production-Hardening Complete, Publication Pending (branch: 0.1.7)

- [x] `oxigdal-cloud-enhanced`: real Azure IMDS managed-identity tokens (`azure_identity::ManagedIdentityCredential`) replacing placeholder-token stub
- [x] `oxigdal-cloud-enhanced`: real GCP metadata-server access/identity tokens + IAM Credentials impersonation (`GCE_METADATA_HOST` overridable, mock-server tests); `reqwest` optional under `gcp` feature
- [x] `oxigdal-cloud`: multicloud `build_backend()` factory (S3/GCS/AzureBlob/Http, feature-gated) + backend cache; `get`/`put`/`delete`/`exists_in_provider` now functional
- [x] `oxigdal-drivers-advanced`: JPEG2000 decode delegates to `oxigdal-jpeg2000` (real decode, full header parse) instead of gray placeholder pixels; `jpeg2000` feature now dep-gated
- [x] `oxigdal-services`: WFS-T Memory/File transactions fully implemented (insert/update/delete/replace, per-path write serialization)
- [x] `oxigdal-services`: Database transactions/feature-sources/SQL count behind new non-default `postgis` feature (`oxigdal-postgis` pool, `ST_GeomFromGeoJSON`/`ST_AsGeoJSON`)
- [x] `oxigdal-services`: WCS File/Url/Memory coverages real (GeoTIFF read/write via `oxigdal-geotiff`), Url fetch behind new non-default `remote` feature; `encode_as_geotiff` produces real GeoTIFF bytes
- [x] `oxigdal-ml-foundation`: `onnx_export.rs` — pure-Rust ONNX protobuf encoder (ir_version 8, opset 13), round-trip-validated against `oxionnx`
- [x] `oxigdal-ml-foundation`: honest typed errors for unavailable scirs2 input gradients (no more silent zero gradients); weights save/load via `oxicode`; augmentation noise now real Gaussian sampling (`scirs2_core` seeded RNG)
- [x] `oxigdal-workflow`: Temporal/Prefect `import_workflow` round-trips exporter-generated definitions (metadata headers for lossless ID recovery); export emits real activity bodies
- [x] `oxigdal-etl`: `transform_crs` implemented via `oxigdal_proj::transform_epsg` (spawn_blocking to avoid nested-runtime panic — real bug fix, previously panicked inside any tokio runtime); `calculate_bbox` fixed (was returning `[0,0,0,0]`); `calculate_ndvi` implemented (zero-denominator guard)
- [x] `oxigdal-cli`: `info`/`stats` implemented for FlatGeobuf, GeoParquet, Zarr, GeoPackage, JPEG2000, COPC, PMTiles, MBTiles (was "not yet implemented"); `merge` placeholder test replaced with real assertion
- [x] `oxigdal-algorithms`: Lanczos resampling Wrap & Mirror edge modes implemented (`rem_euclid` / reflect-101)
- [x] `oxigdal-gpkg`: tile matrix set `srs_id` now writes real 4326 via new `int2_st()` (was placeholder `4`)
- [x] `oxigdal-geojson-stream`: TopoJSON writer now emits real arcs for LineString/MultiLineString (open-chain topology: endpoint junctions, no-rotation splitting, shared-arc dedup with negative reversed indices) — was `"arcs": []` stub
- [x] `oxigdal-gpu`: subgroup/warp operations emit native WGSL subgroup builtins with workgroup-shared-memory emulation fallback; Metal filter/reduction/nn shader generators implemented; ballot/vote/`SimdGroupOperations` upgraded; new execute-and-compare GPU tests (verified on Metal)
- [x] `oxigdal-bench`: raster/io scenarios now do real work (tile reads, `MmapDataSource`)
- [x] `oxigdal-wasm`: new browser APIs — `WasmCogViewer.openBytes` (drag-drop local GeoTIFF with full codec support incl. LZW/Zstd via `CogReader<MemorySource>`), `readTileElevation` (SampleFormat tag 339 parsing), `WasmTerrain` (hillshade/multidirectional/slope/aspect/color-relief-shaded — Horn method, `ImageData` output), `WasmProjection` + `wgs84ToWebMercator`/`webMercatorToWgs84` shims
- [x] GeoLab demo: `demo/cog-viewer` rebranded OxiGDAL GeoLab, drag-drop, terrain-analysis panel, honest byte counters, all CDN deps vendored locally; staged to cooljapan.tech/geolab/ (deploy manual)
- [x] `oxigdal-server`: new example `render_hero.rs` (DEM → combined_hillshade → colormap → PNG)
- [x] README: compile-correct quickstart (`crs()` Option), refreshed stats, fixed doc links, clickable GeoLab hero + `## Demo` section with native-render gallery (`docs/media/`)
- [x] New: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`; docs.rs metadata on all 64 remaining publishable crates (21 curated for Pure-Rust docs builds)
- [x] Examples/benches: 31 orphaned top-level examples wired into `oxigdal-examples` (API rot fixed, 5 dupes pruned), 11 benches wired into `oxigdal-bench`
- [x] Hygiene: removed rustc-ICE dump, auto-fix logs/backups, 3 stray `.bak` files in crate `src` trees; `.gitignore` hardened; `.cargo/config.toml` stale rusqlite/proj-sys entries removed; `pypi-publish.yml` stale openssl-devel step removed; `pyproject.toml` + `package.json` synced to 0.1.7
- [x] `oxigdal-security`: `enterprise`/`tls`/`attestation` feature split (default = all three; wasm32-clean attestation-only surface) + new `attestation` module — domain-separated blake3 hash chain → Merkle root/proofs → Ed25519 seal, full re-verification from attestation JSON alone, native `verify_attestation.rs` example
- [x] `oxigdal-wasm`: `sentinel` module (Earth Search STAC pair search, self-contained UTM↔WGS84, NDVI change-detection pipeline with Otsu/polygonize/Karney hectares), `vault` module (hash-chained session log + Ed25519 seal), `anomaly` module (Z-score/IQR/modified-Z/percentile ports); COG reader: overview-level reads (`read_tile_level`, `read_window_u16`/`read_window_rgb8`), PREDICTOR=2 undo, BitsPerSample/SampleFormat array-parse fix
- [x] `oxigdal-geoparquet`: `plan_pushdown()`/`execute_pushdown()` APIs (metadata-only planning + `ChunkReader` execution), GeoParquet 1.1 `covering.bbox` + plain-`bbox` struct detection, `AttributeFilter::Cmp` with Int64/Float64 literal coercion, multi-filter conjunctions
- [x] New crate `oxigdal-wasm-geoparquet`: browser range-request GeoParquet client (sparse `ChunkReader`, 64 KiB range coalescing, SQL `WHERE` lowering via sqlparser, `RecordBatch`→GeoJSON) — npm `@cooljapan/oxigdal-geoparquet`
- [x] Three new demos staged (deploy manual): GeoSentinel (cooljapan.tech/geosentinel/), GeoVault (/geovault/ — CSP zero-egress + independent verifier), GeoParquet Live (/geoparquet/ — live 5.9 GB VIDA dataset); GeoLab pkg refresh; README `## Demos` sections for all four

### Demo-campaign follow-ups (added 2026-07-13)

- [ ] `oxigdal-query`: feature-gate tokio so the SQL engine is consumable from wasm32; until then GeoParquet Live ships its own SQL `WHERE`-fragment lowering (`crates/oxigdal-wasm-geoparquet/src/filter_expr.rs`) as a deliberate workaround
- [ ] `oxigdal-geoparquet`: Overture and other zstd-compressed GeoParquet remain out of scope until a pure-Rust parquet zstd path exists (`zstd-sys` is C FFI — forbidden by Pure Rust Policy; snappy via pure-Rust `snap` is the supported codec)
- [ ] `oxigdal-wasm`: parallelize per-window tile fetches in the COG reader (verified 2026-07-16: `detect_changes` in `crates/oxigdal-wasm/src/sentinel/core.rs` still awaits `red_a`/`nir_a`/`red_b`/`nir_b` sequentially; currently sequential; observed sentinel-cogs S3 latency variance of 23 s – 250 s on cold reads makes serial fetches the long pole)
- [x] `oxigdal-wasm`: `GeoSentinel.trueColorRgba` returns a bare RGBA buffer — should return dimensions alongside the pixels (callers currently need a separate `overlayInfo` call) — done: `true_color_rgba()` (`crates/oxigdal-wasm/src/sentinel/core.rs`) now returns a `TrueColorImage` carrying `width`/`height` alongside the pixel buffer
- [x] `oxigdal-geoparquet`: `ScalarValue::Bool` predicates are stats-only — boolean scalars surface a type-mismatch error at execution time; implement boolean-column evaluation — done: `eval_scalar_comparison` (`crates/oxigdal-drivers/geoparquet/src/predicate.rs`) now evaluates `ScalarValue::Bool` against `Boolean` columns via the Arrow comparison kernel
- [ ] `demo/cog-viewer/examples.json`: stale remote URLs — the `elevation-tiles-prod` S3 bucket no longer sends CORS headers; refresh the example COG list (candidates: sentinel-cogs and other CORS-enabled open buckets)

### Known limitations carried forward

- [x] `oxigdal-python` `numpy.rs:586`: complex dtype not yet supported — done: `to_numpy_complex()` maps `CFloat32`/`CFloat64` buffers to native `numpy.complex64`/`numpy.complex128` (round-trip tested)
- [x] `oxigdal-proj`: `transform_epsg` opens the PROJ DB per-coordinate call — inefficient for bulk transforms; needs a reusable/cached `Transformer` path — done: opt-in `TransformerCache`/`TransformerKey` (`crates/oxigdal-proj/src/cache.rs`) provides an LRU-cached, thread-safe reusable transform path
- [x] `oxigdal-stac`: implicit `reqwest` feature should be promoted to a named feature (currently pulled in without an explicit flag) — done: `async` is now the real feature gating `dep:reqwest`; `reqwest` is kept only as a backwards-compatible alias
- [x] `oxigdal-cloud-enhanced`: docs.rs build shows only the default feature surface (Azure/GCP feature-gated APIs not visible in generated docs) — done: `[package.metadata.docs.rs]` now sets `features = ["aws", "azure", "gcp"]` so the full API surface is documented

---

## v0.1.7 production-hardening (2026-07-16) [COMPLETE]

Parallel multi-lane defect-sweep campaign following the Beyond-GeoLab campaign
(2026-07-13): **233 verified defects fixed across 69 crates**. Workspace green:
`cargo check` / `cargo clippy --workspace --all-features --all-targets` / `cargo fmt --check`
all clean, ~16,775 tests passing with 0 failures. See CHANGELOG.md's
"Fixed (production-hardening campaign, 2026-07)" entry for the full categorized list
(format drivers / algorithms / security / cloud & infra / bindings / no_std & platform).

Headline fixes:

- [x] GeoTIFF floating-point predictor (`Predictor=3`) decode + encode implemented (was a silent no-op corrupting float COG round-trips)
- [x] JPEG2000 MQ-decoder `INITDEC` brought into ITU-T T.800 Annex C spec conformance
- [x] GML `srsDimension` attribute parsing (3D geometries no longer silently treated as 2D)
- [x] VRT `FirstValid` multi-byte-sample compositing fix + `BandMath` `B10`+ variable substitution
- [x] Raster/DSL calculator NaN-safe optimizer (`x * 0` no longer discards NoData/Inf semantics)
- [x] Weiler-Atherton concave fallback made non-silent (mismatch surfaced rather than masked as a plausible result)
- [x] RBAC `resource_pattern` now enforced (was a privilege-widening bug — parsed but never consulted)
- [x] TOTP constant-time verify + ±1 time-step clock-skew tolerance window (RFC 6238 §5.2)
- [x] SQL `GROUP BY` executor implemented in `oxigdal-query` (was a no-op)
- [x] Umbrella `DatasetWriter::finalize()` now writes real formats or a typed error (was a fake `OXIG`-prefixed placeholder blob)
- [x] `server.toml` now actually loaded in Docker/k8s via `OXIGDAL_CONFIG` (was parsed then discarded)
- [x] `oxigdal-stac` feature wiring fixed — no longer pulls `aws-lc-sys` for consumers who never use the async surface
- [x] Kafka/Kinesis streaming commit-strategy & consumer-lease correctness fixes
- [x] `oxigdal-core` (no_std) now compiles under `--no-default-features --features alloc`
- [x] `oxigdal-drivers/hdf5` and `oxigdal-netcdf` re-backed by the real Pure-Rust `oxih5 0.1.4`
  / `oxinetcdf 0.1.4` crates (crates.io, no libhdf5/libnetcdf FFI) — replaces the prior
  `OXIGDAL_HDF5_METADATA_V1` JSON-sidecar placeholder that returned zeros for real `.h5`
  files; `oxigdal-netcdf` now reads genuine NetCDF-4/CF files. Public API unchanged
  (`Hdf5Reader::open`, `Attribute`/`AttributeValue`/`Datatype`/`Hdf5Version`/`Hdf5Writer`,
  `NetCdfReader::open`); 730 tests passing across the 4 affected crates, clippy clean

### Deferred to v0.2.0 (honest limitations)

The current code has safe error paths and correct fallbacks for all of the following — not
silent corruption — but the full fix is architecturally larger than this campaign's scope:

- [ ] **HDF5 v2/v3 superblock reading**: `oxih5` 0.1.4 fully reads v0-superblock `.h5` files;
  v2/v3-superblock files currently open but yield an empty tree (best-effort, never faked as
  populated data)
- [ ] **HDF5 big-endian source normalization**: `oxih5` 0.1.4 does not yet normalize
  big-endian-encoded source datatypes on read
- [ ] **HDF5 chunked/compressed write**: `oxih5` 0.1.4's writer produces contiguous real HDF5
  only — chunk and compression hints passed by callers are accepted but dropped (written
  values are correct, layout is not chunked/compressed)
- [ ] **NetCDF sub-group flattening**: `oxinetcdf` 0.1.4's reader surfaces the root group
  only; nested NetCDF-4 sub-groups are not yet flattened/exposed
- [ ] **NetCDF auto scale/offset/fill application**: `scale_factor`/`add_offset`/`_FillValue`
  are exposed as attributes but not automatically applied to decoded values (unchanged
  contract — same as prior releases)
- [ ] **JPEG2000 Tier-2**: packet/precinct/layer decode is not yet invoked by the reader —
  code-block bytes are still sliced by naive even division rather than driven by the real
  progression order (LRCP/RLCP/RPCL/PCRL/CPRL) from the COD marker. Tier-1 (EBCOT/MQ) decode
  is real; Tier-2 packet assembly, rate control, and ROI support are not
- [ ] **FlatGeobuf**: header and feature geometry use a custom ad-hoc binary layout, not real
  FlatBuffers — files are not interoperable with the upstream `flatgeobuf.fbs` schema
  (`header.fbs` + `feature.fbs`) despite the pure-Rust `flatbuffers` crate already being a
  workspace dependency
- [ ] **oxigdal-ml multi-channel tensors**: `buffer_to_ndarray()` cannot produce a
  multi-channel tensor because `RasterBuffer` is architecturally single-band; `Model::predict`
  / `predict_batch` do not yet accept `MultiBandBuffer` and stack per-band slices into a real
  `[1,C,H,W]` tensor (nor is there a symmetric multi-band `ndarray_to_buffer`) — the fix
  ripples across the trait and multiple call sites
- [ ] **oxigdal-python GIL release**: remaining `#[pyfunction]`/`#[pymethods]` entry points
  still hold the GIL for their full duration — `operations.rs` (`read`/`read_bands`/`write`/
  `clip`/`merge`/`translate`/`build_overviews`) and `algorithms.rs`
  (`histogram`/`gaussian_blur`/`median`/...) still need `Python::allow_threads` wrapping
- [ ] **Root integration-test suite**: 21 quarantined tests across 7 of 11 root-level
  integration files (~4,280 lines) exercise local stand-in parsers/fixtures instead of the
  real driver crates (NetCDF, HDF5, Shapefile, GeoParquet, and others) and are not
  compiled/run by `cargo test`/`nextest`
- [ ] **PowerManager SoC-specific power states**: `oxigdal-embedded`/`oxigdal-noalloc`
  `PowerManager::apply_*()` for HighPerformance/Balanced/LowPower/UltraLowPower/DeepSleep
  remain bookkeeping-only no-ops — CPU frequency scaling and peripheral clock/power gating are
  inherently SoC-specific (vendor clock/power controllers, not the ARM/RISC-V ISA) and cannot
  be implemented generically without per-vendor HAL crates. (Note: this is distinct from the
  no_std *build* itself, which is green — see "`oxigdal-core` ... now compiles" above.)
- [ ] **LERC2**: bit-stuffed external-decode path is not yet implemented
- [ ] **Publish-script verification**: MSRV 1.89 claim and crate-publish ordering/dry-run gate
  for `~/work/pub_oxigdal.sh`'s CRATES array remain unchecked (cross-repo, outside this
  workspace's ownership)

---

## v0.1.6 — Previous Release (2026-06-15) [COMPLETE]

- [x] Pure-Rust SQLite migration: `rusqlite`/`libsqlite3-sys` (C FFI) fully replaced by `oxisql-sqlite-compat 0.1.5` (Limbo engine) across db-connectors, gpkg, drivers-advanced, mbtiles, pmtiles
- [x] Policy fixes: `ring`, `rusqlite`, `rdkafka-sys` removed from default feature closure
- [x] native-tls → oxitls migration (pure Rust TLS stack)
- [x] ~35 inline deps migrated to `*.workspace = true`
- [x] `oxigdal-shapefile`: non-UTF-8 DBF encoding via `encoding_rs` (CPG/LDID support, PR #10)
- [x] `oxigdal-proj`: `wkt_to_proj_string()` — WKT→PROJ conversion (PR #9)
- [x] `oxigdal-cache-advanced`: W-TinyLFU + Count-Min Sketch cache eviction
- [x] `oxigdal-copc`: LiDAR waveform point formats 9/10 (`WaveformPacket`)
- [x] `oxigdal-drivers/hdf5`: HDF5 v2/v3 superblock parser + Jenkins hash
- [x] `oxigdal-index`: Delaunay triangulation (`triangulate()`, `Triangulation::convex_hull()`)
- [x] `oxigdal-qc`: Batch QC runner, GPKG/STAC/radiometric validators, per-sensor band ranges
- [x] `oxigdal-sensors`: Gaussian Maximum Likelihood Classifier
- [x] `oxigdal-streaming`: OxiStore-backed persistent `KvStateBackend`
- [x] `oxigdal-terrain`: GLCM texture derivatives, TPI variants, geomorphons, cost-distance/least-cost-path
- [x] `oxigdal-temporal`: Whittaker smoother + Savitzky-Golay filter for gap filling
- [x] `oxigdal-analytics`: permutation significance testing for Local Moran's I
- [x] `oxigdal-metadata`: DOI/INSPIRE metadata transform
- [x] Umbrella: GPX, KML, TopoJSON format support in `open()` / vector streaming
- [x] Dependency upgrades: scirs2 0.4.4→0.5.0, oxionnx 0.1.3→0.1.4, oxiarc 0.3.0→0.3.3, oxicode 0.2.3→0.2.4

---

## v0.1.5 — Previous Release (2026-05-22) [COMPLETE]

- [x] `oxigdal-gpu`: WGSL `RayMarchUniforms` layout fix — removed stray `_pad1: f32` that shifted every field by 4 bytes and caused the Metal compute kernel to read `max_steps` ≈ 1.05×10⁹, hanging `device.poll(wait_indefinitely)` for 120s+. Previously-timing-out `test_ray_march_gpu_matches_cpu_when_backend_present` now passes in 0.127s.

---

## v0.1.4 — Previous Release (2026-04-19) [COMPLETE]

- [x] Wave 1: Weiler-Atherton polygon clipping, Karney geodesic area, DE-9IM topology, marching squares contour extraction
- [x] Wave 1: ML migration ort → oxionnx (Pure Rust ONNX runtime)
- [x] Wave 2: R-tree enhancements (deletion, STR bulk load, k-NN priority queue, serialization)
- [x] Wave 2: SIMD resampling (AVX2+NEON), raster polygonization, topology-preserving simplification
- [x] Wave 2: NoAlloc geometry types (FixedLineString, FixedRing, BBox3D, Mercator, geohash neighbours)
- [x] Wave 2: PMTiles reader completion (tile retrieval, OxiARC decompression, FNV-1a dedup)
- [x] Wave 2: COPC reader, GeoPackage B-tree + 3D WKB
- [x] Fixes: pyo3 0.28 migration in oxigdal-python, geojson-stream test clippy cleanup

---

## v0.1.3 — Previous Release (2026-03-21) [COMPLETE]

- [x] Fixed wgpu 29 API breaking changes (Instance::new, bind_group_layouts)
- [x] Fixed libsqlite3-sys version conflict (rusqlite 0.37, proj-sys compat)
- [x] Fixed macOS librocksdb-sys DYLD rpath via .cargo/config.toml
- [x] Fixed 6 critical oxiarc-brotli bugs (patched via [patch.crates-io])
- [x] Fixed pipeline_builder.rs clippy redundant closure

---

## v0.1.2 — Previous Release (2026-03-17) [COMPLETE]

- [x] WASM enhancements and optimizations
- [x] npm publishing workflow for WASM bindings
- [x] Code growth to 540K SLoC (1,934 .rs files)

---

## v0.1.1 — Previous Release (2026-03-11) [COMPLETE]

### Core & Algorithms
- [x] Core geospatial types, traits, async I/O, Arrow buffers, no_std core
- [x] SIMD-optimized raster algorithms (AVX2, AVX-512, NEON)
- [x] Vector algorithms (topology, buffering, convex hull, spatial joins)
- [x] Raster algebra DSL (Pest grammar parser)
- [x] DSL statistical functions (median, mode, percentile)
- [x] DSL for-loop support with 1M-iteration OOM guard
- [x] calculator.rs refactor: 7 modules (ast/lexer/parser/optimizer/evaluator/ops/mod)
- [x] Terrain analysis: hillshade, slope, aspect, curvature, TRI, TPI, roughness

### CRS & Projections (oxigdal-proj)
- [x] Pure Rust PROJ: 20+ projections (UTM 1-60, Web Mercator, LCC, Albers, etc.)
- [x] 211+ EPSG definitions (all UTM zones, JGD2011, GDA2020, CGCS2000, polar)
- [x] WKT2 parser (ISO 19162:2019) with WKT1/ESRI WKT backward compatibility
- [x] Datum transformations (Helmert, Molodensky, NTv2, NADCON)
- [x] SIMD-vectorized batch transforms

### Format Drivers (11 formats)
- [x] GeoTIFF/COG: BigTIFF, overviews, DEFLATE/LZW/ZSTD/JPEG, float predictor
- [x] GeoJSON: RFC 7946, streaming, GeoArrow zero-copy
- [x] GeoParquet: Arrow-native, spatial predicate pushdown
- [x] Zarr v2/v3: sharding, codec pipeline, consolidated metadata
- [x] FlatGeobuf: Hilbert R-tree spatial indexing
- [x] Shapefile: SHP/SHX/DBF full attribute table
- [x] NetCDF: CF conventions, unlimited dims, groups
- [x] HDF5: chunking, compression, SWMR protocol
- [x] GRIB1/2: parameter/level tables
- [x] JPEG2000: tier-1 EBCOT decoder (MQ coder, 3-pass)
- [x] VRT: band math, source mosaicking

### Cloud & Storage
- [x] S3/GCS/Azure Blob backends with HTTP range
- [x] Pure Rust compression (oxiarc-deflate/zstd/lz4/bzip2/lzw)
- [x] Multi-tier cache (in-memory LRU, disk, Redis)
- [x] Delta encoding (all 10 data types)

### Enterprise & Infrastructure
- [x] Security: AES-256-GCM, ChaCha20-Poly1305, Argon2id, RBAC/ABAC
- [x] HA: Raft consensus, failover, leader election
- [x] Observability: OpenTelemetry, Prometheus, Jaeger
- [x] OGC server: WMS 1.3.0, WFS 2.0.0
- [x] API gateway: JWT, OAuth2, rate limiting

### Platform Bindings
- [x] WASM: WasmCogViewer, Huffman compression, < 1MB bundle
- [x] Python: PyO3/Maturin, NumPy array returns
- [x] Node.js: napi-rs, CJS + ESM
- [x] iOS/Android: Swift FFI, Kotlin/JNI
- [x] Embedded: no_std, heapless, embedded-hal

### CLI
- [x] `inspect`, `convert`, `buildvrt` commands
- [x] DEM terrain: hillshade, slope, aspect, TRI, TPI, roughness

### Quality
- [x] 0 unwrap() in production code (2 in non-compiled doc comments)
- [x] 0 clippy warnings, 0 rustdoc warnings
- [x] 0 todo!()/unimplemented!() stubs (except 4 in oxigdal-python, 1 in grib)
- [x] All files < 2,000 lines
- [x] All deps via workspace inheritance
- [x] Pure Rust default features (C/Fortran feature-gated)

---

## v0.2.0 — Target: Q3 2026

### Projections Expansion (100+ total)
- [ ] Add 80+ new projections to reach 100+ total
- [ ] Equidistant Conic, Sinusoidal, Mollweide, Robinson, Eckert IV/VI
- [ ] Cassini-Soldner, Gauss-Kruger extended zones
- [x] EPSG expansion to 500+ definitions (added 300+ extended definitions in oxigdal-proj/src/epsg/extended.rs)
- [ ] Grid shift files: OSTN15 (UK), RGF93 (France), DHDN/ETRS89 (Germany)

### JPEG2000 Tier-2
- [ ] Tier-2 packet decoder (layer/resolution/component/position progression)
- [ ] Rate control and quality layers
- [ ] ROI (Region of Interest) support
- [ ] JPEG2000 Part 2 extensions (JP2 boxes)

### GPU Expansion
- [ ] Additional compute shaders for raster operations
- [ ] GPU-accelerated reprojection
- [ ] GPU raster algebra evaluation
- [ ] Multi-GPU workload distribution improvements
- [ ] WebGPU compute shader compilation for WASM

### ML Pipeline Enhancements
- [ ] ONNX model hot-reload
- [ ] Inference caching with content-addressed storage
- [ ] Batch prediction with adaptive batch sizing
- [ ] Model versioning and A/B testing
- [ ] Foundation model fine-tuning workflows

### Test Coverage Expansion — DONE, all targets exceeded as of 0.1.7
(measured 2026-07-17 via `cargo test -p <crate> --all-features -- --list | grep -c ': test$'`)
- [x] oxigdal-node: 5 → 50+ tests (measured: 60)
- [x] oxigdal (umbrella): 8 → 50+ tests (measured: 257)
- [x] oxigdal-jupyter: 33 → 60+ tests (measured: 105)
- [x] oxigdal-services: 34 → 60+ tests (measured: 608)
- [x] oxigdal-metadata: 38 → 60+ tests (measured: 162)
- [x] Target: 10,000+ total tests (workspace at ~16,775 tests as of 0.1.7 production-hardening, see header above)

### Format Driver Improvements
- [ ] GeoTIFF: JPEG-in-TIFF decompression, LERC codec
- [ ] GeoParquet: nested geometry encoding, partitioned datasets
- [ ] Zarr v3: full sharding codec with partial chunk reads
- [ ] GRIB2: template-based product definition expansion
- [ ] NetCDF: CF conventions v1.11 full compliance

### API Ergonomics
- [x] `oxigdal::open()` universal format detection — implemented (`crates/oxigdal/src/open.rs`: cloud-scheme → magic-byte → extension detection, returns `OpenedDataset`)
- [ ] Builder pattern for all readers/writers
- [ ] Streaming iterator API for large datasets
- [ ] Unified error context with source file/line

---

## v0.3.0 — Target: Q2 2026

### Streaming v2
- [ ] Backpressure-aware stream processing with credit-based flow control
- [ ] Session window improvements with gap detection
- [ ] Exactly-once semantics for Kafka/Kinesis/Pub/Sub
- [ ] Stream-to-stream joins with temporal alignment
- [ ] Checkpoint-based recovery with minimal replay

### Cloud-Native Tile Server v2
- [ ] OGC Tiles API (replacing WMTS)
- [ ] OGC Features API Part 1 & 2
- [ ] Vector tile generation (MVT/Mapbox)
- [ ] Dynamic style rendering (Mapbox GL style spec)
- [ ] CDN-friendly caching headers

### Extended STAC Support
- [ ] STAC Extensions: eo, sar, view, projection, scientific
- [ ] STAC API conformance classes
- [ ] STAC collection-level aggregation
- [ ] STAC transaction extension (create/update/delete)

### Additional Formats
- [x] GeoPackage (Pure Rust SQLite reader)
- [x] MBTiles (vector tile archives)
- [x] Cloud Optimized Point Cloud (COPC)
- [x] PMTiles (single-file tile archive)

### Performance
- [ ] Adaptive tile size selection for COG
- [ ] Parallel I/O coalescing for cloud reads
- [ ] Memory-mapped file support for local reads
- [ ] Zero-copy Arrow IPC for inter-process communication

---

## v1.0.0 — Target: Q3 2026

### Stability & LTS
- [ ] Semantic versioning guarantee: no breaking changes until 2.0
- [ ] Minimum 24-month LTS maintenance commitment
- [ ] Migration guide from 0.x to 1.0
- [ ] Full API documentation with examples for every public item

### Enterprise Compliance
- [ ] SOC2 Type II audit trail
- [ ] GDPR data handling compliance documentation
- [ ] FIPS 140-2 cryptographic module validation
- [ ] FedRAMP authorization support

### Ecosystem Integration
- [ ] Conda-forge package
- [ ] Homebrew formula
- [ ] Docker Hub official images (Alpine, Debian)
- [ ] Kubernetes Helm chart for oxigdal-server
- [ ] GitHub Actions for geospatial CI/CD

### Documentation
- [ ] Complete API reference with examples
- [ ] Architecture decision records (ADRs)
- [ ] Performance tuning guide
- [ ] Cookbook: 50+ recipes for common geospatial tasks
- [ ] Video tutorials

---

## Ongoing / Cross-Cutting

### Dependency Maintenance
- [ ] Replace unmaintained transitive deps: rustls-pemfile, sled/fxhash, evcxr/json, indicatif/number_prefix
- [ ] Track and patch security advisories within 48h
- [ ] Keep Arrow ecosystem at latest stable (currently 58)
- [ ] Keep all COOLJAPAN deps (oxiarc-*, scirs2-core, oxiblas, oxicode, OxiFFT) at latest

### Code Quality
- [ ] Maintain 0 clippy warnings, 0 rustdoc warnings
- [ ] Maintain 0 unwrap() in production code
- [ ] Maintain all files < 2,000 lines
- [x] Increase test count toward 10,000+ — target exceeded (~16,775 tests as of 0.1.7 production-hardening, see "Test Coverage Expansion" above)
- [ ] Property-based testing (proptest) for core algorithms
- [ ] Fuzzing (cargo-fuzz) for format parsers (GeoTIFF, JPEG2000, GRIB)

### Platform
- [ ] RISC-V support (no_std)
- [ ] Redox OS compatibility testing
- [ ] WASM Component Model (wasm32-wasip2) support
- [ ] Python 3.13+ free-threaded mode testing

---

*Last updated: 2026-07-16*

## Stubs to implement (added 2026-06-12 by /cooljapan-stub-check)

- [x] `oxigdal-wasm`: `crates/oxigdal-wasm/src/tests/functions_3.rs:542` — implement full Huffman codec for wasm decompression path (or wire oxiarc equivalent)
  - Priority: P2 | Scope: medium | Hint: oxiarc
- [x] `oxigdal-wasm`: `crates/oxigdal-wasm/src/tests/functions_4.rs:249` — implement decompression roundtrip test
  - Priority: P2 | Scope: small | Hint: oxiarc
- [x] `oxigdal-python`: `crates/oxigdal-python/src/numpy.rs:586` — implement proper complex dtype mapping when pyo3 supports it — done: `to_numpy_complex()` (production-hardening campaign, 2026-07-16)
  - Priority: P2 | Scope: small | Hint: none
- [x] `oxigdal-algorithms`: `crates/oxigdal-algorithms/tests/texture_analysis_test.rs:212` — investigate GLCM computation on uniform rasters (empty matrix bug) and re-enable ignored test
  - Priority: P2 | Scope: small | Hint: none
- [ ] `oxigdal-workflow`: `crates/oxigdal-workflow/src/integrations/temporal.rs:27` — replace generated activity-logic TODO placeholder with real Temporal workflow activity bodies
  - Priority: P2 | Scope: medium | Hint: none
- [x] `oxigdal-drivers/jpeg2000`: `crates/oxigdal-drivers/jpeg2000/src/lib.rs:154` — implement JPEG2000 driver (tracked as TODO in module doc)
  - Priority: P2 | Scope: large | Hint: none

## Policy follow-ups (added 2026-06-13 by /policy-check)

### #1 No-unwrap purge — production code only (tests on infallible paths are policy-allowed)
- [x] **oxigdal**: 0 production `.unwrap()` (clean)
  - Verified: 36 src/ files scanned; all `.unwrap()` calls are inside `#[cfg(test)]` blocks.
    Cross-confirmed by checking every `.rs` file for unwraps before the first `#[cfg(test)]`
    annotation — result: zero hits. `oxigdal` is fully policy-compliant on #1.

## Stubs to implement (added 2026-06-22 by /cooljapan-stub-check)

- [x] **oxigdal** `oxigdal-python`: `crates/oxigdal-python/src/numpy.rs:586` — `TODO`: `Use proper complex dtype when pyo3 supports it` — done: `to_numpy_complex()` (production-hardening campaign, 2026-07-16)
  - **Priority:** P2  **Scope:** small  **Cross-project:** none
  - **Approach:** Map numpy complex64/complex128 arrays to the Rust Complex type instead of falling back, so complex raster/array data round-trips correctly.
  - **Risk:** pyo3 complex support is version-dependent; guard the mapping so unsupported builds give a clear error rather than corrupting real/imag layout.
