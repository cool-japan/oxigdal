# TODO: oxigdal-geotiff

## High Priority
- [ ] Implement JPEG compression codec (currently placeholder, `jpeg` feature)
- [ ] Implement WebP compression codec (currently placeholder, `webp` feature)
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
