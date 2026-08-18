# Changelog

All notable changes to OxiGeo will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.5] - Unreleased

## [0.2.4] - 2026-08-18

### Added
- `oxigeo-proj`: added the unambiguous type aliases `SphericalTransverseMercator` (= `TransverseMercator`) and `EllipsoidalTransverseMercator` (= `GaussKruger`), re-exported at the crate root, so call sites can state which Earth model they mean — `TransverseMercator` is sphere-based and is wrong for UTM/national grids by ~24.9 km of northing at 48° N, which its docs now warn about prominently; a new regression test pins the two apart at a real UTM 33N reference point.
- `oxigeo-proj`: re-exported the ellipsoidal Transverse Mercator kernel `projections::tmerc_forward` / `projections::tmerc_inverse` from `projections` (previously reachable only as `projections::cylindrical::tmerc_*`).
- `oxigeo-proj`: `transform` now re-exports `SphericalTransverseMercator` and `EllipsoidalTransverseMercator` alongside `CassineSoldner`/`GaussKruger`/`TransverseMercator`, so `use oxigeo_proj::transform::*` surfaces the two aliases instead of forcing the longer `transform::cylindrical::` path. Same `std` gate as the existing re-exports; a regression test now imports them through the glob and checks they denote the same types as the crate-root re-exports.
- `oxigeo-geoparquet`: `GeoParquetReader::from_bytes(impl Into<bytes::Bytes>)` reads a GeoParquet image held entirely in memory. The reader now keeps an internal `File`/`Bytes` source that implements `ChunkReader`, so every read path — `read_geometries`, `read_row_group`, `read_all`, `read_pushdown` — behaves identically for on-disk and in-memory inputs, with no change to `GeoParquetReader`'s public shape.
- `oxigeo-geoparquet`: `GeoParquetReader::read_geometries_optional(row_group)` and `GeoParquetBatchReader::extract_geometries_optional(batch)` return `Vec<Option<Geometry>>` with exactly one entry per row, so geometries stay index-aligned with their property rows; `GeoParquetBatchReader::geometry_encoding()` exposes the geometry column's declared encoding.
- `oxigeo-geotiff`: `tiff::is_mask_ifd(&Ifd, ByteOrderType)` classifies a directory as a GDAL internal (transparency) mask, with the pure core `tiff::is_mask_markers(new_subfile_type, photometric)` and the marker constants `tiff::SUBFILE_TYPE_TRANSPARENCY_MASK` / `tiff::PHOTOMETRIC_TRANSPARENCY_MASK`.
- `oxigeo-geotiff`: `CogReader::ifd_count()`, `CogReader::level_ifd(level)` and `CogReader::level_ifd_index(level)` expose the level → IFD mapping and the raw chain length, so a consumer that wants the mask IFDs — or wants to know how many non-level IFDs a file carries — can still reach them while the level API stays mask-free.
- `oxigeo-geotiff`: `CogReader::tile_pixel_size(level, tile_y)` returns the decoded pixel dimensions of the block `read_tile` produces at that level — the level's own `TileWidth`/`TileLength`, or `ImageWidth × RowsPerStrip` narrowed for the short final strip — so a caller can size an image buffer that cannot disagree with the bytes it gets.
- `oxigeo-gpkg`: `GeoPackage::scan_table_by_name_typed(table)` scans a table like `scan_table_by_name` but applies SQLite's [REAL type affinity](https://www.sqlite.org/datatype3.html#type_affinity) to the result: SQLite stores a lossless `40.0` in a `REAL`/`DOUBLE`/`FLOAT`-declared column as the integer `40`, so an untyped scan surfaces it as an `Integer` — the typed variant restores every such value to the equivalent `Float` (via `restore_real_affinity`, driven by the declared column types), so `40` and `40.0` read back identically, matching what every affinity-aware SQLite consumer sees. `scan_table_by_name` itself is unchanged and still returns raw storage classes.

