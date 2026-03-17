# TODO: oxigdal-geotiff

## High Priority
- [ ] Implement JPEG compression codec (currently placeholder, `jpeg` feature)
- [ ] Implement WebP compression codec (currently placeholder, `webp` feature)
- [ ] Add BigTIFF write support (currently read-only for >4GB files)
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
