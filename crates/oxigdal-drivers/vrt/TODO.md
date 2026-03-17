# TODO: oxigdal-vrt

## High Priority
- [ ] Implement lazy tile reading from source rasters (currently builder-focused)
- [ ] Add pixel function evaluation (apply expressions to source bands on-the-fly)
- [ ] Implement warped VRT support (on-the-fly reprojection from source CRS)
- [ ] Add multi-source compositing with priority/overlap resolution
- [ ] Implement VRT validation (check source file existence, dimension consistency)
- [ ] Add GDAL VRT XML compatibility testing (round-trip with GDAL-generated VRTs)
- [ ] Implement partial band reading (window/region extraction from virtual dataset)

## Medium Priority
- [ ] Add kernel-based pixel functions (convolution, statistics in moving window)
- [ ] Implement source band caching with LRU eviction for repeated tile access
- [ ] Add VRT from directory (auto-mosaic all GeoTIFFs in a folder)
- [ ] Implement derived band support (band math expressions on source bands)
- [ ] Add VRT update-in-place (add/remove sources without full rewrite)
- [ ] Implement nodata handling across source boundaries
- [ ] Add color table and color interpretation inheritance from sources
- [ ] Implement overview-level VRT (source overview selection by resolution)

## Low Priority / Future
- [ ] Add VRT for vector data (OGR VRT equivalent)
- [ ] Implement VRT-based time series (temporal dimension from file list)
- [ ] Add cloud-native VRT (reference HTTP/S3 sources with byte ranges)
- [ ] Implement VRT diff (compare two VRT definitions)
- [ ] Add VRT optimization (merge adjacent sources, remove redundant bands)
- [ ] Implement VRT to COG conversion (materialize virtual dataset as tiled GeoTIFF)
- [ ] Add Python-callable pixel functions via embedded interpreter
- [ ] Implement VRT chaining (VRT referencing other VRTs)