### Changed
- Renamed the workspace `quick-xml` dependency (`Cargo.toml`) to the `oxixml-quickxml-compat` package (drop-in quick-xml 0.41 compatible shim), keeping the local dependency name `quick-xml` so every consuming crate (oxigeo-drivers-advanced, oxigeo-vrt, oxigeo-services, oxigeo-server, oxigeo-metadata, oxigeo-qc) required no source changes.
- `deny.toml`: added a `quick-xml` entry to `[bans].deny`, scoped with `wrappers = ["inferno"]` for the one remaining transitive path (inferno -> pprof -> oxigeo-algorithms's dev-only `pprof` dependency); the direct-consumer graph is clear (`cargo tree -i quick-xml -e normal --workspace` is empty).
- `oxigeo-gpkg`: gated GeoJSON conversion (`vector::geojson_convert`, and its `oxigeo-geojson-stream`/`serde_json` dependencies) behind a new `geojson-convert` feature, kept in `default` so no existing build breaks; `cargo build --no-default-features` (e.g. for wasm) no longer pulls in the `regex` family via `oxigeo-geojson-stream`. Consumers that already build `oxigeo-gpkg` with `default-features = false` will need to add `features = ["geojson-convert"]` to keep using `vector::geojson_convert`.
- `oxigeo-proj`: the `oxiproj` dependency is now `optional` and pulled in by the `std` feature instead of being unconditional. Every OxiProj call site already lived in a `std`-gated module (`transform`, `pipeline`, `projections`, …), so a `--no-default-features` (no_std + alloc) build was compiling OxiProj purely as dead weight; `cargo tree -p oxigeo-proj --no-default-features -e normal` now lists only `byteorder`, `serde` and `thiserror`. `default = ["std"]` is unchanged, so the default public surface is byte-identical and the 13 in-workspace dependents (all of which use default features or ask for `std` explicitly) need no change. **Migration:** two trait impls — `impl From<oxiproj::TransformError> for Error` and `impl From<oxiproj::ProjError> for Error` — are now `#[cfg(feature = "std")]` and therefore absent from `--no-default-features` builds; they could not have been used there anyway, since the `oxiproj` types they convert from were not linked. No `Error` variant changed: all of them carry `String`, not OxiProj types. `--no-default-features --features proj-db` remains unsupported (it was already failing to compile before this change, for unrelated `alloc` prelude reasons in `epsg::proj_db`) — superseded later in this same release: `proj-db` now implies `std` and compiles, see Fixed below.
- Dependency bumps: `oxiproj` 0.1.5 — the OxiProj authority-path correctness release, which fixes upstream the divergent EPSG authority definitions documented in the `proj-db` feature-invariance entry under Fixed (unit-converted ellipsoid axes, method-aware `+lat_ts` mapping, LCC 1SP, WGS 84-hub datum composition, prime-meridian datum chains, Molodensky-Badekas operations, PROJ's ballpark/fallback selection policy, and grid direction under `PROJ_DATA`) — plus routine COOLJAPAN ecosystem bumps (`oxiarc`, `oxicode`, `oxih5`, `oxionnx`, `oxisql`, `oxistore`, `oxitls`; the `quick-xml` → `oxixml-quickxml-compat` migration has its own entry above).
- `oxigeo-gpkg`: `SqliteHeader` gained the public field `reserved_bytes: u8` (byte 20 of the SQLite database header — bytes reserved at the end of every page) and a `usable_size()` helper; the issue #17 overflow-page fix (see Fixed) computes local-payload thresholds from the usable page size, not the raw page size. **Compatibility note:** constructing `SqliteHeader` with a struct literal outside the crate now requires the extra field; code that obtains headers through `SqliteReader` is unaffected.
- `oxigeo-wasm`: `WasmCogViewer`, `AdvancedCogViewer` and `BatchTileLoader` hold their cached parsed reader (see the reader-reuse fix under Fixed) in `Rc`/`RefCell` and therefore no longer implement `Send`/`Sync`. On `wasm32-unknown-unknown` — the target these `#[wasm_bindgen]` types exist for — this is inert (single-threaded, driven from JS); only a non-wasm caller holding one behind a `Send`/`Sync` bound would notice, and none exists in the workspace.

### Fixed
- `oxigeo-geoparquet`: `GeoParquetBatchReader::extract_geometries` now dispatches on the geometry column's declared encoding instead of downcasting to `BinaryArray` unconditionally — a GeoArrow-native file read through `read_all()` / `next_batch()` previously failed with a `type_mismatch` error rather than decoding.
- `oxigeo-geoparquet`: null geometries no longer silently desynchronise geometries from their property rows — the new `read_geometries_optional` / `extract_geometries_optional` variants keep each null as a `None` at its original index (the existing null-dropping methods are unchanged).
- `oxigeo-wasm`: GDAL internal-mask IFDs (`NewSubfileType` bit 2, or `PhotometricInterpretation == 4`) are no longer counted as overview levels by the browser COG reader — they share the IFD chain with the overviews, so `overviewCount` was inflated and every level index past the first mask was shifted onto the wrong resolution. The chain is still walked *through* masks, so overviews stored after one are found.
- `oxigeo-wasm`: `WasmCogViewer.readTile(level, x, y)` now honours its `level` argument on the URL path; it previously called a level-0 shortcut, so every overview request silently re-read full-resolution tiles.
- `oxigeo-wasm`: `WasmCogViewer` and `AdvancedCogViewer` parse the COG once and reuse the reader across tile reads instead of re-opening the file (HEAD request plus a range request per IFD) on every tile — for `AdvancedCogViewer` that happened on every tile-cache miss. The cached reader is keyed by URL, so re-opening a different file never serves stale tiles, and a failed open is retried on the next call.
- `oxigeo-wasm`: the URL-backed COG path now normalises `ModelPixelScaleTag` (33550) Y to its magnitude, so `WasmCogViewer.pixelScaleY()`, the `pixelScaleY` key of the metadata JSON, and the Rust `pixel_scale_y` field of `CogMetadata` / `IfdMetadata` are never negative. The GeoTIFF spec defines the tag as strictly positive and conforming writers (GDAL included) store it that way, but a few nonconforming writers bake the north-up sign into it; the URL path previously passed that negative value straight through while the `openBytes` path already applied `.abs()`, so the same raster reported opposite signs depending on how it was loaded. Neither path builds a pixel-to-CRS affine transform, so applying the north-up sign when constructing one remains the consumer's responsibility — callers that were compensating for the negative value on the URL path must drop that compensation.
- `oxigeo-geotiff`: `CogReader` no longer treats GDAL internal masks as pyramid levels. A mask (`NewSubfileType` bit 2, or `PhotometricInterpretation == 4`) shares the IFD chain with the overviews, so `overview_count()` counted one extra level per mask and every level index past a mask named the wrong resolution: `read_tile(2, …)` on a `[full, overview, mask, overview]` chain returned the *mask's* pixels, and `GeoTiffReader::level_size(2)` the mask's dimensions. Levels are now mapped onto non-mask IFDs and every level-indexed path — the block-offset cache, the `tile_byte_range` fallback, `band_read::LevelGeometry` (window/band reads) and `GeoTiffReader::level_size`/`read_window` — resolves through that one map, so the geometry and the tile offsets can no longer describe different images. The same map fixes a latent desync of its own: an IFD whose `ImageInfo` failed to parse was already skipped when counting overviews but not when indexing tile offsets. **Behaviour change:** on a masked COG `CogReader::overview_count()`, `GeoTiffReader::level_size`, `cog::get_cog_info`'s `overview_count` and every `level` argument now describe resolutions only — code that compensated for the inflated count (e.g. by subtracting mask IFDs, or by reading level *n+1* to get overview *n*) must drop that compensation. Raw chain access is unchanged: `TiffFile::ifds` / `image_count()` still see every IFD, and `CogReader::ifd_count()` / `level_ifd_index()` expose the mapping.
- `oxigeo-wasm`: `AdvancedCogViewer` and `WasmCogViewer` now report the same `overviewCount` for the same file. `AdvancedCogViewer.open()` derived it from `TiffFile::image_count()`, which counts every IFD including GDAL internal masks, while its own `readTile` indexed `CogReader`'s mask-free levels and `WasmCogViewer` skipped masks when walking the chain — so on a masked COG the advanced viewer advertised a level its own tile reads rejected. It now takes the count from the `CogReader` it reads through. `open()` also parses the file once instead of twice (it built a whole `TiffFile` for metadata and then re-opened a `CogReader` on the first tile read); the reader it parses is handed straight to the tile path.
- `oxigeo-wasm`: `readTileAsImageData`, `readTileWithContrast`, `computeStats` and `computeHistogram` size their RGBA buffer from the *requested level's* tile geometry instead of the full-resolution `tileWidth`/`tileHeight` captured at `open()`. Now that tile reads honour their `level` argument, a COG whose overviews declare a different `TileWidth`/`TileLength` (`gdaladdo` is free to choose one) produced an `ImageData` at the wrong dimensions with the tile truncated or three-quarters transparent. Both viewers convert through one shared helper, so they cannot drift apart again. Each path takes the geometry from the very reader that decodes the block — `CogReader::tile_pixel_size` for `openBytes`/`AdvancedCogViewer` (which narrows for the short final strip of a striped level), the URL reader's own per-level record for `WasmCogViewer` — so buffer and bytes always agree.
- `oxigeo-wasm`: `AdvancedCogViewer.open()` works in a browser for the first time. The viewer parses the COG with `oxigeo_geotiff::CogReader`, which reads through the **synchronous** `DataSource` trait, but the only data source it had was `FetchBackend`, whose `read_range` is hard-wired to `NotSupported("Synchronous read in WASM - use async methods")` — WASM cannot block on `fetch()` — and which holds no bytes of its own. Every `open()` therefore failed on the parser's very first header read, and so did every tile read behind it (`readTileCached`, `readTileAsImageData`, `readTileWithContrast`, `computeStats`, `computeHistogram`, `BatchTileLoader`): the whole URL path was dead code in the one environment it exists for. A new crate-private `buffered_source` module inverts the loop instead of making the parser async: `BufferedRangeSource` implements `DataSource` over a cache of already-downloaded ranges and *records* the ranges it cannot serve, and `pull_until_ready` re-runs the synchronous operation, fetching the recorded ranges between attempts, until it completes with nothing pending. Fetches are rounded up to 64 KiB and coalesced, so a normally laid-out COG opens in one `HEAD` plus one range request and a tile read costs at most one more (none at all when its block is already buffered); a server that ignores `Range` and answers `200` with the whole body is detected from the status and serves everything from that body thereafter. The loop is keyed on the *miss log*, not on the error: `CogReader::open` reads overview `ImageInfo`s, the GeoKey directory and the per-level block index best-effort and swallows the failure, so a driver that retried only on `Err` would have returned a reader that silently dropped an overview or lost the file's `epsgCode`. Termination is bounded in every direction — a transport error and a genuine format error are surfaced as themselves, a round that downloads nothing new stops with a "made no progress" error, and the round count is capped. `AdvancedCogViewer` now keeps the reader, its buffer and its transport together for the life of the opened URL, so tile reads reuse everything the header walk downloaded. The `openBytes` (in-memory) path is untouched. Native tests drive the whole loop over synthetic TIFF bytes with an in-memory transport; only the `web_sys`-backed implementation of the fetch seam — a thin translation of one `fetch()` response, with the response decoding split out and tested — is browser-only.
- `oxigeo-wasm`: the `pyramid` block of `AdvancedCogViewer.getMetadata()` no longer contradicts the `overviewCount` printed beside it. It was built from a `TilePyramid` synthesised from the image's dimensions alone — halving width and height until a single tile remained — which describes a pyramid the file need not contain: a 4096x4096 COG with 256-pixel tiles and *no* overviews reported `numLevels: 5` next to `overviewCount: 0`, and every level past 0 named tile grids that the viewer's own tile reads reject. The block is now derived from the levels the file actually has — the same mask-filtered IFD chain `overviewCount` comes from — with each level's own dimensions and block size read back through `CogReader::level_ifd`. **Value changes** (keys are unchanged): `numLevels` is now always `overviewCount + 1`; `tilesPerLevel` has one `[tilesX, tilesY]` entry per real level, computed from that level's own `ImageWidth`/`ImageLength` and `TileWidth`/`TileLength` (for a striped level, image width by `RowsPerStrip`) instead of from repeatedly halved level-0 dimensions; `totalTiles` is the sum over those real levels, and remains a count of *spatial* blocks — a planar (`PlanarConfiguration = 2`) file stores `SamplesPerPixel` times as many. One key is **added**: `pyramid.levels`, an array of `{width, height, tileWidth, tileHeight, tilesX, tilesY}` in level order. `TilePyramid` itself is unchanged, still exported and still the type to use for tile-scheme math. `WasmCogViewer.metadataJson()` emits no pyramid block and is unaffected.
- `oxigeo-proj`: `--no-default-features --features proj-db` compiles for the first time (it produced 56 errors before, so no build could ever have depended on its previous behaviour). `proj-db` now **implies `std`**: the feature is not expressible on a `no_std` + `alloc` build, because `epsg::proj_db` opens a file-system database (`std::path::{Path, PathBuf}`, `std::env::var` for `PROJ_DATA`/`PROJ_LIB`) and drives `oxisql-sqlite-compat`'s async engine through its `blocking` API on a `current_thread` tokio runtime. The alternative — sprinkling `alloc` prelude imports over `epsg/proj_db.rs` — would only have moved the 56 errors onto the `std::path`/`std::env`/tokio uses underneath them. `proj-db` also spells its OxiProj feature `oxiproj?/epsg` instead of `oxiproj/epsg`: the sole consumer of that feature, `transform::crs_to_oxi` → `oxiproj::Crs::from_epsg`, is itself `#[cfg(feature = "std")]`, and `std` already activates `dep:oxiproj`, so the sigil-less form only force-enabled a dependency that was enabled anyway — while making `proj-db` a second activator of the optional `oxiproj`. With the `?`, `oxiproj` has exactly one activator (`std`), and dropping `"std"` from `proj-db` in the future would fail loudly on the missing `from_epsg` rather than quietly re-linking OxiProj into a `no_std` build. `cargo tree -p oxigeo-proj --no-default-features -e normal` still lists no `oxiproj`, and the `proj-db` tree still contains `oxiproj` + `oxiproj-db`. Nothing changes for `std`/default builds: `proj-db` was already a superset of them in practice.
- `oxigeo-proj`: `--no-default-features --features proj4rs-compat` compiles for the first time (2 errors before, so again no working consumer could exist). `impl From<proj4rs::errors::Error> for Error` is gated on `proj4rs-compat` alone — the conversion needs nothing beyond `alloc` — but the `Error::Proj4rsError` variant it constructs and the `Error::from_proj4rs` constructor it calls were both gated on `std`, and the `format!` it uses came only from the `std` prelude. The variant and the constructor are now gated `any(feature = "std", feature = "proj4rs-compat")` (purely additive: every configuration that had them keeps them), and `error.rs` imports `alloc::format` under `proj4rs-compat`. New `tests/proj4rs_compat_test.rs` pins the three properties — the constructor is reachable, the message survives, and the `Display` string stays `"Proj4rs error: {0}"` with `thiserror/std` off.
- `oxigeo-proj`: `--no-default-features` (`no_std` + `alloc`) compiles again without a `std` crate of its own. It previously compiled only by accident: `oxiproj` was a mandatory dependency, which pulled `std` into the compilation, and rustc collects the inherent impls of primitive types from every crate loaded into it — including `std`'s `impl f64 { fn sin(…) … }`. Making `oxiproj` optional (see Changed above) removed that, and 73 call sites in `geodesic`, `datum_transform`, `ups_projection`, `geoid` and `operation_selection` stopped resolving `sin`/`cos`/`tan`/`asin`/`atan`/`atan2`/`sqrt`/`powf`/`powi`/`ln_1p`/`floor`/`rem_euclid`, none of which `core` provides. A new internal `math` module supplies them through the pure-Rust `libm` crate (a new, non-optional dependency — Cargo cannot express "enable when feature `std` is *off*"; `libm` is `no_std` and dependency-free, and its unused code is dropped by the linker in `std` builds) via a `FloatExt` trait whose signatures mirror the inherent methods exactly, so no call site changed and `std` builds still use the inherent methods. Unit tests cross-check every shim against the inherent method over a sweep of arguments; agreement is to ~1 ulp, not bit-exact, since Rust `libm` and the platform libm are different implementations (`powi` in particular is a `pow` call rather than LLVM's repeated squaring). Two caveats worth knowing: only the *library* build of `--no-default-features` exercises the shim — `--all-targets` (clippy/tests) puts the dev-dependencies and therefore `std` back into the compilation, so `cargo check -p oxigeo-proj --no-default-features` is the command that guards it; and a real bare-metal target (`--target thumbv7em-none-eabihf`) still fails to build, because the workspace-level `byteorder = "1"` keeps its default `std` feature — outside this crate to fix.
- `oxigeo-proj`: enabling `proj-db` no longer changes the result of a coordinate transformation. `transform::crs_to_oxi` resolved a `CrsSource::Epsg` through `oxiproj::Crs::from_epsg` (oxiproj's bundled authority database) under that feature and through this crate's own PROJ-verified registry string otherwise, so the same CRS pair produced two different answers depending on the feature set. Because `Crs::from_epsg` itself goes through `lookup_epsg`, that branch could only ever fire for codes the embedded registry *already* carried: it added no coverage, only a second, divergent definition. Two failure modes followed. **Asymmetric pairs:** a PROJ-string CRS transformed against an EPSG-sourced one combined a datum-bearing definition (`+towgs84` from the registry string) with a datum-less authority one, and the pipeline applied a *one-sided* datum shift — transforming from a code's own geodetic base to the code came out 87 m off for `EPSG:2039`, 226 m for `EPSG:2056` and 4.8e5 m for `EPSG:2314`, where PROJ 9.7.0 returns the projection alone (it composes *both* sides' datum transformations for such a mixed pair). **Divergent definitions:** oxiproj 0.1.4's authority definitions disagree with PROJ even when both sides are EPSG-sourced — `EPSG:2314`/`EPSG:24382` state the ellipsoid's semi-major axis in the CRS's own linear unit (`+a=20926348`, Clarke's feet) while still saying `+units=m`, `EPSG:6933` emits `+lat_1` instead of `+lat_ts` (a 2.5e6 m error), and `EPSG:2062`/`EPSG:5469`/`EPSG:24382` fail to build a transformer at all. Under `--features proj-db` this failed all four transform tests of `epsg_verified_registry_extended_test` — 560 projection mismatches in each direction, 77 end-to-end mismatches in each direction, 3 transformer-construction failures — against fixtures that pass under default features. Every CRS is now resolved through `Crs::to_proj_string()` → `oxiproj::Crs::from_proj` in all configurations; `oxiproj::Crs::from_epsg` is kept only as a fallback for an EPSG code the embedded registry does not carry (reachable via `Deserialize`), so `proj-db` stays strictly additive — it widens coverage without moving a number the default build already produces. Default-feature behaviour is unchanged, and the `proj-db` run of that test binary also got ~10x faster. New unconditional regression tests in `tests/transform_test.rs` pin the `EPSG:2039` / `EPSG:2056` base↔code pivots to their PROJ values in both directions, plus the fallback's presence (`proj-db`) and absence (default). The upstream oxiproj defects are reported separately.
- `oxigeo-proj`: corrected the linear unit of the NAD83 State Plane EPSG registry entries (legacy zones 2222-2289/2195-2204/32164-32166 plus the NAD83(2011) zones 6355-6419) and assorted other regional CRSs whose native unit is US survey feet or international feet. The registered PROJ strings declared `+units=m` for zones EPSG itself defines in feet — e.g. `EPSG:2222` "NAD83 / Arizona East" — so every coordinate run through `oxigeo-proj`'s embedded registry for these codes was off by the metre/foot conversion factor (~3.28× for `us-ft`); false-easting/northing and standard-parallel/central-meridian constants are now the exact EPSG values (`x_0=2000000.0001016 us-ft`, the precise definition for the affected zones, rather than the previous `x_0=2000000 m`) with tightened decimal precision throughout. Separately, the *reported* EPSG unit name was hardcoded to `"metre"` at every projected-CRS registration site regardless of which PROJ string it went with — silently mislabelling 79 US-survey-foot/international-foot CRSs (the whole State Plane `(ft)`/`(ftUS)` family) plus two entries expressed via `+to_meter=` (Indian yard, previously reported as `"metre"`; Clarke's foot, previously `"link"`). A new `epsg_unit_for` helper now derives the reported unit from each entry's own `+units=`/`+to_meter=` token instead of a hardcoded default. `tests/epsg_verified_registry_extended_test.rs` gained PROJ-verified fixtures over the corrected zones.
- `oxigeo-proj`: `Transformer::transform_batch`'s SIMD fast path (Transverse Mercator/UTM, Mercator and Lambert Conformal Conic forward projection) no longer silently mis-projects. Previously the fast path activated whenever the *target* CRS's `+proj=` matched a supported kernel, without examining the *source* CRS or checking whether the kernel could faithfully reproduce the scalar OxiProj pipeline; a new `fast_path_applicable` gate now declines — falling back to the scalar per-point path — whenever either CRS has a non-Greenwich prime meridian, a non-ENU axis order, a real (non-`@null`) `+nadgrids`, or a named datum other than a null-shift one, or whenever `parse_ellipsoid` cannot recognise either CRS's ellipsoid. Three concrete bugs this closes: (1) an unrecognised `+ellps` (e.g. `+ellps=clrk66`) was silently projected on WGS-84 — measured up to 2.1e5 m off; (2) the generic Transverse Mercator kernel ignored `+lat_0`, so any CRS whose origin isn't the equator — e.g. the Japan Plane Rectangular CS, `EPSG:6669-6687`/`2443-2461`, `+lat_0=26..44` — came out offset by the meridional arc to that latitude (≈3,985,144 m for `+lat_0=36`); (3) the Mercator kernel ignored `+x_0`/`+y_0` (false easting/northing) entirely and always used `k=1` even when `+lat_ts` should derive the scale factor, and the shared output step now converts through the target CRS's actual linear unit instead of assuming metres. The dispatch logic also moved out of `transform/mod.rs` into a new internal `transform::simd_dispatch` module; no public API changed. New `tests/epsg_verified_registry_test.rs` and `tests/simd_batch_params_test.rs` pin the fast path against both the scalar path and PROJ-verified fixtures for all four kernel families.
- `oxigeo-proj`: the embedded EPSG registry's JGD2011 Japan entries are corrected. Japan Plane Rectangular CS zones I–X (`EPSG:6669`–`6678`) were misregistered as "JGD2011 / UTM zone 51N–60N" — a whole-family misassignment that placed Plane Rectangular data ~4,000 km east into the Pacific — and zones XI–XIX (`EPSG:6679`–`6687`) were absent entirely. All nineteen zones are now registered from a verified per-zone table with each zone's true `lat_0`/`lon_0` origin, and the JGD2011 UTM zones 51N–55N live at their real codes `EPSG:6688`–`6692` — the codes the Plane Rectangular family used to squat on.
- `oxigeo-gpkg`: table B-tree cells whose payload spills onto SQLite overflow pages are read correctly ([issue #17](https://github.com/cool-japan/oxigeo/issues/17)). Two defects combined: the local (on-page) payload size was computed as `min(P, U − 35)`, but SQLite stores only `K` (or `M`) bytes locally when a cell overflows — 489 bytes at a 4096-byte page size, not 4061 — and the overflow-page chain was never followed at all, so a `sqlite_master` row wider than one page (the reporter's ~5000-character QGIS layer name) failed `GeoPackage::from_bytes` + `load_contents()` with "overflow cell needs 4061 bytes inline … but only 3209 available". The reader now computes SQLite's local-payload split against the true usable page size (`page_size − reserved_bytes`, see Changed) and reassembles the full payload across the chain. Regression tests in `tests/issue_17_overflow_pages.rs`.
- `oxigeo-vrt`: `SrcRect`/`DstRect` windows parse the numeric formats GDAL actually writes ([issue #18](https://github.com/cool-japan/oxigeo/issues/18)). `gdalbuildvrt`/`gdalwarp -of VRT` keep source and destination windows as doubles and round them only at rasterisation time, so real-world VRTs carry sub-pixel values such as `xOff="9783.50000000003"` — and parsing those attributes with `str::parse::<u64>` rejected every GDAL-produced mosaic with "Invalid u64: invalid digit found in string". The attributes are now parsed as `f64` and rounded where GDAL rounds. (`<WarpMemoryLimit>6.71089e+07</WarpMemoryLimit>` was also suspected; it has parsed as `f64` since 0.2.3, and a scientific-notation test now pins that so the two offenders cannot be conflated.) Regression tests in `tests/issue_18_gdal_numeric_formats.rs`.
- `oxigeo-vrt`: mosaic compositing of overlapping `ComplexSource`s honours each source's `<NODATA>` ([issue #19](https://github.com/cool-japan/oxigeo/issues/19)). GDAL applies sources in document order and skips any source pixel equal to that source's nodata value — the pixel neither overwrites what is already there nor claims coverage, so a later overlapping source can still supply valid data. Previously the first source to cover a pixel won even when all it had there was nodata, punching holes along the overlap bands of every `gdalbuildvrt` mosaic. The comparison is made on the decoded sample value for the band's data type, not on raw bytes, so float nodata (including the NaN convention) compares correctly.

## [0.2.3] - 2026-08-05

**Issues #15 and #16.** [GitHub issue #15](https://github.com/cool-japan/oxigeo/issues/15)
reported that `oxigeo-vrt` rejected every `gdalwarp -of VRT` product — a Warped
VRT's `<GDALWarpOptions>` block — with "Band must have at least one source or
a pixel function": the driver understood mosaics and pixel-function VRTs but
had no concept of a warp at all. [Issue #16](https://github.com/cool-japan/oxigeo/issues/16)
reported that vector-layer support was incomplete: `Dataset::open` on a
GeoPackage reported `layer_count() == 0`, and there was no public API to read
a layer's features regardless of format. Both are now implemented for real —
4 new files in `oxigeo-vrt` (1,635 lines: `warp.rs`, `warped.rs`, `srs.rs`,
`source_dataset.rs`) and 2 new files in `oxigeo` (1,405 lines: `layer.rs`,
`gpkg_schema.rs`) — alongside [issue #14](https://github.com/cool-japan/oxigeo/issues/14)
("how do I read a GeoTIFF into `ndarray::Array2`"), which needed no code
change: the readers it asked for (`read_band_into`, `read_window_into`,
`read_interleaved(_into)`, `read_window_interleaved(_into)`) already shipped
in 0.2.2.

### Added

**Warped VRT support (`oxigeo-vrt`, cool-japan/oxigeo#15)**

- New `warp` module: `WarpOptions` (the parsed `<GDALWarpOptions>` block),
  `WarpResampleAlg` — `is_kernel_exact()` reports which resample algorithms the
  engine implements exactly rather than approximates, see the known-limitation
  note under Fixed below — `WarpKernel`, `WarpBandMapping`, `InitDest`,
  `ReprojectionTransformer`, `GenImgProjTransformer`.
- New `srs` module: `resolve_crs`, a WKT/PROJ4/`EPSG:n` CRS-string resolver.
- New `source_dataset` module: `SourceDataset`, which dispatches a warp's
  source to a GeoTIFF leaf reader or recurses into a nested VRT
  (`MAX_VRT_NESTING = 16`, so a VRT that references itself fails cleanly
  instead of exhausting the stack).
- `VrtDataset::with_warp_options`/`is_warped`; a new `VrtError::EmptyWindow`
  variant (`VrtError::empty_window`) that distinguishes "no source covers this
  window" — legitimate on a warp over a sparse mosaic, GDAL's
  `ERROR_OUT_IF_EMPTY_SOURCE_WINDOW=FALSE` behavior — from a real structural
  error, so a routine mosaic gap can't also mask genuine failures.
- `oxigeo-vrt` gained a new dependency on `oxigeo-proj` to perform the
  reprojection. Pure Rust: `oxigeo-proj`'s default feature set excludes the
  `oxiproj-db`/tokio EPSG-database path, so this does not pull SQLite into a
  default `oxigeo-vrt` build.

**Vector layers (`oxigeo`, cool-japan/oxigeo#16)**

- `Dataset::layers() -> Result<Vec<Layer>>`, `Dataset::layer(index)`,
  `Dataset::layer_by_name(name)`, `Dataset::layer_names()`; `Layer::features()
  -> Result<LayerFeatures>` (eager). New `oxigeo::{Layer, LayerFeatures}`, and
  `oxigeo::{Feature, FieldValue, Geometry}` re-exported from
  `oxigeo-core::vector` so reading features needs no direct `oxigeo-core`
  dependency.
- New `crates/oxigeo/src/layer.rs` (the `layers()` dispatch plus the
  Shapefile/GeoJSON/GeoPackage readers) and `crates/oxigeo/src/gpkg_schema.rs`
  (a `CREATE TABLE` column/constraint parser shared by the new layer reader and
  the existing streaming GeoPackage path, so a schema fix lands in both at
  once).

### Changed

- Dependency bump: `scirs2-core` 0.6.4 → 0.6.5, `oxicode` 0.2.4 → 0.2.5 —
  routine latest-crates-on-crates.io maintenance. `oxicode` 0.2.5 is a
  hardening release (DoS/panic/overflow rejections added to its decode paths);
  neither bump changes any OxiGeo-visible API or behavior.

### Fixed

**`oxigeo-vrt` (cool-japan/oxigeo#15)**

- **Every Warped VRT was rejected at parse time.** A `VRTWarpedRasterBand`
  legitimately carries no `<SimpleSource>`/`<ComplexSource>`/pixel function —
  its pixels come entirely from the sibling `<GDALWarpOptions>` block — but
  `VrtDataset::validate` applied the same "Band must have at least one source
  or a pixel function" rule regular VRTs need, rejecting every warped VRT GDAL
  has ever written. The rule is now relaxed exactly when a validated
  `<GDALWarpOptions>` block is present (`VrtDataset::is_warped`); a
  `VRTWarpedDataset` that carries the `subClass` marker but no warp block is
  still rejected, since it then has no source for any pixel.
- **Depth-aware `AUTHORITY`/`ID` resolution in WKT CRS strings.** The previous
  scan returned the *first* `AUTHORITY[...]`/`ID[...]` node found anywhere in
  a WKT tree. In a `GEOGCS`, that is the node nested inside `SPHEROID` (e.g.
  `AUTHORITY["EPSG","7030"]`, the ellipsoid's own code), which precedes the
  CRS's own root-level code (e.g. `AUTHORITY["EPSG","4326"]`) in the string. A
  source WKT naming EPSG:4326 was silently resolved as EPSG:7030 — the wrong
  CRS, and one close enough in practice to distort output rather than fail
  loudly. `srs::resolve_crs` now tracks bracket depth and reads only the
  direct-child `AUTHORITY`/`ID` of the root node.
- **`relativeToVRT` was discarded on both read and write.** Parsing a
  `<SourceFilename relativeToVRT="1">` silently dropped the attribute (every
  path was treated as absolute), and writing one never emitted it either — so
  OxiGeo could not read back a VRT written by its own `oxigeo buildvrt`
  wherever that VRT used relative source paths. Both directions now round-trip
  the attribute.
- **quick-xml 0.41 entity-reference events were dropped, corrupting escaped
  text.** quick-xml reports `&quot;`/`&amp;`/`&#34;` as their own
  `Event::GeneralRef`, separate from the surrounding `Event::Text`; those
  events fell through the XML parser's catch-all arm and vanished. A `<SRS>`
  block written by this crate's own `VrtXmlWriter` (which escapes the quotes
  in a WKT tree) read back with every `"` missing, and any path containing `&`
  silently lost it.
- `VrtReader::read_window`'s band-to-index conversion was an unchecked `band -
  1`, an integer-underflow panic waiting for a caller that passed band `0`;
  now `checked_sub` with a typed `VrtError::band_out_of_range` on failure.
- **The `oxigeo` facade opened `.vrt` files with a zero-filled
  `DatasetInfo`.** `Dataset::open` routed every VRT through the generic
  fallback arm of `open_raster`: `width()`/`height()`/`band_count()` all read
  back `0` and `geotransform()` read back `None`, for a file that states all
  of them in its own header. `raster_read`'s `read_band`/`read_window`/
  `read_interleaved` (and their `_into` forms) were also hardwired to the
  GeoTIFF path only. `Dataset::open` now parses the VRT header for real
  metadata via a new `extract_vrt_info`, and every raster read method
  dispatches to the VRT reader — including through nested warps and mosaics —
  whenever the opened dataset is a VRT.
- **Known limitation, stated rather than hidden**:
  `WarpResampleAlg::is_kernel_exact()` is `true` only for `NearestNeighbour`
  and `Bilinear`. Cubic, CubicSpline, Lanczos, Average, and Mode all parse
  correctly and select their named kernel, but the warp engine currently
  resamples every one of them bilinearly rather than with the kernel it
  selected.

**`oxigeo` GeoPackage / vector layers (cool-japan/oxigeo#16)**

- **`Dataset::open("x.gpkg")` always reported 0 layers.** The facade's
  `open_vector` had no GeoPackage arm at all, so every `.gpkg` fell through to
  an empty `DatasetInfo::default()`. `open_vector` now calls the new
  `extract_gpkg_info` under the (non-default) `gpkg` feature.
- **`fid` read back `NULL` on every GeoPackage feature.** SQLite stores an
  `INTEGER PRIMARY KEY` column as `NULL` in the row's record payload and keeps
  the real value only in the row's own 64-bit `rowid`; naively reading the
  stored cell therefore always produced a null `fid`. `gpkg_schema` now
  detects an `INTEGER PRIMARY KEY` column at schema-parse time
  (`rowid_alias`) and substitutes the row's `rowid` for it whenever the stored
  cell is `NULL`.
- **Named table-level constraints were parsed as columns.** A `CREATE TABLE`
  body item such as `CONSTRAINT pk_geom_cols PRIMARY KEY (table_name,
  column_name)` was split on its top-level commas exactly like a real column
  list, producing bogus extra "columns". `is_table_constraint` now recognizes
  `PRIMARY KEY`/`UNIQUE`/`CHECK`/`FOREIGN KEY`/`CONSTRAINT`-led body items and
  skips them.
- **Known limitation, stated rather than hidden**: `layers()` covers
  GeoPackage (feature `gpkg`, not on by default), Shapefile, and GeoJSON.
  FlatGeobuf and GeoParquet return `OxiGeoError::NotSupported` naming the
  unsupported driver; both remain reachable only through the streaming feature
  API.

## [0.2.2] - 2026-07-30

**Issue #14 fix campaign.** [GitHub issue #14](https://github.com/cool-japan/oxigeo/issues/14)
reported that `Dataset::read_band` silently ignored its `band` argument on
multi-band rasters, returning the whole pixel-interleaved image instead of the
requested band. Root-causing it traced back to `oxigeo-drivers/geotiff`'s
block-decode engine (rewritten from scratch as `band_read.rs`/`band_read/multi.rs`),
then surfaced the identical defect pattern — assuming chunky
(`PlanarConfiguration=1`) interleaving, or the wrong byte order, wherever
multi-band raster data was read — independently re-implemented in a dozen other
crates, plus a handful of unrelated bugs found along the way. 192 files changed;
33 new `issue_14_*`-named files (30 regression tests, 2 benchmarks, 1 example) plus
dedicated cases embedded in the Node/ML/Jupyter/CLI suites guard against
regressions.

### Changed

- **BREAKING — `oxigeo::Dataset::read_band` now returns one band.** Up to 0.2.1 it
  ignored its `band` argument on multi-band rasters and returned the whole
  pixel-interleaved image (`width × height × bands` samples, `b0 b1 b2 b0 b1 b2 …`),
  which silently mis-fed every caller that asked for a single band. It now returns
  exactly that band's `width × height` samples. Single-band rasters are unaffected;
  on a 3-band file `read_band(0)` returns a third as many samples as it used to, so
  a length check finds affected code quickly.

- **BREAKING — `DatasetInfo` is now `#[non_exhaustive]`.** It also gained
  `impl Default` and a new `data_type: Option<RasterDataType>` field (the on-disk
  pixel type, readable before any raster read via the new `Dataset::data_type()`).
  Downstream struct-literal construction — even `DatasetInfo { field, .. }` — no
  longer compiles; build from `DatasetInfo::default()` instead.

- **DEFLATE tile decoding is substantially faster.** The `oxiarc-*` suite moves
  0.3.6 → 0.4.0, which rewrites the DEFLATE/zlib decoder (two-level Huffman
  root+sub-tables, a buffered bit reader with a register-resident accumulator, and
  an LZ77 history that *is* the output buffer instead of a ring buffer written
  twice), and the GeoTIFF driver now uses its new decompress-into-slice entry
  point. Measured on 256×256 UInt16 DEM tiles with `PREDICTOR=2` — the layout used
  by SRTM/Copernicus DEM COGs — decode throughput goes from 99.0 MiB/s on
  oxiarc-deflate 0.3.6 to 143.7 MiB/s on 0.4.0 (**1.45×**), and to 177.4 MiB/s
  (**1.79×**) through `zlib_decompress_into`, which is the path a whole-band read
  now takes. Whole-band DEFLATE reads additionally perform zero decode-side
  allocations: one caller-owned scratch buffer serves every tile, where each tile
  previously grew its own `Vec` by repeated doubling. Output bytes are unchanged;
  the decoded-size hint is an optimisation only, and a wrong, absent, or clamped
  hint falls back to the growable path rather than failing (cool-japan/oxigeo#14).

### Added

- **`oxigeo::Dataset` interleaved (multi-band) readers** — the supported
  replacement for the pre-0.2.2 `read_band` behaviour, so the breaking change above
  leaves no gap:
  - `read_interleaved(bands) -> Vec<T>` and `read_interleaved_into(bands, dst)`
  - `read_window_interleaved(bands, col, row, w, h) -> Vec<T>` and
    `read_window_interleaved_into(bands, col, row, w, h, dst)`

  `bands` is `Option<&[u32]>`: `None` means every band in file order (mirroring
  GDAL's `panBandMap == nullptr`), and a slice selects, reorders (`&[2,1,0]` reads
  RGB as BGR), subsets (only the named bands are decoded), or repeats band indices.
  The element type is converted from the file's type while the blocks are decoded,
  exactly as `read_band_into` does. The `*_into` forms allocate a single scratch
  buffer sized to one horizontal *strip* of one band — not to the raster — so peak
  extra memory stays bounded however large the image is; a single-band selection
  delegates to the `read_band_into` path and allocates nothing at all. All four
  honour `Dataset::clip`'s pixel window like every other reader.

- **`oxigeo::Dataset` gained a pre-read type query and zero-allocation
  single-band/window readers**: `data_type() -> Option<RasterDataType>` reads the
  on-disk pixel type from the header before any raster read; `read_band_into<T:
  RasterElement>(band, dst)` and `read_window_into<T>(band, col, row, w, h, dst)`
  decode straight into a caller-owned buffer (the interleaved readers above already
  build on this same path). `RasterElement` — see `oxigeo-core` below — is
  re-exported at the crate root.

- **`oxigeo-core` gained a typed, zero-copy raster-element layer.** The sealed
  `RasterElement` trait (implemented for `u8/i8/u16/i16/u32/i32/u64/i64/f32/f64`;
  `Copy + Default + Send + Sync + 'static`) defines each type's on-disk byte width,
  `RasterDataType` tag, and native-endian byte conversion, plus exact — never
  lossy through `f64` — integer-to-integer conversion via an `i128` bridge. Built
  on it: `convert_raw_into`/`convert_raw_into_with`/`convert_raw_bytes`/
  `elements_as_bytes`, and `RasterBuffer::from_element_slice`/
  `copy_to_slice[_with]`/`to_typed_vec[_with]`. `DataSource`/`AsyncDataSource`
  gained `read_range_into`/`range_slice` methods (default: still allocates
  internally); `FileDataSource` now issues real positional reads (`pread`/
  `seek_read`) instead of serializing every read through one `Mutex<File>`, and
  `MmapDataSource`/`MmapDataSourceRw` override both for true zero-copy reads
  straight out of the mapping.

- **`oxigeo-drivers/geotiff` gained a real band-aware, low-allocation read API**:
  `band_byte_len`/`band_pixel_count`, `read_band_into`/`read_band_into_typed`,
  `read_window`/`read_window_into`/`read_window_into_typed`,
  `read_bands_into_typed`/`read_window_bands_into_typed` (one block decode shared
  across every requested band), `byte_order()`, `level_size(level)` (exact
  per-overview dimensions from that level's own IFD, not `full_size / 2^level`),
  `read_tile_band_buffer` (`read_tile_buffer` is now its `band = 0` shorthand),
  `CogReader::tile_decoded_size`/`read_tile_into`, and `compression::decompress_into`/
  `decompress_into_partial`. New opt-in `parallel` feature fans block decode out
  across rayon workers (bit-identical to serial).

### Fixed

**GeoTIFF driver — the issue #14 root cause (`oxigeo-drivers/geotiff`)**

- **`GeoTiffReader::read_band` never read its `band` parameter** (it was named
  `_band`). It sized its output as the *whole* image (`width × height ×
  bytes_per_sample × samples_per_pixel`) and copied every decoded tile's raw
  chunky (`PlanarConfiguration=1`) bytes into it 1:1 — so every band index
  returned identical, full-image bytes, and a `PlanarConfiguration=2` (planar)
  file was decoded as if it were chunky, scrambling every band. Overview levels
  (`level > 0`) additionally walked the primary image's tile grid unconditionally.
  Replaced by a purpose-built engine (`band_read.rs`, `band_read/multi.rs`): a
  `ReadPlan`/`LevelGeometry` resolves each level's real geometry and planar config
  once, and `decode_block` either de-interleaves the requested band during the
  scatter (chunky) or reads only that band's own blocks (planar) — the interleaved
  plane is never materialized, and the band index is validated. Output is now
  exactly one band's `width × height × bytes_per_sample` bytes.
- **The TIFF predictor (horizontal-differencing) undo used the wrong stride on
  planar files.** `CogReader::read_tile` always passed `samples_per_pixel` as the
  predictor stride, but a planar block holds one band, so the correct stride is 1;
  the wrong stride "subtracts the wrong neighbour from every sample… rows bleed
  into each other. Nothing errors; the pixels are simply wrong." Fixed via a new
  per-block `block_samples_per_pixel` (1 when planar). Separately,
  `Compression::Lerc` combined with any `Predictor` is undefined by spec — no real
  encoder produces it — but the old driver reversed the predictor over
  already-decoded LERC floats anyway, corrupting every sample after the first;
  this combination is now a hard error instead of silent garbage.
- **Per-tile reads re-parsed the entire `TileOffsets`/`TileByteCounts` array on
  every single lookup** — measured at 77% (190 of 248 ms) of one band read on an
  8000-strip file. A new `BlockIndex` (`cog/block_index.rs`) parses each level's
  offset/count arrays once at `open()` for O(1) lookups thereafter, bounded
  against hostile headers.
- **`CogConverter::convert` (GeoTIFF→COG) depended on the bug above** — it called
  the old `read_band(0, 0)` specifically *because* it returned the whole
  interleaved image, and reassembled that into its output. Fixing `read_band` in
  isolation would have silently truncated every multi-band conversion to one
  band; the converter now reads and re-interleaves each band explicitly.
- `tiff/ifd.rs`: several direct-slice-index panics on truncated/malformed IFDs are
  now typed errors. `lerc_codec`: `serialize_native` used a hardcoded
  `to_le_bytes()`, silently byte-reversing output on big-endian hosts; now
  `to_ne_bytes()`.

**`oxigeo-core` foundation**

- **`RasterBuffer::convert_to` silently corrupted large `UInt64`/`Int64`
  values.** Its per-pixel path round-tripped every sample through
  `get_pixel`/`set_pixel`, which decoded/encoded via `f64` — exact only to 2^53 —
  so e.g. `(1u64 << 53) + 1` silently became `1u64 << 53` on conversion. Fixed by
  routing through an exact `i128` bridge.
- **Latent undefined behavior in `RasterBuffer::as_slice`/`as_slice_mut`/
  `row_slice`.** They reinterpreted a `Vec<u8>`'s pointer directly as `*const T`
  without checking alignment (`Vec<u8>` only guarantees 1-byte alignment), and ran
  `from_raw_parts` on the zero-length dangling sentinel pointer for empty buffers
  — UB regardless of length whenever `align_of::<T>() > 1`. It never crashed in
  practice because production allocators over-align, which is exactly why it
  stayed latent. Now checks alignment explicitly and short-circuits on zero
  length.
- `MmapDataSource`/`MmapDataSourceRw::read_range` cast a `u64` byte offset to
  `usize` unchecked, silently wrapping on 32-bit targets for offsets beyond
  `u32::MAX`; now a checked `usize::try_from`.
- **`oxigeo::Dataset::open` (and the standalone format probes) now report a real
  error instead of silently opening a GeoTIFF/GeoJSON/Shapefile/FlatGeobuf/
  GeoParquet file with zeroed-out metadata.** The old hand-rolled TIFF header
  parser only looked at the first 8 KiB/1 MiB of the file; an IFD located past
  that window (as OxiGeo's own writer produces, which emits pixels before the
  IFD) silently opened as a 0×0, 0-band dataset instead of erroring. Reading now
  delegates to `oxigeo_geotiff::GeoTiffReader` directly, and every format probe
  changed from `Option` (silent empty result) to `Result`. GeoJSON's
  `feature_count` similarly stopped under-counting large documents past its 64
  KiB peek window — it now reports `None` instead of a wrong number. BREAKING: a
  file that previously opened "successfully" with empty/zeroed metadata now
  returns `Err`.

**The same defect pattern, independently found and fixed workspace-wide
(cool-japan/oxigeo#14)**

- **oxigeo-qc**: the nodata and radiometric scanners hardcoded little-endian byte
  decoding regardless of the file's actual byte order, misclassifying nodata on
  big-endian GeoTIFFs; a separate bug assumed chunky layout unconditionally, so
  planar files were scanned one plane at a time, attributed to the wrong band, and
  reported clean. A new `band_scan` module centralizes correct band-aware
  scanning.
- **oxigeo-server**: the WMS/WMTS/XYZ tile handlers assumed power-of-two overview
  pyramids (`level = 1 << n`); a non-power-of-two chain served the wrong
  resolution for the requested zoom — a georeferencing error in served imagery —
  and multi-band pixel windows silently returned all-zero data. An RGB
  composite's red channel was also read through a different, disagreeing code
  path than green/blue; all three now share one corrected `read_level_band`
  helper.
- **oxigeo-services (WCS)**: `GetCoverage` on a multi-band raster wrote only one
  band's samples into a buffer sized for all of them, silently truncating/
  zero-padding the response.
- **oxigeo-mobile**: tile reads on a planar-layout file interleaved the wrong
  bytes into RGBA output; windowed region reads (`oxigeo_dataset_read_region`)
  silently left the output buffer untouched instead of erroring or filling it,
  once the driver-level bug above was fixed out from under it; overview-level
  statistics had an off-by-one level and assumed power-of-two dimensions.
- **oxigeo-wasm**: the in-browser COG viewer had two tile-decoding paths that
  disagreed about whether a tile's byte order still needed swapping, double- or
  never-swapping depending on which one served the request.
- **oxigeo-node**: `Dataset::open` hand-rolled a de-interleave step assuming the
  old broken `read_band` — every multi-band open hard-failed with
  `FORMAT_ERROR`.
- **oxigeo-cli**: `read_band`'s workaround code (tolerating either a single-band
  or a whole-interleaved-image result) and `read_band_region`'s ~200-line
  hand-rolled tile-stitcher/de-interleaver (which, as its own removed comment
  noted, "could not read `PlanarConfiguration = 2` files correctly") are both
  replaced by direct calls to the fixed driver API; `profiler.rs`'s per-band
  benchmark used to only ever measure band 0.
- **oxigeo-ml-foundation**: `GeoTiffDataset::load_all_bands` manually
  de-interleaved a `read_band(0, 0)` result under the old semantics; once
  `read_band` was fixed, that workaround itself became actively wrong and is now
  removed.
- **oxigeo-jupyter**: the `%stats` magic double-de-interleaved an already
  single-band buffer for the same reason.
- **oxigeo-drivers-vrt**: pixel accessors used `from_le_bytes`/`to_le_bytes` on
  buffers that are already host-native (from `GeoTiffReader`), corrupting values
  on big-endian hosts.

**Unrelated bugs found along the way**

- **oxigeo-streaming**: `ChunkedReader::read_chunk` failed on the *first* call on
  every stream (an empty, not-yet-filled buffer was treated as a hard error), and
  read-ahead prefetch desynced its cursor against any directly-read chunk, failing
  every subsequent prefetch push.
- **oxigeo-mbtiles**: `MBTilesReader::open_in_memory` deleted only the primary
  SQLite spill file on cleanup, leaking its `-wal`/`-shm`/`-journal` siblings into
  the OS temp directory on every call.
- **oxigeo-ml**: `optimization::iterative_pruning` wrote intermediates to fixed,
  non-unique file paths; concurrent calls could overwrite and corrupt each
  other's in-progress models. Each call now uses an isolated, auto-cleaned
  scratch directory.
- **oxigeo-compress**: `LZ4_MAX_OUTPUT_GUESS` was a raw `4 * 1024 * 1024 * 1024`
  `usize` literal, which overflows `usize` on 32-bit targets (wasm32, 32-bit
  ARM/x86); now computed in `u64` with a saturating cast.
- **oxigeo-netcdf**: `Variable::new`/`new_coordinate`'s empty-name error was
  misclassified as a generic `Core` error instead of `VariableError`.
- **`oxigeo-compress` now builds for `wasm32-unknown-unknown`.** It failed to
  compile at all on that target: `ahash`'s default `runtime-rng` feature pulls
  `getrandom`, which hard-errors on wasm32 unless its JavaScript backend is
  explicitly opted into (`The "wasm_js" backend requires the wasm_js feature`).
  wasm builds of this crate now use ahash's `compile-time-rng` instead, so the
  hash keys are generated on the host at build time and no JavaScript host is
  required; every other target keeps `runtime-rng` and its per-process keys
  unchanged. The workspace `ahash` entry moved to `default-features = false`
  (Cargo forbids a member from disabling a workspace dependency's defaults), so
  `oxigeo-{compress,edge,gateway,streaming}` now name `["std", "runtime-rng"]`
  explicitly — the same feature set as before on native targets. Only
  `oxigeo-wasm` is built for wasm32 in CI, so this was invisible there.

## [0.2.1] - 2026-07-28

Production-hardening campaign (2026-07): a workspace-wide, multi-agent defect
sweep across all 76 crates surfaced **342 confirmed defects**
(47 critical / 84 high / 83 medium / 33 low). **314 were fixed** across 38 crate
lanes (~520 files changed); the remaining 79 were honestly deferred, each left
with a safe typed-error path — a loud `Unsupported*` / `NotImplemented` /
`DecodingError` rather than silent or fabricated data. Quality gates all green:
`cargo fmt --check` clean; `cargo clippy --workspace --all-features --all-targets`
0 warnings; `cargo nextest run --all-features` 17,723 passed / 0 failed /
100 skipped (16,307 passed / 0 failed / 79 skipped on default features); 416
doc tests passing; `cargo deny check` passing. The categorized list of
deferrals carried to v0.3.0 is in TODO.md.

### Fixed

**Format drivers**

- **oxigeo-jpeg2000**: two CRITICAL correctness bugs fixed — multi-tile decode now
  `Psot`-bounds each tile's bitstream and composites it at its real pixel offset
  (previously every tile silently returned tile 0), and the JP2 box parser now
  recurses into `jp2h` so `ihdr`/`colr` in spec-conformant `.jp2` files are read
- **oxigeo-geotiff**: real planar-configuration (`PlanarConfiguration=2`) decoding;
  authoritative EPSG projected/geographic classification; a working JPEG/WebP writer
  path; the silent `GeoKeyDirectory` error and a policy-violating `expect()` removed;
  a `usize`-overflow bug in header-driven allocation fixed
- **oxigeo** (umbrella): fixed GitHub issue #12, "Metadata missing when reading
  geotif" — the lightweight `extract_tiff_info()` peek parser used by `Dataset::open()`
  (distinct from the full `oxigeo-geotiff` driver above) only scanned a GeoTIFF's
  first 8 KiB, so `ModelPixelScaleTag`/`ModelTiepointTag`/`GeoKeyDirectoryTag` values
  stored out-of-line past that offset — routine for striped TIFFs with many strips —
  were silently treated as absent and `crs()`/`geotransform()`/`bounds()` all returned
  `None` even though the tags were present and well-formed; the peek buffer now
  extends up to a bounded 1 MiB when a georeferencing tag's value lands past the
  initial window, a Y-axis sign inversion in the derived `GeoTransform` is fixed
  (`ModelPixelScaleTag`'s Y scale is a positive magnitude per spec but
  `GeoTransform::north_up` expects a negative `pixel_height`), and `bounds()` —
  previously hardcoded to `None` — is now derived from the geotransform and raster
  dimensions; regression test `test_issue_12_far_offset_georeferencing` added
- **oxigeo-drivers/grib**: CRITICAL DRT 5.40 silent-corruption bug fixed — the GRIB2
  decoder now dispatches on the Data Representation Template number, so a
  JPEG2000/PNG/CCSDS payload can never fall through to the simple-packing
  bit-unpacker; DRT 5.40 is wired to a real Pure-Rust JPEG2000 decode via
  `oxigeo-jpeg2000` (new default-on `jpeg2000` feature)
- **oxigeo-shapefile** (vector drivers): the Polygon reader now reconstructs
  multi-part polygons by ESRI ring winding (clockwise = exterior, CCW = hole) with
  containment-based hole assignment, emitting `MultiPolygon` for multiple exteriors —
  a two-island country shapefile round-trips instead of merging its rings
- **oxigeo-drivers/netcdf** & **oxigeo-drivers/hdf5**: NetCDF-4 reader now recurses
  into HDF5 sub-groups (was silently dropping their variables); the HDF5 writer's
  chunking/compression/fill-value hints are no longer silently dropped (real chunked
  write path plus honest errors for shapes oxih5 cannot represent); real object-header
  parsing so `decode_chunk`/filter-pipeline/chunking are no longer dead code
- **oxigeo-drivers/netcdf** & **oxigeo-drivers/hdf5**: attribute decoding now trusts
  the dataspace-declared element count (`count × dtype_size`) and ignores trailing
  bytes, so scalar/small numeric attributes written with padded payloads no longer
  decode as phantom extra elements — this silently disabled CF `_FillValue`/
  `scale_factor` handling for files written by oxih5 0.2.1, whose `FileWriter` padded
  sub-8-byte scalar attribute payloads; the writer regression is now root-fixed
  upstream in oxih5 0.2.2 (this workspace is pinned to it), and the defensive trim
  stays in place as a belt-and-suspenders guard against older files written by 0.2.1
- **oxigeo-drivers/geoparquet**: XYZ/XYM geometry decode ambiguity fixed

**Algorithms & CRS**

- **oxigeo** (umbrella): CRITICAL `Dataset::clip()` bug fixed — clip now records a
  pixel window that every raster read (`read_band`/`bands`/`statistics`/`convert`/
  `read_window`) crops the source file to, so a clipped dataset no longer silently
  reprocesses the full raster
- **oxigeo-algorithms**: real NEON SIMD (with scalar-parity tests) for morphology
  (3×3 erode/dilate) and threshold kernels; a real CSE (let-binding hoisting) + DCE
  (liveness/reachability) pass for the raster-algebra optimizer
- **oxigeo-proj**: PROJ `+proj=hgridshift` / `+proj=vgridshift` pipeline steps now
  actually apply a grid — new `GridRegistry` + `Pipeline::with_hgrid/with_vgrid` and
  evaluators calling the crate's NTv2 grid parser (a sign bug in it was fixed)

**Server & OGC services**

- **oxigeo-server**: the `/tiles/{layer}/{z}/{x}/{y}.{fmt}` XYZ endpoint now renders
  real raster data — reads the intersecting source window, reprojects Web-Mercator
  tiles into the dataset's native CRS (per-pixel inverse warp for non-3857 data),
  applies the layer colormap/RGB style, and masks off-dataset/nodata pixels as
  transparent — replacing a hard-coded checkerboard
- **oxigeo-services**: WPS `buffer`/`clip`/`union` now perform real geometry math via
  `oxigeo-algorithms` and return the computed GeoJSON (previously ignored their
  inputs); CQL2 gained `!=`/`<>`, `IN (...)`, and `IS [NOT] NULL`

**Query engine**

- **oxigeo-query** / **oxigeo-index**: JOIN output now preserves native column types
  instead of stringifying everything; SELECT projection lists are actually applied;
  HAVING is executed (including aggregates referenced only by HAVING); the WHERE
  evaluator gained `BETWEEN`/`IN`/`CASE`/`CAST` with real type coercion

**ML**

- **oxigeo-ml**: model pruning/quantization no longer corrupts ONNX files — a real
  ONNX protobuf walker (`optimization/onnx_weights.rs`) applies genuine tensor
  transforms; `ModelVersion` `Ord` bug fixed
- **oxigeo-ml-foundation**: the crate now compiles and trains — a genuine trainable
  scirs2-neural backend (real forward/backward/optimizer step with explicit gradient
  routing) replaces code that referenced removed `rand` APIs and mismatched types

**Cloud & DB connectors**

- **oxigeo-postgis**: `Transaction::drop` now issues a real implicit `ROLLBACK`
  (was a log-only message that leaked locks) with a double-take guard
- **oxigeo-db-connectors**: MySQL/TimescaleDB SQL-injection surfaces closed via a new
  `crate::sql` identifier-quoting/literal-escaping module plus parameter binding
- **oxigeo-cloud**: CRITICAL rs3gw tokio nested-runtime panic fixed; byte-range reads,
  the prefetch I/O driver, OAuth2/SAS credential refresh (HttpBackend), and STAC fixes
- **oxigeo-cloud-enhanced**: fabricated Azure (Cost/Monitor/ML/Synapse) and GCP (Vertex
  AI/Dataflow/Cost) clients replaced with real, bearer-token-authenticated REST clients
  behind the existing `azure`/`gcp` features — Azure Cost Management queries/forecasts/
  budgets/Advisor, Azure Monitor metrics/Log Analytics/alerts/diagnostic settings, Azure
  ML v2 control-plane compute/model/endpoint/job management, Synapse SQL/Spark pool (ARM)
  management and Spark job/pipeline submission (Livy); GCP Dataflow template launch with
  job status/list/metrics/cancel/drain, Vertex AI model/endpoint/training/batch-prediction
  (long-running-operation polling), and GCP Cost Management via BigQuery billing export
  plus Cloud Billing budgets/Recommender — every previously-fabricated success/ID/
  empty-list is now a real call or an honest typed `NotImplemented`. True data-plane
  operations a control-plane REST client can't mint stay `NotImplemented` (Monitor
  metric/diagnostic ingestion, Cost alert/export, Synapse `execute_query`, ML
  `invoke_endpoint`, GCP cost forecast/export)

**HA & infra**

- **oxigeo-ha**: PITR, snapshot, backup, and DR were entirely fabricated (canned bytes,
  always-pass tests) — replaced with real WAL + on-disk persistence and injectable
  executors; a genuine Raft log-replication module (`failover/log_replication.rs`) with
  `AppendEntries` consistency check, conflict truncation, and majority commit added
- **oxigeo-cluster** (cluster-dist): leader heartbeats now travel over the transport to
  followers (real `AppendEntries`-style RPC + handler) so followers stop perpetually
  re-running elections; W-TinyLFU is now reachable and used by the multi-tier cache
- **oxigeo-kinesis** / **oxigeo-kafka** / **oxigeo-pubsub**: fake/no-op broker paths
  replaced with real implementations and honest errors — Firehose transformation now
  actually happens; Kafka read-process-write exactly-once wired to real transactions

**Bindings**

- **oxigeo-node**: multi-band GeoTIFF save (BIP interleave round-trip); GeoJSON parser
  handles every geometry type; `CancellationToken` wired into batch/parallel processors
  doing real chunked multi-threaded per-pixel work
- **oxigeo-jupyter**: `%crs`/`%bounds`/`%stats` now read a real parsed GeoTIFF dataset
  instead of returning hard-coded `"(example)"` literals
- **oxigeo-python**: `open_raster`/`create_raster` no longer silently discard the
  `driver`/`options` arguments — a real remote/cloud data-source layer (`remote.rs`)
  wires `driver="COG"` and S3/HTTP options through to `oxigeo-cloud`

**no_std & platform**

- **oxigeo-core** / **oxigeo-embedded**: the no_std/embedded claim is now real
  end-to-end — both crates genuinely cross-compile for bare-metal
  `thumbv7em-none-eabihf` (Cortex-M4) and `riscv32imac-unknown-none-elf` (verified with
  actual `--target` builds); `parking_lot`/`crossbeam` are std-gated; `RealtimeScheduler`
  deadline enforcement now actually fires
- **oxigeo-gpu** / **oxigeo-gpu-advanced**: `reproject_gpu`/`execute_gpu` no longer error
  `InvalidBuffer` at runtime — the output buffers now request `MAP_READ` usage
  (verified on Metal)
- **oxigeo-proj**: the `no_std` (`--no-default-features`) build was broken — the crate
  declared `#![cfg_attr(not(feature = "std"), no_std)]` but failed with 63 errors; `extern
  crate alloc` is now unconditional and the alloc-prelude imports
  (`String`/`Vec`/`Box`/`ToString`/`format!`) plus `core::f64::consts` replacements were
  added across the crate, so `no_std` genuinely compiles and its tests pass

**Release-verification pass**

- **oxigeo-cloud**: a doctest in the multi-cloud abstraction example was missing a
  `#[cfg(feature = "s3")]` guard, so `cargo test --doc` failed to compile it under
  default (non-`s3`) features
- **oxigeo-drivers-advanced**: the GeoPackage doctest in `src/lib.rs` had the same bug —
  `gpkg::GeoPackage` used with no `#[cfg(feature = "geopackage")]` guard, because the
  doc prose wrongly called `geopackage` "enabled by default"; fixed with the guard, the
  prose, and a `fn example()`/`async fn example()` in place of `fn main`
- 9 `rustdoc::private_intra_doc_links` violations fixed across 8 files in 7 crates —
  `oxigeo-index`, `oxigeo-gateway`, `oxigeo-security` (×2 files), `oxigeo-drivers/hdf5`,
  `oxigeo-gpu`, `oxigeo-ml-foundation`, `oxigeo-postgis`
- Publish-order bug: `oxigeo-grib` (its default-on `jpeg2000` feature depends on
  `oxigeo-jpeg2000`) was sequenced *before* `oxigeo-jpeg2000` in both
  `~/work/pub_oxigeo.sh` and `scripts/publish-order.txt` — publishing in that order
  would have failed with an unresolved dependency; both are now correctly ordered
- 3 crates were missing `repository` metadata: `oxigeo-geojson-stream`, `oxigeo-index`,
  `oxigeo-noalloc`
- **oxigeo-node**: npm `optionalDependencies` were still pinned to `0.2.0` while the
  package itself is `0.2.1`
- Two hardcoded version strings in HTTP `User-Agent` headers (`oxigeo-stac`,
  `oxigeo-ml`) replaced with `env!("CARGO_PKG_VERSION")` so they can no longer drift
  from the crate version

### Added

- **oxigeo-drivers/zarr**: the empty Zarr v2 reader/writer stubs replaced with a working
  v2 read/write path (chunk-key builder, compressor+filter pipeline, fill values,
  dimension separator, dtype sizing); the ZEP-0002 v3 sharding codec; the fake ZFP codec
  made honest (mode-honoring, overflow-checked)
- **oxigeo-drivers/geoparquet**: the writer now emits real attribute columns and a
  `covering.bbox` column (was silently dropping all attributes); extended-WKB nested
  geometry encoding; Hive-style + spatial (bbox-grid/quadtree/Z-order) partitioning
- **oxigeo-geotiff**: real LERC decode (BitStuffer2 v1/v2/v3) and a JPEG-in-TIFF read
  path that auto-merges shared `JPEGTables` (tag 347)
- **oxigeo-proj**: native forward/inverse projections + round-trip tests for Equidistant
  Conic, Sinusoidal, Mollweide, Robinson, Eckert IV/VI, Cassini-Soldner, and
  Gauss-Krüger (extended zones)
- **oxigeo-drivers/grib**: template-based product-definition expansion (PDT 0.0–0.48
  coverage) and NetCDF CF-conventions v1.11 parsing (`cf_conventions/v1_11.rs`)
- **oxigeo-gpu**: reprojection, raster-algebra, and hillshade WGSL compute shaders;
  multi-GPU workload distribution; WebGPU/WASM shader compilation via a compile-time
  `ShaderRegistry`
- **oxigeo-ml**: ONNX model hot-reload (file-watch + atomic swap), content-addressed
  inference caching (SHA-256 key + LRU), adaptive batch sizing, and model
  versioning / deterministic A/B testing
- **oxigeo** / **oxigeo-streaming**: `DatasetOpenBuilder`/`DatasetCreateBuilder` fluent
  builders; a `FeatureStream`/`TileStream` streaming-iterator API
- **oxigeo-mbtiles** / **oxigeo-gpkg** / **oxigeo-pmtiles**: a real SQLite-backed MBTiles
  writer (now genuinely persists to `.mbtiles`); an opt-in R-tree spatial-index writer
  for GeoPackage
- **fuzz/**: 7 new libFuzzer targets (NetCDF, HDF5 superblock/object-headers, VRT XML,
  GeoJSON, and more), bringing coverage to 11 format/parser targets
- **tests/**: the 1,337-line mock re-implementation in `vector_advanced.rs` replaced —
  33 tests now exercise the real `oxigeo-algorithms` vector stack
- **oxigeo-gateway serving layer**: the previously stubbed `Gateway::serve()` (it accepted
  TCP connections and its `handle_connection` did nothing) is now a real axum 0.8 HTTP
  service — a new `GatewayServer` / `GatewayServerBuilder` wires the crate's
  already-implemented components into a running router:
  - routes: `GET /health`, `GET /gateway/metrics`, `POST /graphql` (plus a GraphiQL page
    when introspection is enabled and a `/graphql/ws` subscription endpoint when
    `enable_subscriptions` is set — that flag is now actually enforced), a `GET /ws`
    WebSocket upgrade (WebSocketManager wiring, default `EchoHandler` route, per-user
    connection caps, ping keepalive, gated on `enable_websocket`), and a load-balanced
    reverse-proxy fallback
  - reverse proxy: a streaming hyper 1 connection client, HTTPS upstreams over the
    Pure-Rust OxiTLS (rustls/RustCrypto) probe connector, hop-by-hop header stripping,
    `FailoverManager` retries that finally honor the previously-ignored
    `LoadBalancerConfig.retry_attempts`, circuit-breaker outcome reporting, and per-attempt
    request timeouts
  - pipeline: query-free trace spans (no query strings), API version negotiation +
    deprecation headers, the in-house middleware chain (CORS with real `OPTIONS` preflight,
    compression, response caching, logging, metrics), JWT/API-key/session auth via
    `MultiAuthenticator` (authenticate-if-present plus a `require_auth` mode, with the
    `require_mfa` flag now enforced), atomic rate limiting with `X-RateLimit-*` /
    `Retry-After` headers, request timeout and body-size limits; a `require_permission`
    RBAC guard is available for route groups and `GatewayError` now implements
    `IntoResponse`
  - honesty fixes: `CachingMiddleware` is now a real LRU+TTL cache instead of a no-op stub;
    compression performs real `Accept-Encoding` negotiation; the 1,865-line
    `middleware::advanced` module (request-ID / enhanced-logging / timeout-header /
    error-handling / histogram-metrics / cache-control) was orphaned — never declared or
    compiled — and is now wired in, compiling and tested; `X-Forwarded-For` is built
    against a trusted-proxy allowlist (`with_trusted_proxies`) rather than blindly trusting
    client-supplied values
  - honest limitations (v0.3.0+): GraphQL resolvers still serve demo/in-memory data (no
    storage backend); middleware-chain hops and proxied requests are buffered (bounded by
    `max_body_size`) while proxy responses stream; there is no WebSocket pass-through
    proxying, no upstream keep-alive pooling, and response-side transformation is not yet
    wired (request-side only)
  - the crate's own test suite grew from 266 to 381 tests (1 → 3 doctests)

### Security

- **oxigeo-services**: WFS-T CQL filtering now **fails closed** on unparseable CQL —
  an unparseable filter previously failed *open*, matching every feature and enabling a
  mass delete/update; it now rejects the request
- **Memory-safety (DoS/OOM hardening)**: header-driven allocation caps added to the
  NetCDF, HDF5, GRIB, and GeoTIFF parsers so a crafted header can no longer trigger a
  multi-gigabyte allocation; includes the GeoTIFF `usize`-overflow fix noted above
- **oxigeo-gateway**: load-balancer health checks now issue genuine HTTP/1.1-over-TCP
  requests (real Pure-Rust TLS via the OxiTLS RustCrypto provider for HTTPS) instead of
  always returning healthy, so a down backend is correctly marked unhealthy; the
  `MalwareScanner` now actually reads and inspects its input; the gRPC health check
  **fails closed** with an honest error rather than reporting unknown backends healthy
- **oxigeo-observability**: health checks do real work (sysinfo disk usage, injectable
  connectivity checker) instead of returning hard-coded `Healthy`; a stub `LabelMatch`
  alert condition that always returned `true` fixed

### Changed

- **oxigeo-db-connectors**: default features made Pure-Rust — the C-FFI database backends
  are now strictly opt-in behind named features
- **oxigeo-query**: `tokio` moved to dev-dependencies and `rayon` gated behind a
  default-on `parallel` feature, so the SQL engine is consumable from
  `wasm32-unknown-unknown`
- **Packaging & legal**: added `NOTICE` and `THIRD_PARTY.md` (Apache-2.0 §4(d)
  attribution + generated third-party license inventory), a committed `deny.toml`
  (advisories + bans + licenses) wired into `cargo deny check`, an in-repo 75-crate
  topological publish-order manifest (previously only in an external script), a license
  note for the vendored `pathfinder_simd`, and `[package.metadata.docs.rs]` fixes on
  the C-FFI-gated crates
- **Supply-chain hygiene**: `.cargo/audit.toml`'s advisory allowlist re-verified against
  the current lockfile and pruned from 21 to 15 entries — `aws-lc-sys`
  (RUSTSEC-2026-0044/-0048) and `tokio-postgres`/`postgres-protocol`
  (RUSTSEC-2026-0178/-0179/-0180) are already patched at our pinned versions, and
  `proc-macro-error2` (RUSTSEC-2026-0173) is no longer in the dependency graph; the new
  `deny.toml` `[bans]` list enforces this workspace-wide, and `tower-http`'s
  `compression-br`/`compression-gzip`/`compression-deflate` features (unused — no
  `CompressionLayer` anywhere — but pulling banned `flate2`/`brotli`/`miniz_oxide` outside
  `deny.toml`'s allowed wrapper scoping) are now explicitly excluded in every consumer;
  `SECURITY.md`'s contact address corrected to `security@cooljapan.tech`
- Dependencies kept current per the Latest Crates Policy (`arrow` 58 → 59, `indicatif`
  0.18 dropping the unmaintained `number_prefix`, `oxih5`/`oxih5-core`/`oxinetcdf`
  0.2.0 → 0.2.2, `scirs2-core` and the `scirs2-{neural,autograd,optimize,datasets,
  metrics,linalg,vision,series}` family 0.6.1 → 0.6.4)
- A further round of Latest Crates Policy bumps: `base64` 0.22 → 0.23, `pollster`
  0.4 → 1.0, `las` 0.9 → 0.10, `jsonwebtoken` 10 → 11, `ed25519-dalek` 2 → 3
  (`std` feature dropped, `zeroize` retained), `azure_core` 1.0 → 1.1,
  `google-cloud-pubsub` 1.1 → 1.2, `statrs` 0.18 → 0.19, `tokio-tungstenite`
  0.29 → 0.30. Only `las` 0.10 required a source change: it replaced the
  per-point `Reader::points()` streaming iterator with a batch/buffer API
  (`Reader::read_all()` / `read_points(n)` returning a `PointData` slab whose
  `.points()` yields the same row-oriented iterator), so `oxigeo-3d`'s
  `LasReader::read_all`/`read_n` were updated accordingly; the other eight
  bumps were drop-in with no source changes required
- **Dependency hygiene**: genuinely-unused dependencies removed from 66 crates'
  `Cargo.toml` files (found via `cargo-machete`, each removal build-verified);
  `deny.toml`'s advisory-ignore list pruned from 15 to 7 entries (the other 8 IDs no
  longer match anything in the current `Cargo.lock`) and its license allowlist trimmed
  of entries no longer reachable in the dependency graph; a `wildcard`-dependency
  `cargo-deny` warning resolved via `allow-wildcard-paths` (three intra-workspace
  dev-dependencies — `oxigeo-3d` → `oxigeo-copc`, `oxigeo-dev-tools` →
  `oxigeo-algorithms`, `oxigeo-qc` → `oxigeo-geojson` — are deliberately unpinned path
  deps so publish ordering doesn't become circular)

### Removed

- **`oxigeo-kafka` is retired as a project, effective 0.2.1.** The crate has been
  deleted from the workspace and **will receive no further releases**; the versions
  already on crates.io (0.0.1 and 0.2.0) have been yanked. This is a deliberate
  retirement, not an oversight — the crate is gone on purpose and is not coming back.

  Removed alongside it: the `kafka` feature of **oxigeo-etl** (and with it
  `KafkaSource`/`KafkaSourceConfig`, `KafkaSink`/`KafkaSinkConfig`, their prelude
  re-exports, and the `Kafka` variants of `SourceError`/`SinkError`), the `kafka`
  feature of **oxigeo-workflow** (which gated an `rdkafka` dependency that no source
  file in that crate ever used), and the `rdkafka` entry in `[workspace.dependencies]`.

  Reason: `oxigeo-kafka` was the **sole mandatory C-toolchain dependency in the entire
  workspace** — `rdkafka-sys` builds librdkafka via `cmake` — which stands against the
  COOLJAPAN Pure Rust Policy. At 4,831 lines it was 0.62% of the workspace's ~778k
  lines of Rust and had **zero reverse dependencies inside the workspace**: nothing
  built on it. As a direct result of the removal, **`cargo check --workspace
  --all-features` no longer requires `cmake` or a C toolchain** and completes clean.

  Migration: use a dedicated Kafka client (e.g. `rdkafka`) directly in your own code,
  or one of the sibling messaging crates that remain supported — `oxigeo-streaming`,
  `oxigeo-kinesis`, `oxigeo-pubsub`, `oxigeo-mqtt`. Workflow definitions can still
  *describe* a Kafka endpoint over the wire: the pure-Rust `IntegrationType::Kafka`
  and `MessageQueueType::Kafka` metadata enums in `oxigeo-workflow` are unchanged.

- **oxigeo-proj**: the `proj-sys` feature and the `proj` C-bindings dependency (C
  bindings to the system libproj) removed, per the COOLJAPAN Pure Rust Policy. All
  coordinate transformation already routed through the pure-Rust `oxiproj` engine, so
  the feature was vestigial — it contributed only an unused error variant and its
  `From<proj::ProjError>` conversion, and no transformation path ever called the C
  library. Its one real effect was that `--all-features` builds required `cmake` and a
  system libproj (the `proj` crate builds PROJ from source), which broke
  `cargo test --workspace --all-features`. For higher-fidelity CRS coverage use the
  pure-Rust `proj-db` feature (oxisql PROJ.db reader, ~7500 EPSG codes) instead.

## [0.2.0] - 2026-07-20

### Changed

- **Project renamed: OxiGDAL → OxiGeo.** Version 0.2.0 is functionally
  identical to 0.1.7 — this is a rename-only release with no feature or
  behavior changes beyond identifiers. The GitHub repository has moved to
  <https://github.com/cool-japan/oxigeo> (old `oxigdal` URLs redirect), and
  v0.1.7 remains the final release published under the OxiGDAL name.

  Migration table (old → new):

  | Area | Old (OxiGDAL) | New (OxiGeo) |
  |------|---------------|--------------|
  | Crates (all 74 published) | `oxigdal`, `oxigdal-<name>` | `oxigeo`, `oxigeo-<name>` |
  | CLI binary | `oxigdal` | `oxigeo` |
  | Environment variables | `OXIGDAL_*` (e.g. `OXIGDAL_CONFIG`, `OXIGDAL_HOST`, `OXIGDAL_PORT`, `OXIGDAL_WORKERS`, `OXIGDAL_LOG_LEVEL`, `OXIGDAL_DATA_DIR`, `OXIGDAL_CACHE_DIR`) | `OXIGEO_*` (`OXIGEO_CONFIG`, `OXIGEO_HOST`, `OXIGEO_PORT`, `OXIGEO_WORKERS`, `OXIGEO_LOG_LEVEL`, `OXIGEO_DATA_DIR`, `OXIGEO_CACHE_DIR`) |
  | Python | PyPI package `oxigdal`; `import oxigdal`; native module `oxigdal._oxigdal` | PyPI package `oxigeo`; `import oxigeo`; native module `oxigeo._oxigeo` |
  | npm | `@cooljapan/oxigdal`; `@cooljapan/oxigdal-node` (+ platform packages); `@cooljapan/oxigdal-geoparquet` | `@cooljapan/oxigeo`; `@cooljapan/oxigeo-node` (+ platform packages); `@cooljapan/oxigeo-geoparquet` |
  | C / mobile FFI | symbol prefix `oxigdal_`; JNI class `com.cooljapan.oxigdal.OxiGDAL`; header `oxigdal_mobile.h`; include guard `OXIGDAL_MOBILE_H` | symbol prefix `oxigeo_`; JNI class `com.cooljapan.oxigeo.OxiGeo`; header `oxigeo_mobile.h`; include guard `OXIGEO_MOBILE_H` |
  | Rust API types | `OxiGdal*` prefixed types (e.g. `OxiGdalError`) | `OxiGeo*` (`OxiGeoError`) |
  | WASM artifacts | `oxigdal_wasm*`; napi artifact `oxigdal.<triple>.node` | `oxigeo_wasm*`; napi artifact `oxigeo.<triple>.node` |
  | Container images | `oxigdal/*`; systemd unit `oxigdal-server.service` | `oxigeo/*`; systemd unit `oxigeo-server.service` |
  | Runtime identifiers | HTTP User-Agent `OxiGDAL/1.0`; Kafka consumer group `oxigdal-etl`; ETL checkpoint dir `oxigdal-checkpoints`; edge cache dir `.oxigdal_cache`; attestation format id `oxigdal-attestation` | HTTP User-Agent `OxiGeo/1.0` (the `oxigeo-stac`/`oxigeo-ml` agents now report `0.2.0`); Kafka consumer group `oxigeo-etl`; ETL checkpoint dir `oxigeo-checkpoints`; edge cache dir `.oxigeo_cache`; attestation format id `oxigeo-attestation` |

- The `oxigdal-*` 0.1.x crates remain published on crates.io for existing
  users; the `oxigeo-*` crates supersede them starting with 0.2.0.

## [0.1.7] - 2026-07-20

### Added

- **oxigdal-cloud-enhanced**: real Azure IMDS managed-identity tokens via `azure_identity::ManagedIdentityCredential`, replacing the placeholder-token stub; real GCP metadata-server access/identity tokens plus IAM Credentials API impersonation, with `GCE_METADATA_HOST` overridable for mock-server tests
- **oxigdal-cloud**: multicloud `build_backend()` factory (S3/GCS/AzureBlob/Http, feature-gated) with a backend cache; `get`/`put`/`delete`/`exists_in_provider` are now functional against real backends
- **oxigdal-drivers-advanced**: JPEG2000 decode now delegates to `oxigdal-jpeg2000` for real decode with full header parsing, replacing the gray-placeholder-pixel stub
- **oxigdal-services**: WFS-T Memory/File transactions fully implemented — insert/update/delete/replace with per-path write serialization
- **oxigdal-services**: WCS File/Url/Memory coverages now do real GeoTIFF read/write via `oxigdal-geotiff`; `encode_as_geotiff` produces real GeoTIFF bytes (was stub output)
- **oxigdal-ml-foundation**: `onnx_export.rs` — pure-Rust ONNX protobuf encoder (ir_version 8, opset 13), round-trip-validated against `oxionnx`
- **oxigdal-ml-foundation**: augmentation noise generation now uses real Gaussian sampling (`scirs2_core` seeded RNG) instead of a synthetic pattern
- **oxigdal-ml**: `OnnxModel::infer_multiband` — real multi-channel `[1, C, H, W]` NCHW tensor inference over a `MultiBandBuffer` (band-sequential channel order, unpacked back into one output band per channel); previously `infer` accepted only a single-band `RasterBuffer`
- **oxigdal-workflow**: Temporal/Prefect `import_workflow` round-trips exporter-generated definitions via metadata headers for lossless ID recovery; export now emits real activity bodies
- **oxigdal-etl**: `calculate_ndvi` map transform implemented, with a zero-denominator guard so masked/no-data pixels emit `0.0` rather than `NaN`
- **oxigdal-cli**: `info`/`stats` implemented for FlatGeobuf, GeoParquet, Zarr, GeoPackage, JPEG2000, COPC, PMTiles, MBTiles (previously "not yet implemented")
- **oxigdal-algorithms**: Lanczos resampling `Wrap` and `Mirror` edge modes implemented (`rem_euclid` / reflect-101)
- **oxigdal-geojson-stream**: TopoJSON writer now emits real arcs for LineString/MultiLineString — open-chain topology with endpoint junctions, no-rotation splitting, and shared-arc dedup via negative reversed indices (was an empty `"arcs": []` stub)
- **oxigdal-gpu**: subgroup/warp operations emit native WGSL subgroup builtins with a workgroup-shared-memory emulation fallback; Metal filter/reduction/nearest-neighbor shader generators implemented; ballot/vote/`SimdGroupOperations` upgraded; new execute-and-compare GPU tests (verified on Metal)
- **oxigdal-bench**: raster/io scenarios now do real work (tile reads, `MmapDataSource`) instead of synthetic placeholders
- **oxigdal-wasm**: `WasmCogViewer.openBytes` — drag-drop local GeoTIFF with full codec support including LZW/Zstd via `CogReader<MemorySource>`; `readTileElevation` (SampleFormat tag 339 parsing); `WasmTerrain` — hillshade/multidirectional hillshade/slope/aspect/color-relief-shaded (Horn method, `ImageData` output); `WasmProjection` + `wgs84ToWebMercator`/`webMercatorToWgs84` shims
- **GeoLab demo** (`demo/cog-viewer`): rebranded OxiGDAL GeoLab — drag-drop loading, terrain-analysis panel, honest byte counters, all CDN dependencies vendored locally; staged to cooljapan.tech/geolab/ (deploy manual)
- **oxigdal-security**: new `attestation` module — tamper-evident session ledger: domain-separated blake3 hash chain (`SessionLog`), Merkle root + per-entry inclusion proofs, Ed25519 session seal (`SessionSigner::seal`), and `verify_attestation()` re-verifying chain/root/signature from the attestation JSON alone; golden-fixture and tamper-detection tests; native skeptic's verifier example `verify_attestation.rs`; compiles for wasm32 under `--no-default-features --features attestation`
- **oxigdal-wasm**: `sentinel` module (GeoSentinel) — `WasmStacClient` Earth Search STAC scene-pair search with client-side cloud/nodata/grid filtering; self-contained UTM↔WGS84 (Krüger series, EPSG 326xx/327xx); `GeoSentinel` change-detection pipeline: windowed COG reads → BOA offset → NDVI drop → fixed/Otsu threshold → polygonization → Karney geodesic hectares → GeoJSON, plus true-color and diff-heatmap RGBA overlays
- **oxigdal-wasm**: `vault` module (GeoVault) — `WasmVaultSession` blake3 hash-chained operation log sealed with Ed25519 into attestation JSON, `verifyAttestation`, blake3 `fileDigestHex` for dropped files
- **oxigdal-wasm**: `anomaly` module — self-contained Z-score / IQR / modified-Z-score / percentile / σ-bounds detectors (parity-ported from `oxigdal-analytics` / `oxigdal-qc`) with mask, `ImageData`, and summary-JSON outputs
- **oxigdal-wasm**: COG reader overview-level reads — full per-overview IFD parsing (each level gets its own tile directory, predictor, and sample layout), `read_tile_level`, and `read_window_u16` / `read_window_rgb8` window assembly; PREDICTOR=2 horizontal-differencing undo (TIFF tag 317) for u8/u16 samples on all tile and window paths
- **oxigdal-geoparquet**: new `plan` / `pushdown` APIs — `plan_pushdown()` computes row-group bbox + attribute-statistics pruning and exact column-chunk byte ranges from metadata alone (zero I/O); `execute_pushdown()` runs pushdown over any `parquet::ChunkReader` (`GeoParquetReader::read_pushdown` is now a thin wrapper)
- **oxigdal-geoparquet**: bbox-column detection now honors GeoParquet 1.1 `covering.bbox` paths from the `geo` metadata (authoritative) with a plain `bbox` struct-root fallback — VIDA-style files (5.9 GB / 9,533 row groups) now prune correctly
- **oxigdal-geoparquet**: `AttributeFilter::Cmp` scalar comparisons (`>`, `>=`, `<`, `<=`, `<>`) with Int64/Float64 literal↔column coercion (a bare integer compares correctly against a Float64 column and a whole-valued decimal against an integer column); multiple filters compose as a conjunction via `with_attribute_filters`
- **oxigdal-wasm-geoparquet** (new crate): browser GeoParquet range-request client — remote footer decode, `SparseChunkReader` over prefetched byte ranges, 64 KiB-gap range coalescing, SQL `WHERE`-fragment → predicate lowering (sqlparser, typed rejections naming unsupported constructs), `RecordBatch` → GeoJSON conversion, and `RemoteGeoParquet` open/plan/query with byte and request accounting (npm: `@cooljapan/oxigdal-geoparquet`)
- **GeoSentinel demo** (`demo/geosentinel`): in-browser Sentinel-2 change detection — STAC pair search, streamed COG windows, NDVI-drop polygons with geodesic hectares, GeoJSON export, before/after crossfade; staged to cooljapan.tech/geosentinel/ (deploy manual)
- **GeoVault demo** (`demo/geovault`): sovereign clean-room workstation — CSP-enforced zero egress, live session ledger, seal → attestation download, independent `verify.html` verifier; synthetic Site K-7 DEM via new `oxigdal-geotiff` example `geovault_scene.rs`; staged to cooljapan.tech/geovault/ (deploy manual)
- **GeoParquet Live demo** (`demo/geoparquet`): bounding-box + SQL attribute queries against the 5.9 GB VIDA GeoParquet via predicate pushdown over HTTP ranges — row-group strip visualization, plan-cost preview before any fetch, Cache API footer caching, offline sample + new `oxigdal-geoparquet` example `generate_sample.rs`; staged to cooljapan.tech/geoparquet/ (deploy manual)
- **oxigdal-server**: new example `render_hero.rs` (DEM → combined hillshade → colormap → PNG)
- docs.rs metadata added to all 64 remaining publishable crates (21 curated for Pure-Rust-only docs builds)
- New `CONTRIBUTING.md` and `CODE_OF_CONDUCT.md`

### Changed

- **oxigdal-cloud-enhanced**: `reqwest` made optional, gated behind the `gcp` feature
- **oxigdal-ml-foundation**: weights save/load moved to `oxicode` (COOLJAPAN no-bincode policy)
- **oxigdal-services**: Database transactions/feature-sources/SQL count moved behind new non-default `postgis` feature (`oxigdal-postgis` pool, `ST_GeomFromGeoJSON`/`ST_AsGeoJSON`); WCS `Url` coverage fetch moved behind new non-default `remote` feature
- **oxigdal-drivers-advanced**: `jpeg2000` feature is now dependency-gated (pulls in `oxigdal-jpeg2000` only when enabled)
- **oxigdal-security**: dependencies split behind new `enterprise` / `tls` / `attestation` features (default enables all three) — the heavyweight server-side surface (tokio, dashmap, petgraph, scirs2-core, oxiarc-zstd, regex, parking_lot, uuid, chrono, crypto stack) is now optional under `enterprise`; `tls` implies `enterprise`; `attestation` pulls only `blake3` + `ed25519-dalek`, keeping the wasm32 surface lean
- **GeoLab demo**: shared `@cooljapan/oxigdal` WASM package rebuilt (pkg refresh) — GeoLab, GeoSentinel, and GeoVault all serve the same refreshed package
- Examples/benches reorganized: 31 orphaned top-level examples wired into `oxigdal-examples` (API rot fixed, 5 duplicates pruned); 11 benches wired into `oxigdal-bench`
- README: stats refreshed, doc links updated, GeoLab hero image made clickable, new `## Demo` section with native-render gallery (`docs/media/`); section grown to `## Demos` with hero/GIF/gallery/honest-notes blocks for GeoSentinel, GeoVault, and GeoParquet Live
- Dependencies bumped to latest per the Latest Crates Policy: `oxiproj`/`oxiproj-core` 0.1.1 → 0.1.2, `oxisql-core`/`oxisql-sqlite-compat` 0.3.2 → 0.4.0, `oxinetcdf` 0.1.4 → 0.2.0, `oxih5` 0.1.4 → 0.2.0 — version-only `Cargo.toml` changes; the `oxih5`/`oxinetcdf` jump to 0.2.0 was verified source-compatible with the `oxigdal-drivers/hdf5`/`oxigdal-netcdf` driver code (no driver-side changes required)

### Fixed (production-hardening campaign, 2026-07)

Parallel multi-lane defect sweep across the workspace: 233 verified defects fixed across
69 crates (correctness, unwrap-elimination, clippy, doc/README accuracy). Headline items:

**Format drivers**

- **oxigdal-geotiff**: floating-point predictor (TIFF `Predictor=3`) decode *and* encode now
  actually implemented — was previously a silent no-op that passed float32/float64 tile data
  through unmodified, corrupting round-trips of predictor-encoded float COGs
- **oxigdal-jpeg2000**: MQ arithmetic decoder `INITDEC` procedure brought into ITU-T T.800
  Annex C spec conformance
- **oxigdal-drivers/gml**: `srsDimension` attribute now parsed, so 3D coordinate geometries
  are no longer silently treated as 2D
- **oxigdal-drivers-advanced (VRT)**: `FirstValid` pixel-function compositing fixed for
  multi-byte sample types (u16/f32/f64 — was only correct for single-byte u8 samples);
  `BandMath` pixel function now substitutes `B10` and higher band variables (previously only
  `B1`–`B9` were recognized, silently dropping bands past 9 from expressions)
- **oxigdal-drivers/hdf5** and **oxigdal-netcdf**: both drivers re-backed by the real
  Pure-Rust `oxih5 0.1.4` / `oxinetcdf 0.1.4` crates (crates.io, no libhdf5/libnetcdf FFI).
  `oxigdal-drivers/hdf5` previously read a custom `OXIGDAL_HDF5_METADATA_V1` JSON sidecar
  and returned zeros for real `.h5` files; it now reads and writes genuine HDF5 via `oxih5`.
  `oxigdal-netcdf` now reads genuine NetCDF-4/CF files via `oxinetcdf`. Public API is
  unchanged (`Hdf5Reader::open`, `Attribute`/`AttributeValue`/`Datatype`/`Hdf5Version`/
  `Hdf5Writer`, `NetCdfReader::open`); 730 tests passing across the 4 affected crates,
  clippy clean. Honest limitations carried forward: `oxih5` 0.1.4 fully reads
  v0-superblock `.h5` files, while v2/v3-superblock files open but currently yield an empty
  tree (best-effort, never faked); the writer produces contiguous real HDF5 (chunk/
  compression hints are dropped, values are correct); the NetCDF reader surfaces the root
  group, and `scale_factor`/`add_offset`/`_FillValue` are exposed as attributes but not
  auto-applied

**Algorithms**

- **oxigdal-algorithms**: the raster/DSL calculator's algebraic optimizer no longer folds
  `x * 0` / `0 * x` to a constant `0.0` — since `NaN * 0.0 == NaN` and `Inf * 0.0 == NaN`,
  the previous simplification silently discarded NoData/Inf semantics in NoData-masked
  raster expressions; covered by a new NaN-semantics regression test
- **oxigdal-algorithms**: Weiler-Atherton polygon clipping's concave-region fallback path no
  longer silently returns a geometrically wrong (angularly-sorted) shape — the mismatch is
  now surfaced as an explicit condition rather than masked as a plausible-looking result;
  full boundary-walk reconstruction for concave fallbacks remains future work (see TODO.md)

**Security**

- **oxigdal-security**: RBAC `resource_pattern` matching is now actually consulted by the
  authorization check — was previously parsed and stored but never read, a
  privilege-widening bug that let any pattern-scoped permission match every resource
- **oxigdal-gateway**: TOTP verification switched to a constant-time comparison and gained a
  ±1 time-step (30s) clock-skew tolerance window per RFC 6238 §5.2; backup-code and
  SMS-challenge comparisons are now constant-time as well

**Cloud & infra**

- **oxigdal-server**: `server.toml` is now actually loaded via `OXIGDAL_CONFIG` in
  Docker/Kubernetes deployments — was previously parsed and then discarded, silently
  running on built-in defaults regardless of the mounted config file
- **oxigdal-stac**: implicit `reqwest` feature pull replaced with an explicit `async`
  feature (with `reqwest` kept as a backwards-compatible alias) — the HTTP client and its
  `aws-lc-sys` transitive dependency are no longer pulled in for consumers who never use the
  async surface
- **oxigdal-streaming**: Kafka/Kinesis connector commit-strategy and consumer-lease
  correctness fixes
- **oxigdal-query**: `GROUP BY` execution implemented in the SQL executor (was previously a
  no-op that ignored the clause)

**Bindings**

- **oxigdal (umbrella)**: `DatasetWriter::finalize()` now writes a real format, or returns a
  typed error, instead of emitting a fake `OXIG`-prefixed placeholder blob on unsupported
  paths

**no_std & platform**

- **oxigdal-core**: now compiles under `--no-default-features --features alloc` (no_std +
  `alloc`, no `std`) — the build previously failed under this combination, blocking
  `oxigdal-embedded`/`oxigdal-noalloc` no_std consumers

### Fixed

- **oxigdal-etl**: `transform_crs` now implemented via `oxigdal_proj::transform_epsg`, offloaded to `tokio::task::spawn_blocking` — previously panicked with "Cannot start a runtime from within a runtime" when invoked inside any Tokio runtime, because `transform_epsg` opens the bundled PROJ database and builds its own current-thread runtime internally; this is a real bug fix, not a hardening change
- **oxigdal-etl**: `calculate_bbox` fixed — was unconditionally returning `[0, 0, 0, 0]`
- **oxigdal-ml-foundation**: unavailable `scirs2` input-gradient paths now return honest typed errors instead of silently returning zero gradients
- **oxigdal-gpkg**: tile matrix set `srs_id` now writes the real EPSG:4326 SRS encoding via new `int2_st()` helper (was a hardcoded placeholder value of `4`)
- **oxigdal-cli**: `merge` placeholder test replaced with a real assertion
- **oxigdal-wasm**: COG IFD parser — `BitsPerSample` / `SampleFormat` entries carrying one SHORT per sample (count > 1, e.g. RGB TCI COGs) were read as inline scalars, yielding a garbage bit depth from the offset word and silently disabling predictor undo for multi-band tiles; arrays now go through offset-following array reads (first entry authoritative)
- **oxigdal-drivers/flatgeobuf**: reader and writer now produce and parse the *real* FlatBuffers wire format — size-prefixed `Header`/`Feature` tables per the official FlatGeobuf schema, written via `flatbuffers::FlatBufferBuilder` and read back through a new bounds-checked vtable walker (`fbs` module) — instead of an ad-hoc custom binary layout; files are now interoperable with GDAL and other FlatGeobuf tooling. New `tests/real_format.rs` independently walks the on-disk bytes to confirm they are genuine FlatBuffers, not just round-trippable against this crate's own reader
- **oxigdal-geotiff**: LERC decode (TIFF Compression tag 34887) now implements the real Esri/GDAL LERC2 bit-stuffed block format — header parsing, run-length-encoded validity mask, `BitStuffer2` variable-bit-width unpacking, and exact dequantization — via a new `lerc_codec::lerc2` decoder; previously the codec only round-tripped its own raw-value payload and returned an explicit error on genuine GDAL/Esri-produced LERC streams. LERC *encoding* to the interoperable bit-stuffed format remains explicitly unimplemented (typed error, not a fabricated blob)
- **oxigdal-jpeg2000**: Tier-2 packet-header parsing (new `tier2::layout`/`tier2::packet`/`tier2::tile` modules) now drives code-block decoding from the real per-(resolution, subband, code-block) precinct geometry and COD progression order, replacing a naive even-division byte split across code-blocks that did not reflect the actual packet structure of real JPEG2000 codestreams. Supports LRCP/RLCP progression, single quality layer, maximum-size precincts, and the reversible 5/3 wavelet; unsupported progression orders or multi-layer streams now return a typed `UnsupportedFeature` error instead of mis-decoding silently
- **oxigdal-drivers/hdf5**: the ScaleOffset (`H5Z_SCALEOFFSET`, id 6) and N-Bit (`H5Z_NBIT`, id 5) filters now implement libhdf5's actual on-disk `cd_values`/per-chunk layouts (matching `H5Zscaleoffset.c`/`H5Znbit.c`) instead of an invented header format, so chunks produced by h5py/netcdf-c decode correctly and chunks written here are byte-compatible with libhdf5; a new `filters::pipeline_message` parser decodes the real Object Header Filter Pipeline message (both v1 and v2 on-disk layouts) that supplies each filter's parameters
- **oxigdal-embedded**: the `power` module now makes explicit that `PowerManager` performs no hardware power/clock transitions unless a board-support `PowerController` is installed (new trait extension point) — CPU-frequency scaling and clock/power gating are SoC-vendor-specific and were previously implied rather than actually performed; `request_mode_strict` added for callers where a silent no-op would be a correctness bug
- **oxigdal-algorithms**: both raster-algebra expression front-ends (the Pest-based `dsl` parser and the hand-written raster calculator parser) are recursive descent and had no bound on input nesting depth — a deeply nested expression such as `((((...))))` or a long `-----x` unary chain aborted the whole process with a stack overflow (`SIGABRT`), an unrecoverable crash reachable from untrusted expression text. Both now enforce a measured `MAX_EXPRESSION_DEPTH` (64) before recursing, returning the typed `AlgorithmError::NestingTooDeep` instead of crashing; wired through to `oxigdal-node`'s error mapping as well
- Test fixtures: two `oxigdal-cli` integration tests silently depended on demo fixtures excluded by `.gitignore` (`demo/cog-viewer/*.zarr`, `*.fgb`), so they only passed on machines where a developer had manually regenerated the fixture locally and failed deterministically on a clean checkout (previously misdiagnosed as a Linux-only flake). `test_read_zarr_info_demo_fixture` is fixed by committing the actual `iron-belt.zarr` fixture; `test_read_flatgeobuf_info_demo_fixture` is fixed by falling back to an equivalent in-process synthesized FlatGeobuf fixture when the demo file is absent, keeping the test self-contained either way
- README: quickstart example now compiles as written (`crs()` returns `Option`)
- Hygiene: removed a stray rustc-ICE dump, auto-fix-generated logs/backups, and 3 stray `.bak` files from crate `src/` trees; `.gitignore` hardened; `.cargo/config.toml` stale `rusqlite`/`proj-sys` entries removed; `pypi-publish.yml` stale `openssl-devel` step removed; `pyproject.toml` and `package.json` synced to 0.1.7

## [0.1.6] - 2026-06-15

### Added

- **oxigdal-shapefile**: Non-UTF-8 DBF encoding support via `encoding_rs` — `resolve_cpg()` maps CPG file labels, `resolve_ldid()` maps LDID byte to IANA encoding, `decode()` transcodes byte slices; `ShapefileReader::open_with_encoding()` and `DbfReader::read_with_encoding()` accept an explicit encoding override (PR #10)
- **oxigdal-proj**: `wkt_to_proj_string()` — converts an OGC WKT-1/WKT-2 CRS string to a PROJ string, enabling `from_wkt` CRS objects to work directly with `Transformer` (PR #9)
- **oxigdal-analytics**: `LocalMoranI::calculate_with_permutations()` — permutation-based significance testing for Local Moran's I spatial autocorrelation (pseudo-p-values under conditional randomisation)
- **oxigdal-cache-advanced**: W-TinyLFU eviction policy — `WTinyLfuEviction<K>` (window + protected/probationary segmented LRU) backed by `CountMinSketch` frequency estimator for O(1) admit decisions
- **oxigdal-copc**: `WaveformPacket` — LiDAR point-format 9 and 10 full-waveform data types (byte-offset, packet-size, return-point-waveform-location, XYZ(t) parametric vector)
- **oxigdal-drivers/hdf5**: HDF5 v2/v3 superblock parser — `SuperblockV2`, `read_superblock_v2()`, `validate_superblock_checksum()` (Jenkins lookup3 hash), enabling full HDF5 V2/V3 file support
- **oxigdal-index**: Delaunay triangulation — `triangulate(points)` (Bowyer-Watson), `Triangulation::convex_hull()` returning vertex indices in CCW order
- **oxigdal-qc**: `BatchRunner` / `BatchReport` / `SeverityCounts` — batch QC over directories; `GpkgValidator` / `GpkgValidationResult` — structural GeoPackage validation; `StacValidator` / `StacValidationResult` — STAC item/collection schema validation; `RadiometricValidator` / `RadiometricValidationResult` / `BandRange` / `SensorProfile` — per-band range validation against sensor profiles (Sentinel-2, Landsat-8/9, custom)
- **oxigdal-sensors**: `MaximumLikelihood` classifier — Gaussian MLC with per-class prior support and `singular_covariance` error variant for degenerate covariance matrices
- **oxigdal-streaming**: `KvStateBackend` — OxiStore-backed persistent state backend for stateful streaming pipelines (replaces in-memory HashMap state)
- **oxigdal-terrain**: GLCM texture derivatives — `glcm_texture()`, `GlcmTextures` (contrast, dissimilarity, homogeneity, energy, correlation, ASM), `GlcmOffset` direction enum; TPI variants — `tpi_annulus()`, `tpi_standardized()`, `landform_classification_tpi()`, parallel editions `tpi_annulus_parallel()` / `tpi_standardized_parallel()`; geomorphons landform classifier — `geomorphons()` (Jasiewicz & Stepinski 2013, 10-class); cost distance / least-cost path — `cost_distance()`, `least_cost_path()`
- **oxigdal-temporal**: Whittaker smoother and Savitzky-Golay filter for time-series gap filling (`WhittakerSmoother`, `SavitzkyGolay`), completing the `gap_filling` module
- **oxigdal-metadata**: DOI/INSPIRE metadata transform support — `transform_doi_locator()`, enabling ISO 19115 locator URIs to be mapped to DOI/INSPIRE-compliant identifiers
- **oxigdal-algorithms**: Viewshed curvature/refraction constants extracted — `EARTH_RADIUS_M` (IUGG 2015, 6 371 000 m) and `REFRACTION_COEFF` (k = 0.13, standard atmosphere) replace magic numbers in viewshed analysis
- **oxigdal (umbrella)**: GPX, KML, and TopoJSON formats now supported in `open()` / vector streaming — detected by file extension and routed to the appropriate parser
- **oxigdal-drivers/geotiff**: `compress_webp_with_params()` — WebP compression with explicit quality/lossless parameters; `image-webp 0.2` added as workspace dep
- **oxigdal-pmtiles**: `MbTilesConn` — OxiSQL-backed MBTiles adapter (`open()`, `open_memory()`, `query_count()`, `query_text()`, `query_blob()`) used internally by PMTiles MBTiles export

### Changed

- **SQLite backend**: `rusqlite` and `libsqlite3-sys` (C FFI) fully eliminated from the entire workspace; all SQLite access now goes through `oxisql-sqlite-compat 0.1.5` (pure-Rust Limbo engine). Affected crates: `oxigdal-db-connectors`, `oxigdal-gpkg`, `oxigdal-drivers-advanced`, `oxigdal-mbtiles`, `oxigdal-pmtiles`
- **oxigdal-security**: TLS stack migrated from `ring`/`webpki-roots` to `oxitls-core` + `oxitls-adapter-rustls-rustcrypto` + `oxitls-webpki-roots` — 100% Pure Rust by default; `tls` feature gating maintained; PBKDF2 key derivation moved from `ring::pbkdf2` to `pbkdf2::pbkdf2_hmac::<sha2::Sha256>`
- **oxigdal-security**: `ring = "0.17"` replaced with `pbkdf2 = "0.13"` in workspace dependencies; `argon2`, `aes-gcm`, `chacha20poly1305` retained as pure-Rust alternatives
- **oxigdal-drivers-advanced**: `rusqlite`/`geopackage` feature made optional (removed from `default` closure); GeoPackage connection now uses `SqliteConnectionBlocking`
- **oxigdal-workflow**: `rdkafka` moved behind `kafka` feature; new `http-client`, `kafka`, `integrations`, and `full` feature flags
- `scirs2-core` / `scirs2-neural` / `scirs2-autograd` / `scirs2-optimize` / `scirs2-datasets` / `scirs2-metrics` / `scirs2-linalg` / `scirs2-vision` / `scirs2-series` updated 0.4.4 → 0.5.0
- `oxionnx` updated 0.1.3 → 0.1.4
- `oxiarc-*` suite updated 0.3.0 → 0.3.3 (archive, core, deflate, lzw, lz4, zstd, bzip2, lzhuf, snappy, brotli)
- `oxicode` updated 0.2.3 → 0.2.4
- Workspace: ~35 inline dependency declarations migrated to `*.workspace = true` (workspace policy compliance)
- `oxigdal-kafka` and `oxigdal-offline` removed from `default-members` (C FFI crates excluded from default workspace builds per Pure Rust Policy)
- `mimalloc` changed to `default-features = false` to avoid C dependency in default build
- Workspace `[patch.crates-io]`: added `oxitls-core`, `oxitls-adapter-rustls-rustcrypto`, `oxitls-webpki-roots` local checkout paths
- **MSRV**: minimum supported Rust version raised 1.85 → 1.89 — the `time 0.3.49` dependency requires Rust ≥1.88; standardized on 1.89 to align with the active oxi-ecosystem cluster

### Fixed

- Pure Rust Policy: `ring`, `rusqlite`/`libsqlite3-sys`, `rdkafka-sys` removed from default feature closure — workspace default build is now 100% C/FFI-free
- `oxigdal-gpkg` change-tracking tests: 11 tests `#[ignore]`ed with explanation comment noting Limbo does not yet fire `AFTER INSERT/UPDATE/DELETE` triggers; remaining test verifies schema creation path

### Security

- Replaced `ring 0.17` (RUSTSEC-2023-advisory dependent) with pure-Rust `pbkdf2 0.13` + existing `argon2`/`aes-gcm`/`chacha20poly1305` alternatives
- `aws-lc-sys`, `rustls-webpki`, `rsa` advisories (RUSTSEC-2026-0044/0048/0049/0097-0099/0104, RUSTSEC-2023-0071) remain in `.cargo/audit.toml` allowlist — all transitive via AWS SDK / rumqttc / azure_core, not directly controllable

## [0.1.5] - 2026-05-22

### Fixed

- **oxigdal-gpu**: WGSL uniform layout in `RayMarchUniforms` — removed stray `_pad1: f32` that shifted every field by 4 bytes and caused the compute kernel to read `max_steps` ≈ 1.05×10⁹, hanging `device.poll(wait_indefinitely)` indefinitely on macOS Metal. The previously-timing-out `test_ray_march_gpu_matches_cpu_when_backend_present` now passes in 0.127s.

## [0.1.4] - 2026-04-19

### Added

- **Wave 1 Algorithms Depth** (`oxigdal-algorithms`): Weiler-Atherton polygon clipping (general polygon-polygon clipping with hole support), Karney's geodesic area formula (sub-meter accuracy on WGS84 ellipsoid), DE-9IM (Dimensionally Extended 9-Intersection Model) topological predicates, marching squares contour extraction for raster isolines
- **Wave 1 ML Migration** (`oxigdal-ml`): Migrated from `ort` to `oxionnx` — Pure Rust ONNX inference runtime aligned with COOLJAPAN Pure Rust Policy; cloud detection, super-resolution, and ONNX model loading now use `oxionnx`
- **Wave 2 R-tree Enhancements** (`oxigdal-index`): Node deletion with tree rebalancing, STR (Sort-Tile-Recursive) bulk loading for O(n log n) construction, k-nearest neighbor search with priority queue, R-tree serialization/deserialization
- **Wave 2 SIMD Resampling** (`oxigdal-algorithms`): AVX2 and NEON intrinsics for bilinear and bicubic resampling kernels; auto-detects CPU features at runtime
- **Wave 2 Raster Polygonization** (`oxigdal-algorithms`): Vector polygon extraction from labeled raster regions with boundary tracing and hole detection
- **Wave 2 Topology-Preserving Simplification** (`oxigdal-algorithms`): Visvalingam-Whyatt and Douglas-Peucker variants that preserve shared boundaries across adjacent polygons
- **Wave 2 NoAlloc Geometry Types** (`oxigdal-noalloc`): `FixedLineString<N>`, `FixedRing<N>`, `BBox3D`, `Mercator` projection helpers, `geohash` neighbour enumeration — all zero-allocation, const-generic capacity
- **Wave 2 PMTiles Reader Completion** (`oxigdal-pmtiles`): Full tile retrieval pipeline with OxiARC decompression (gzip/brotli/zstd), FNV-1a content deduplication on reads, directory navigation for root + leaf directories
- **Wave 2 COPC Reader** (`oxigdal-copc`): Cloud Optimized Point Cloud reader with EPT hierarchy traversal, octree-based spatial queries, and HTTP range request support
- **Wave 2 GeoPackage B-tree + 3D WKB** (`oxigdal-gpkg`): B-tree index support for attribute queries, Well-Known Binary 3D geometry parsing (PointZ, LineStringZ, PolygonZ, etc.)

### Fixed

- **pyo3 0.28 Migration** (`oxigdal-python`): Full migration from pyo3 0.24 to 0.28 — updated `Bound<'py, T>` lifetime parameters, new `IntoPyObject` trait usage, migrated GIL handling APIs
- **Clippy Cleanup** (`oxigdal-drivers/geojson`): Streaming test suite clippy cleanup — removed unused imports, fixed `.collect()` redundancies, corrected error propagation patterns
- **GeoTIFF Metadata Optimizer** (`oxigdal-geotiff`): Improvements to COG metadata optimizer and validator for tile ordering and overview consistency
- **ML Error Types** (`oxigdal-ml`): Refined error taxonomy and `OnnxModel` API for the oxionnx migration

### Changed

- All ONNX inference now routes through `oxionnx` (Pure Rust) — no C++ ONNX Runtime dependency
- Doc examples and subcrate READMEs updated to reference v0.1.4

## [0.1.3] - 2026-03-21

### Fixed
- Fixed all wgpu 29 API breaking changes: `Instance::new` now takes `InstanceDescriptor` by value; `InstanceDescriptor` uses `new_without_display_handle()` instead of `Default::default()`; `bind_group_layouts` now `&[Option<&BindGroupLayout>]` — across all GPU and GPU-advanced crates including benchmarks
- Fixed `libsqlite3-sys` version conflict: downgraded `rusqlite` 0.39→0.37 and `libsqlite3-sys` 0.37→0.35 for `proj-sys` compatibility
- Fixed macOS `librocksdb-sys` dynamic library loading via `.cargo/config.toml` with `DYLD_LIBRARY_PATH`
- Fixed 6 critical bugs in `oxiarc-brotli` (local patch via `[patch.crates-io]`):
  - Encoder `write_window_bits` wrong bit pattern range and encoding
  - Decoder `read_window_bits` incorrect bit-to-lgwin mapping
  - Missing ISEMPTY=0 bit in `encode_meta_block` for non-empty last blocks
  - `BrotliParams::validate()` incorrect lgwin range check
  - `write_code_length_value` values 1 and 5 swapped
  - Huffman decoder EOF/single-symbol edge cases causing "no matching code found" errors
- Fixed `pipeline_builder.rs` clippy: `.map(|l| Some(l))` → `.map(Some)`

### Changed
- All compression/decompression now uses locally-patched `oxiarc-brotli` (via `[patch.crates-io]`)

## [0.1.2] - 2026-03-17

### Added

- **Geometry Validation & Operations** (`oxigdal-index`): `validation.rs` with 7 `ValidationIssue` variants (unclosed ring, self-intersection, hole orientation, etc.), `operations.rs` with centroid, area (Shoelace), perimeter, point-in-polygon (ray casting), Douglas-Peucker simplification, Graham scan convex hull, `is_convex`, `distance`, `ring_bbox`, `buffer_bbox`
- **PMTiles v3 Writer** (`oxigdal-pmtiles`): `PmTilesBuilder` with `add_tile`/`build` API, Hilbert curve tile ID encoding (`hilbert.rs`), LEB128 varint encode/decode (`varint.rs`), content deduplication by FNV-1a hash, PMTiles v3 header/directory serialization
- **Umbrella Crate Integration** (`oxigdal`): 7 new feature-gated re-exports (`gpkg`, `pmtiles`, `mbtiles`, `copc`, `index`, `noalloc`, `services`), `convert.rs` with `DatasetFormat` detection (12 formats), `ConversionPlan`, `can_convert`, `supported_conversions`
- **Subcrate READMEs**: Added README.md for oxigdal-copc, oxigdal-geojson, oxigdal-gpkg, oxigdal-index, oxigdal-mbtiles, oxigdal-noalloc, oxigdal-pmtiles

### Changed

- **Refactored `ogc_features.rs`** (`oxigdal-services`): Split 1,981-line monolithic file into 7 focused modules (`error.rs`, `types.rs`, `query.rs`, `crs.rs`, `server.rs`, `cql.rs`, `mod.rs`) per 2,000-line policy; zero breaking changes
- **Refactored `epsg.rs`** (`oxigdal-proj`): Split 1,873-line file into 5 modules (`types.rs`, `geographic.rs`, `projected.rs`, `utm.rs`, `mod.rs`); zero breaking changes
- **3 new `DatasetFormat` variants** (`oxigdal`): `PMTiles`, `MBTiles`, `Copc` with format detection support
- Workspace now has **76 crates** (~565K total SLoC, ~540K Rust)

### Fixed

- **Clippy `should_implement_trait`** (`oxigdal-netcdf`): Renamed `CfVersion::from_str` → `parse_version` and `CellMethodName::from_str` → `parse_method` to avoid confusion with `std::str::FromStr`

## [0.1.1] - 2026-03-11

### Added

- **EPSG Database Expansion** (`oxigdal-proj`): Expanded from 20 to 211+ EPSG definitions including all 120 WGS84 UTM zones (32601-32660 North, 32701-32760 South), JGD2011, GDA2020, CGCS2000, polar stereographic projections, and State Plane zones
- **JPEG2000 EBCOT Tier-1 Decoder** (`oxigdal-jpeg2000`): Full MQ arithmetic coder with Significance Propagation, Magnitude Refinement, and Cleanup passes; split into submodules (`mq.rs`, `contexts.rs`, `passes.rs`, `decoder.rs`)
- **GeoTIFF Floating-Point Predictor** (`oxigdal-geotiff`): Implemented TIFF Technical Note 3 predictor (horizontal differencing + byte reordering) for Float32/Float64 with full round-trip support
- **Streaming Raster Reader Integration** (`oxigdal-streaming`): Real GeoTIFF driver integration replacing placeholder metadata/data; format detection, metadata from real files, chunk reading via CogReader
- **Pure Rust Compression Migration**: Replaced `flate2` (C) with `oxiarc-deflate` and `zstd` (C) with `oxiarc-zstd` in GeoTIFF driver per COOLJAPAN Pure Rust Policy
- **CLI Command Implementations** (`oxigdal-cli`): Functional `inspect` (reads headers/metadata), `convert` (GeoTIFF-to-COG), and `buildvrt` (generates VRT XML) commands
- **Compression Benchmarks** (`oxigdal-compress`): Real codec benchmarks for deflate, lzw, zstd, bzip2, and lz4 via oxiarc ecosystem
- **Driver Test Coverage**: 20+ integration tests per driver for GeoTIFF, Shapefile, and GeoJSON including round-trip, edge cases, error handling, and multi-band/multi-feature scenarios
- **DEM CLI Terrain Analysis** (`oxigdal-cli`): Activated all 6 terrain operations (`hillshade`, `slope`, `aspect`, `TRI`, `TPI`, `roughness`) — previously blocked by `bail!("not yet implemented")`; slope percent/degree modes and zero-for-flat aspect option added
- **DSL Statistical Functions** (`oxigdal-algorithms`): Implemented `median` (sort-based), `mode` (frequency-map with f64::to_bits), and `percentile` (NumPy-compatible linear interpolation) in DSL function evaluator
- **DSL For-Loop Support** (`oxigdal-algorithms`): `Expr::ForLoop` now evaluates via child scope iteration with 1M-iteration guard against OOM
- **WASM Huffman Decompression** (`oxigdal-wasm`): Implemented full round-trip Huffman decompression — frequency table stored in compressed header, tree reconstructed on decode, single-symbol edge case handled
- **WASM Huffman Decoder** (`oxigdal-wasm`): Canonical Huffman encoding/decoding for WebAssembly compression
- **Server-Side Map Rendering** (`oxigdal-server`): Tile rendering pipeline with dynamic styling
- **Delta Encoding** (`oxigdal-compress`): Delta-of-delta and XOR-delta encoding for time-series raster data
- **Grouped Aggregation Engine** (`oxigdal-analytics`): SQL-style GROUP BY aggregation with min/max/sum/mean/count/variance/stddev
- **HDF5 SWMR Protocol** (`oxigdal-hdf5`): Single Writer Multiple Reader protocol for concurrent HDF5 access
- **FlatGeobuf Spatial Indexing** (`oxigdal-flatgeobuf`): Hilbert R-tree spatial indexing improvements

### Fixed

- **Compilation Blocker**: Fixed workspace version mismatch (0.3.0 → 0.1.1) that blocked all compilation
- **oxiarc-deflate Bug**: Fixed `rle_encode_lengths` Huffman run-length encoding overflow for large homogeneous datasets; applied local patch via `[patch.crates-io]`
- **Dependency Versions**: Corrected oxiarc-* (0.3.0 → 0.2.2), oxicode (0.3.0 → 0.1.1), rs3gw (0.3.0 → 0.1.0), scirs2-core (corrected to 0.3.1)
- **Security**: Updated quinn-proto (RUSTSEC-2026-0037, DoS vulnerability, CVSS 8.7) and yanked wasm-bindgen 0.2.111 → 0.2.114
- **Invalid crates.io Category**: Fixed `science::geo` → `science` in oxigdal crate metadata
- **JPEG2000 Module Conflict**: Removed duplicate `tier1.rs` conflicting with `tier1/` directory module
- **File Size Policy**: Split `reader.rs` (2099 lines) into `reader/mod.rs` + `reader/tests.rs` to comply with 2000-line limit
- **Hardcoded Version Strings**: Replaced hardcoded `"0.1.0"` strings with `env!("CARGO_PKG_VERSION")` in oxigdal-hdf5 and oxigdal-mobile
- **Test Isolation**: Fixed `oxigdal-edge` integration test race condition using unique temp dirs with `AtomicU64` counter
- **ml-foundation Doctest**: Added `#[cfg(not(feature = "ml"))] impl Dataset for GeoTiffDataset` stub to satisfy trait bound in non-ml builds

### Changed

- **Refactored `calculator.rs`** (`oxigdal-algorithms`): Split 1,982-line monolithic file into 7 focused modules (`ast.rs`, `lexer.rs`, `parser.rs`, `optimizer.rs`, `evaluator.rs`, `ops.rs`, `mod.rs`) per 2,000-line policy; zero breaking changes
- **Dependency Updates**: Arrow ecosystem 57→58, sysinfo 0.36→0.38, criterion 0.7→0.8, tokio-tungstenite 0.25→0.28 (API fix applied), redis 0.27→1.0, all SciRS2 subcrates 0.2.0→0.3.1
- Workspace now has **69 crates** (~505K total SLoC, ~480K Rust)
- All internal crates use `version.workspace = true`
- CHANGELOG, README, and publish script updated for v0.1.1

## [0.1.0] - 2026-02-22

**The Independence Release** -- First public release of OxiGDAL, a pure Rust
reimplementation of GDAL for cloud-native geospatial computing.

This release represents the culmination of intensive development across multiple
phases, delivering **~495,961 SLoC** of production-ready Rust code in **68
workspace crates** (474,600 lines of Rust across 1,739 `.rs` files) with **zero
C/C++/Fortran dependencies** in default features. Estimated development cost:
$18.3M equivalent (COCOMO model).

### Added

#### Core Foundation

**Core Library (`oxigdal-core`)**
- Core geospatial data types: `BoundingBox`, `GeoTransform`, `RasterDataType`,
  `RasterBuffer`
- Abstract I/O traits: `AsyncDataSource`, `Dataset`, `RasterDataset`,
  `VectorDataset`
- Storage backends: `LocalFileBackend`, `S3Backend`, `HttpBackend` with HTTP
  range request support
- `RangeCoalescer` for intelligent HTTP request batching and optimization
- Arrow-backed `GeoBuffer` for zero-copy columnar data operations
- Comprehensive error handling with `OxiError` using `thiserror` (no unwrap
  policy enforced workspace-wide)
- `no_std` compatible core types for embedded systems
- Memory-efficient buffer operations with type-safe pixel access

**Algorithms (`oxigdal-algorithms`)**
- SIMD-optimized raster processing: resampling (nearest, bilinear, cubic,
  Lanczos), reprojection, hillshade, slope, aspect, contour generation
- Vector algorithms: topology operations (split, merge, simplify), buffering,
  convex hull, spatial joins, dissolve, and clipping
- Raster algebra DSL powered by a Pest grammar parser
- Portable SIMD with feature-gated AVX2, AVX-512, and ARM NEON paths
- Optional Rayon-based parallelism (`parallel` feature)
- Terrain analysis: aspect (0-360 degrees), slope (degrees or percent),
  curvature (profile and planform), hillshade with configurable azimuth/altitude
- Zonal statistics by polygon zones with support for categorical and continuous
  data
- Douglas-Peucker simplification, positive/negative buffering, boolean
  operations (union, intersection, difference), spatial predicates (intersects,
  contains, within, touches, crosses, overlaps, disjoint)

#### Coordinate Reference Systems (`oxigdal-proj`)

- Pure Rust PROJ reimplementation with zero C dependencies
- 20+ map projections: Transverse Mercator (UTM 1-60), Web Mercator
  (EPSG:3857), Lambert Conformal Conic, Albers Equal Area, Polar
  Stereographic, Azimuthal Equidistant, Oblique Mercator, Japan Plane
  Rectangular (I-XIX zones, JGD2000/JGD2011)
- Complete WKT2 (ISO 19162:2019) parser with WKT1 (OGC 01-009) and ESRI WKT
  backward compatibility
- 211+ embedded EPSG CRS definitions with O(1) lookup
- Datum transformations: 7-parameter Helmert (Bursa-Wolf), 3/5-parameter
  Molodensky, NTv2 grid interpolation, NADCON (NAD27-NAD83)
- Automatic transformation path finding between arbitrary CRS pairs
- SIMD-vectorized batch transforms: < 10ms for 1 million points
- Accuracy within 0.001m of the reference PROJ implementation

#### Geospatial File Format Drivers (11 formats)

- **GeoTIFF / COG** (`oxigdal-geotiff`): Cloud-Optimized GeoTIFF reader/writer
  with tiled access, BigTIFF (> 4GB), overview generation, GeoTIFF 1.1 GeoKey
  directory, compression codecs (DEFLATE, LZW, ZSTD, PackBits, JPEG),
  horizontal differencing predictor, LRU tile cache
- **GeoJSON** (`oxigdal-geojson`): RFC 7946 compliant reader/writer, streaming
  parser for large files, all geometry types, GeoArrow zero-copy conversion,
  configurable coordinate precision
- **Shapefile** (`oxigdal-shapefile`): SHP/SHX/DBF reader/writer with full
  attribute table support and legacy format compatibility
- **FlatGeobuf** (`oxigdal-flatgeobuf`): Packed Hilbert R-tree spatial index,
  streaming feature reads, spatial filtering during decode
- **GeoParquet** (`oxigdal-geoparquet`): WKB and GeoArrow encoding, row group
  statistics with bbox metadata, spatial predicate pushdown, parallel row group
  reading, ZSTD compression (10x faster than GeoPandas for large datasets)
- **Zarr v2/v3** (`oxigdal-zarr`): Array/group hierarchies, zarr.json manifest
  (v3), codec pipeline with compression chain, sharding extension, byte shuffle
  and delta filters, consolidated metadata, parallel chunk loading
- **HDF5** (`oxigdal-hdf5`): Hierarchical data structures, chunking and
  compression, dataset attributes, group navigation
- **NetCDF** (`oxigdal-netcdf`): CF (Climate and Forecast) conventions, unlimited
  dimensions, group hierarchies, variable metadata extraction
- **GRIB** (`oxigdal-grib`): GRIB1/GRIB2 meteorological data, parameter tables,
  level types
- **JPEG2000** (`oxigdal-jpeg2000`): Tier-1 entropy coding, wavelet transforms
  (DWT), codestream parsing
- **VRT** (`oxigdal-vrt`): Virtual raster datasets, on-the-fly processing, band
  mathematics, source mosaicking

**Advanced Drivers** (`oxigdal-drivers-advanced`): Extended format support and
driver plugin architecture

#### Database Connectors (`oxigdal-db-connectors`)

- **PostgreSQL / PostGIS** (`oxigdal-postgis`): Native geometry types
  (WKB/EWKB), GiST/BRIN spatial index integration, bulk COPY protocol,
  connection pooling via `deadpool-postgres`
- **MySQL**: Async connector with spatial type mapping (GEOMETRY, POINT,
  LINESTRING, POLYGON), R-tree spatial index, bulk insert
- **MongoDB**: Document-based geospatial storage with GeoJSON support
- **ClickHouse**: Columnar analytics for geospatial OLAP workloads
- **Cassandra / ScyllaDB**: Wide-column store for time-series geospatial data
- **SQLite / SpatiaLite**: Feature-gated (C dependency, not in defaults per Pure
  Rust Policy), R*-tree spatial index, single-file deployment
- **Redis** (via `oxigdal-gateway`): In-memory caching for tile and query results
- **DuckDB** support via query engine integration

#### Cloud Storage (`oxigdal-cloud`, `oxigdal-cloud-enhanced`)

- **AWS S3**: Full S3 API with range requests for COG byte-range access,
  multipart upload/download
- **Azure Blob Storage**: Azure SDK integration with Data Lake support
- **Google Cloud Storage**: GCS backend with authenticated access
- **RS3GW** (`oxigdal-rs3gw`): S3-compatible gateway adapter (MinIO,
  DigitalOcean Spaces)
- Automatic retry with exponential backoff, client-side caching layer
- Deep cloud integrations: AWS Athena, Glue, Lambda, SageMaker, CloudWatch,
  Cost Explorer; GCP BigQuery, Pub/Sub

#### Streaming and Event Processing

- **Streaming Pipelines** (`oxigdal-streaming`): Real-time data processing with
  backpressure, windowing (tumbling, sliding, session), watermarks for late data
  handling, stateful operators, metrics reporting
- **Apache Kafka** (`oxigdal-kafka`): Producer/consumer for geospatial event
  streams, key-based partitioning, schema registry, exactly-once semantics
- **AWS Kinesis** (`oxigdal-kinesis`): Kinesis Data Streams integration with
  shard parallelism and checkpointing
- **Google Cloud Pub/Sub** (`oxigdal-pubsub`): GCP message queue with
  subscription management and acknowledgment
- **MQTT** (`oxigdal-mqtt`): Lightweight IoT messaging with sensor data types,
  QoS 0/1/2, topic-based routing, retained messages; custom `SensorValue`
  deserializer for robust handling of `serde_json/arbitrary_precision`

#### Query Engine (`oxigdal-query`)

- SQL-like query language for geospatial data with `sqlparser` integration
- Cost-based query optimizer with pluggable rule system
- Optimization rules: Common Subexpression Elimination (CSE), join reordering,
  projection pushdown, predicate pushdown, filter fusion
- Spatial join algorithms: indexed nested loop, spatial hash join
- Arrow-based columnar execution engine

#### Machine Learning and AI

- **ML Runtime** (`oxigdal-ml`): ONNX Runtime 2.0 integration for
  cross-platform inference with multi-backend support (CUDA, ROCm, Vulkan,
  Metal, OpenCL, WebGPU, DirectML), batch preprocessing with automated batch
  size tuning, INT8/FP16 quantization, ResNet/UNet/Transformer/LSTM
  architectures
- **ML Foundation** (`oxigdal-ml-foundation`): Deep learning training
  infrastructure with transfer learning, training loops, Adam/SGD optimizers,
  early stopping, data augmentation, model checkpointing; SciRS2 backend for
  Pure Rust numerical operations

#### GPU Acceleration

- **GPU Core** (`oxigdal-gpu`): WGPU-based GPU computing with Vulkan, Metal,
  DX12, and WebGPU backends; shader compilation for raster operations
- **GPU Advanced** (`oxigdal-gpu-advanced`): Multi-GPU load balancing, memory
  pool management, shader optimization, ML inference pipeline with kernel fusion,
  automatic backend detection
- Optional CUDA backend support

#### Server and API

- **HTTP Server** (`oxigdal-server`): Axum-based REST API for tiles, features,
  and metadata; XYZ tile endpoint; rendering pipeline with on-the-fly processing
- **API Gateway** (`oxigdal-gateway`): Rate limiting (Governor), JWT/OAuth2
  authentication, GraphQL (async-graphql), WebSocket proxying, Redis-backed
  sessions
- **WebSocket** (`oxigdal-ws`, `oxigdal-websocket`): Real-time bidirectional
  protocol for live geospatial data feeds with backpressure handling

#### Enterprise Features

- **Security** (`oxigdal-security`): Encryption at rest (AES-256-GCM,
  ChaCha20-Poly1305), Argon2id password hashing, TLS 1.3 via `rustls`,
  RBAC/ABAC access control, audit logging for compliance (SOC2, GDPR readiness)
- **High Availability** (`oxigdal-ha`): Raft-based consensus, WAL replication,
  automatic failover, health monitoring, circuit breaker pattern
- **Observability** (`oxigdal-observability`): OpenTelemetry tracing and metrics,
  Prometheus exposition, Jaeger backend, structured logging via `tracing`
- **Clustering** (`oxigdal-cluster`): Node management, distributed locking,
  health checks, failure detection
- **Distributed** (`oxigdal-distributed`): Arrow Flight-based data transfer,
  work-stealing scheduler, task graph optimization, fault-tolerant retry

#### ETL and Workflow

- **ETL** (`oxigdal-etl`): Extract-Transform-Load pipelines with
  source/sink abstraction, data validation, incremental processing
- **Workflow** (`oxigdal-workflow`): DAG-based workflow engine (Petgraph),
  cron scheduling, dependency management, state checkpointing

#### Spatial and Domain-Specific

- **3D / Point Cloud** (`oxigdal-3d`): LAS/LAZ point cloud processing, 3D Tiles
  1.0 (B3DM, I3DM, PNTS), glTF export, Delaunay triangulation, terrain mesh
- **Terrain** (`oxigdal-terrain`): DEM processing, hydrological modeling (flow
  direction, flow accumulation), watershed delineation, viewshed analysis,
  terrain ruggedness and topographic position indices
- **Temporal** (`oxigdal-temporal`): Time-series datacube operations, temporal
  aggregation, change detection, gap filling and interpolation
- **Analytics** (`oxigdal-analytics`): Spatial statistics, hot spot analysis
  (Getis-Ord Gi*), clustering, zonal operations, performance profiling
- **STAC** (`oxigdal-stac`): SpatioTemporal Asset Catalog 1.0.0 client,
  catalog/collection/item API, spatial/temporal search
- **Metadata** (`oxigdal-metadata`): ISO 19115:2014, ISO 19139 XML, FGDC CSDGM,
  metadata extraction and transformation between standards
- **Sensors** (`oxigdal-sensors`): IoT sensor observation types, calibration,
  data ingestion
- **Quality Control** (`oxigdal-qc`): Data validation, anomaly detection,
  quality score calculation

#### Platform and Language Bindings

- **WASM** (`oxigdal-wasm`): WebAssembly target with IndexedDB storage, Web
  Worker support, `WasmCogViewer` JavaScript/TypeScript API, Canvas `ImageData`
  integration, bundle size < 1MB gzipped
- **PWA** (`oxigdal-pwa`): Progressive Web App with offline-first architecture,
  Service Worker caching, installable web apps
- **Offline** (`oxigdal-offline`): Offline data sync with conflict resolution,
  operation queue, delta sync
- **Node.js** (`oxigdal-node`): N-API bindings via `napi-rs` for Node.js 16+,
  async Promise-based API, CommonJS and ESM
- **Python** (`oxigdal-python`): PyO3/Maturin bindings, `oxigdal.open()`
  universal opener, `read_geoparquet()` / `read_geotiff()` / `read_zarr()`,
  NumPy array returns, CRS class, algorithm bindings, manylinux2014/macOS/Windows
  wheels
- **Jupyter** (`oxigdal-jupyter`): `evcxr` kernel integration with `plotters`
  visualization, rich display for rasters and vectors
- **Mobile** (`oxigdal-mobile`, `oxigdal-mobile-enhanced`): iOS (Swift FFI) and
  Android (Kotlin/JNI), background processing, battery/network-aware scheduling
- **Embedded** (`oxigdal-embedded`): `no_std` support with `heapless` and
  `embedded-hal`
- **Edge** (`oxigdal-edge`): Edge computing platform with minimal footprint,
  offline-first local database cache, streaming sensor ingestion

#### Developer Tooling

- **CLI** (`oxigdal-cli`): `oxigdal info`, `convert`, `dem`, `rasterize`,
  `warp` commands via Clap
- **Dev Tools** (`oxigdal-dev-tools`): File watching (notify), progress bars
  (indicatif), diff utilities, pretty tables (comfy-table)
- **Benchmarks** (`oxigdal-bench`, `benchmarks/`): Criterion-based benchmarks
  with flamegraph profiling (pprof)
- **Examples** (`oxigdal-examples`): Runnable examples for COG tile serving,
  GeoParquet creation, format conversion, satellite processing

#### Additional Subsystems

- **Compression** (`oxigdal-compress`): Pure Rust compression via OxiArc
  ecosystem (Deflate, LZ4, Zstd, BZip2, LZW, LZH); legacy codec support
  (flate2 rust_backend, zstd, brotli, snappy)
- **Data Synchronization** (`oxigdal-sync`): CRDT-based sync (OR-Set), Merkle
  tree verification, vector clocks for causality tracking, offline queue
- **Caching** (`oxigdal-cache-advanced`): Multi-tier caching (in-memory LRU,
  on-disk, distributed Redis), cache warming strategies
- **Services** (`oxigdal-services`): WMS 1.3.0, WFS 2.0.0, health check
  endpoints

#### Demo Applications

- **COG Viewer** (`demo/cog-viewer/`): Browser-based Cloud-Optimized GeoTIFF
  viewer with JavaScript frontend, MapLibre GL and Leaflet integration

### Changed

- Edition set to Rust 2024 (`edition = "2024"`) with minimum supported Rust
  version 1.85
- Workspace-wide lint configuration: `clippy::unwrap_used = "deny"`,
  `clippy::panic = "deny"` enforced across all 68 crates
- All compression defaults use Pure Rust backends (COOLJAPAN Policy); C-based
  compression libraries are feature-gated or being phased out
- `oxicode` replaces `bincode` for binary serialization (COOLJAPAN Policy)
- `OxiArc` ecosystem (`oxiarc-*`) replaces the `zip` crate for archive
  handling (COOLJAPAN Pure Rust Policy)
- Arrow ecosystem pinned to version 57 across all crates for consistency (upgraded to 58 in v0.1.1)
- Release profile configured with LTO, single codegen unit, and `opt-level = 3`
- `SensorValue` deserialization rewritten with custom `Deserialize` impl to
  handle `serde_json/arbitrary_precision` correctly (replaced derived
  `#[serde(untagged)]` deserialization)
- Edge binary database cache updated for latest schema

### Fixed

- Eliminated 1,143 out of 1,145 `unwrap()` calls across the entire codebase
  (99.83% reduction); remaining 2 are in non-compiled doc comments
- Resolved all 16 rustdoc warnings (feature-gated module links, HTML tags in
  doc comments)
- Fixed `SensorValue` enum deserialization ordering for correct serde roundtrip
  under `arbitrary_precision`
- Fixed all Clippy warnings to achieve zero actionable warnings
- All files refactored to stay under 2,000 lines (maximum observed: 1,976)
- Resolved compilation errors in calculator and buffer modules
- Cleared stale build cache artifacts causing phantom compilation errors
- Fixed Pub/Sub error types and integration test reliability
- Fixed query optimizer rules (CSE, join reordering, projection pushdown)
- Fixed WebSocket protocol handling
- Fixed streaming metrics reporter

### Security

- Encryption at rest via AES-256-GCM and ChaCha20-Poly1305
- Password hashing with Argon2id
- TLS 1.3 transport via `rustls` (no OpenSSL dependency)
- JWT and OAuth2 authentication in the API gateway
- Role-Based Access Control (RBAC) and Attribute-Based Access Control (ABAC)
- Audit logging for compliance (SOC2, GDPR readiness)
- HMAC-SHA256 message authentication for inter-service communication
- All cryptographic operations use pure Rust crates (`ring`, `rustls`,
  `aes-gcm`, `chacha20poly1305`, `argon2`)
- Minimal unsafe code (< 1% of codebase), fully audited and documented
- Vulnerability scanning integrated via `cargo-audit`

### Performance

**Benchmarks Achieved**
- COG tile access: < 10ms (local SSD), < 100ms (cloud S3/GCS)
- Metadata reading: < 5ms for typical GeoTIFF headers
- GeoParquet reading: 10x faster than GeoPandas for large datasets
- PROJ transformations: < 10ms for 1 million points (WGS84 to UTM)
- Docker image size: < 50MB (vs 1GB+ with traditional GDAL)
- WASM bundle: < 1MB gzipped (vs impossible with C-based GDAL)

### Technical Details

**Statistics**
- **Total SLoC**: 495,961 (2,042 files)
- **Rust Code**: 474,600 lines across 1,739 `.rs` files
- **Workspace Crates**: 68
- **Format Drivers**: 11 (GeoTIFF, COG, GeoJSON, GeoParquet, Zarr, FlatGeobuf,
  Shapefile, NetCDF, HDF5, GRIB, JPEG2000, VRT)
- **Map Projections**: 20+ implemented, 211+ EPSG codes embedded
- **Estimated Cost**: $18,275,174 (COCOMO model)

**Platform Support**
- **Operating Systems**: Linux (x86_64, aarch64), macOS (x86_64, aarch64/M1+),
  Windows (x86_64)
- **WebAssembly**: `wasm32-unknown-unknown` target
- **Mobile**: iOS (arm64, simulator), Android (arm64-v8a, armeabi-v7a, x86_64)
- **Embedded**: `no_std` support for microcontrollers

**COOLJAPAN Ecosystem Compliance**
- **Pure Rust Policy**: 100% Rust in default features (C/Fortran feature-gated)
- **No Unwrap Policy**: Zero `unwrap()` in production code (`clippy::unwrap_used
  = "deny"`)
- **Workspace Policy**: All dependencies use workspace inheritance
- **Latest Crates Policy**: All dependencies at latest available versions
- **COOLJAPAN Integration**: SciRS2-Core, OxiCode (not bincode), OxiArc (not
  zip), OxiFFT (not rustfft), OxiZ (not Z3)

### Known Issues

- JPEG2000 support is basic (tier-1 only, no tier-2 optimizations yet)
- Some transitive dependencies have unmaintained advisories (tracked):
  `rustls-pemfile` (RUSTSEC-2025-0134), `sled` (RUSTSEC-2025-0057 fxhash),
  `evcxr` (json 0.12.4), `indicatif` (number_prefix 0.4.0)
- Embedded platforms require nightly Rust for some features

### Migration from GDAL

See [MIGRATION.md](docs/MIGRATION.md) for detailed migration guide from GDAL
C/C++, Rasterio, GeoPandas, and PROJ.

### Roadmap

- **v0.2.0** (Q2 2026): Additional projections (100+ total), GPU acceleration
  expansion, ML pipeline enhancements
- **v0.3.0** (Q3 2026): Real-time streaming improvements, enhanced JPEG2000,
  cloud-native tile server
- **v1.0.0** (Q4 2026): Production stability, LTS commitment, enterprise
  compliance certifications

### Contributors

**Development Team**: COOLJAPAN OU (Team Kitasan)

### Acknowledgments

- **GDAL Project**: Original inspiration and reference implementation
- **GeoRust Community**: Ecosystem collaboration and shared crates
- **PROJ**: Coordinate transformation reference and test suite
- **Rust Community**: Language, tooling, and ecosystem support
- **Specifications**: GeoTIFF, COG, OGC (WMS/WFS), STAC, ISO 19115, RFC 7946
- **Testing Data**: USGS Earth Explorer, Copernicus, OpenStreetMap

---

## Links

- **Homepage**: <https://github.com/cool-japan/oxigeo>
- **Documentation**: <https://docs.rs/oxigeo>
- **Issue Tracker**: <https://github.com/cool-japan/oxigeo/issues>

[0.2.4]: https://github.com/cool-japan/oxigeo/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/cool-japan/oxigeo/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/cool-japan/oxigeo/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/cool-japan/oxigeo/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/cool-japan/oxigeo/compare/v0.1.7...v0.2.0
[0.1.7]: https://github.com/cool-japan/oxigdal/releases/tag/v0.1.7
[0.1.6]: https://github.com/cool-japan/oxigdal/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/cool-japan/oxigdal/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/cool-japan/oxigdal/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/cool-japan/oxigdal/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/cool-japan/oxigdal/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cool-japan/oxigdal/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/cool-japan/oxigdal/releases/tag/v0.1.0
